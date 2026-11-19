//! Direct bounded output sinks: backpressure reaches the guest instead of an
//! unbounded intermediate stdout pipe. Legacy Stage 0 capture is unchanged.
use crate::service_input::{InputQueue, TerminalBridge};
use std::{
    collections::VecDeque,
    fmt, io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use wasmdroid_engine::logs::live::{CHUNK_BYTES, LogStream, LogWriter};
use wasmdroid_engine::terminal::{FrameQuery, Terminal};
use wasmer_wasix::os::tty::TtyBridge;
use wasmer_wasix::virtual_fs::{self, FsError, VirtualFile};
const TAIL_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct Tail {
    bytes: VecDeque<u8>,
    total: u64,
}
impl Tail {
    fn append(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
        self.bytes.extend(bytes);
        if self.bytes.len() > TAIL_BYTES {
            self.bytes.drain(..self.bytes.len() - TAIL_BYTES);
        }
    }
    fn text(&self) -> (String, bool) {
        (
            String::from_utf8_lossy(&self.bytes.iter().copied().collect::<Vec<_>>()).into_owned(),
            self.total > TAIL_BYTES as u64,
        )
    }
}
struct OutputState {
    writer: LogWriter,
    stdout: Tail,
    stderr: Tail,
    failed: bool,
    terminal: Option<Terminal>,
    bridge: Arc<TerminalBridge>,
    replies: Option<InputQueue>,
    terminal_saved: bool,
}
#[derive(Clone)]
pub(crate) struct ServiceOutput(Arc<Mutex<OutputState>>);
impl ServiceOutput {
    pub fn new(
        writer: LogWriter,
        tty: bool,
        bridge: Arc<TerminalBridge>,
        replies: Option<InputQueue>,
    ) -> (Self, OutputSink, OutputSink) {
        let output = Self(Arc::new(Mutex::new(OutputState {
            writer,
            stdout: Tail::default(),
            stderr: Tail::default(),
            failed: false,
            terminal: tty
                .then(|| Terminal::new(25, 80).expect("fixed initial terminal dimensions")),
            bridge,
            replies,
            terminal_saved: false,
        })));
        (
            output.clone(),
            OutputSink {
                output: output.clone(),
                stream: LogStream::Stdout,
                complete_write: false,
            },
            OutputSink {
                output,
                stream: LogStream::Stderr,
                complete_write: false,
            },
        )
    }
    pub fn finish(&self) -> io::Result<((String, bool), (String, bool))> {
        let mut state = self.lock()?;
        state.writer.flush().map_err(io::Error::other)?;
        if state.terminal.is_some() {
            state.terminal_saved = terminal_pages(&mut state)
                .and_then(|pages| state.writer.save_terminal(&pages))
                .is_ok();
        }
        Ok((state.stdout.text(), state.stderr.text()))
    }
    pub fn terminal_handle(&self) -> Option<TerminalHandle> {
        self.lock().ok().and_then(|state| {
            state
                .terminal
                .as_ref()
                .map(|_| TerminalHandle(self.clone()))
        })
    }
    pub fn terminal_saved(&self) -> bool {
        self.lock().is_ok_and(|state| state.terminal_saved)
    }
    pub fn echo_sink(&self) -> OutputSink {
        OutputSink {
            output: self.clone(),
            stream: LogStream::Stdout,
            complete_write: true,
        }
    }
    pub fn status(&self) -> io::Result<()> {
        if self.lock()?.failed {
            Err(io::Error::other("service output storage failed"))
        } else {
            Ok(())
        }
    }
    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, OutputState>> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("output writer lock failed"))
    }
}
/// Private worker display state; frame reads never hold the execution mutex.
#[derive(Clone)]
pub struct TerminalHandle(ServiceOutput);
impl TerminalHandle {
    pub fn frame(&self, json: &str) -> wasmdroid_engine::Result<String> {
        if json.len() > 4096 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "terminal query exceeds limit".into(),
            ));
        }
        let query: FrameQuery = serde_json::from_str(json)?;
        let mut state = self.0.lock()?;
        sync_dimensions(&mut state)?;
        state
            .terminal
            .as_mut()
            .ok_or_else(|| {
                wasmdroid_engine::EngineError::Unsupported("no terminal display".into())
            })?
            .page_json(&query)
    }
}
fn sync_dimensions(state: &mut OutputState) -> wasmdroid_engine::Result<()> {
    let current = state.bridge.tty_get();
    if let Some(terminal) = &mut state.terminal {
        terminal.resize(current.rows as u16, current.cols as u16)?;
    }
    Ok(())
}
fn terminal_pages(
    state: &mut OutputState,
) -> wasmdroid_engine::Result<Vec<wasmdroid_engine::terminal::TerminalPage>> {
    sync_dimensions(state)?;
    let terminal = state.terminal.as_mut().unwrap();
    let mut query = FrameQuery::default();
    let mut pages = Vec::new();
    loop {
        let page = terminal.page(&query)?;
        query.token = Some(page.token);
        query.start_row = page.start_row + page.cells.len() as u16;
        let more = page.has_more;
        pages.push(page);
        if !more {
            break;
        }
    }
    Ok(pages)
}
pub(crate) struct OutputSink {
    output: ServiceOutput,
    stream: LogStream,
    complete_write: bool,
}
impl fmt::Debug for OutputSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputSink")
            .field("stream", &self.stream)
            .finish_non_exhaustive()
    }
}
impl AsyncWrite for OutputSink {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.output.lock()?;
        let mut written = 0;
        while written < bytes.len() {
            let count = match state.writer.append(self.stream, &bytes[written..]) {
                Ok(count) => count,
                Err(error) => {
                    state.failed = true;
                    return Poll::Ready(Err(io::Error::other(error)));
                }
            };
            match self.stream {
                LogStream::Stdout => state.stdout.append(&bytes[written..written + count]),
                LogStream::Stderr => state.stderr.append(&bytes[written..written + count]),
            };
            let options = state.bridge.tty_get();
            if let Some(terminal) = state.terminal.as_mut() {
                terminal
                    .resize(options.rows as u16, options.cols as u16)
                    .map_err(io::Error::other)?;
                let reply = terminal.process(&bytes[written..written + count], options.line_feeds);
                if !reply.is_empty()
                    && state
                        .replies
                        .as_ref()
                        .is_some_and(|queue| !queue.try_reply(&reply))
                {
                    state.terminal.as_mut().unwrap().mark_reply_overflow();
                }
            }
            written += count;
            if !self.complete_write {
                break;
            }
        }
        Poll::Ready(Ok(written))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.output.lock()?.writer.flush().map_err(io::Error::other))
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}
impl AsyncRead for OutputSink {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()))
    }
}
impl AsyncSeek for OutputSink {
    fn start_seek(self: Pin<&mut Self>, _: io::SeekFrom) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::ErrorKind::Unsupported.into()))
    }
}
impl VirtualFile for OutputSink {
    fn last_accessed(&self) -> u64 {
        0
    }
    fn last_modified(&self) -> u64 {
        0
    }
    fn created_time(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _: u64) -> virtual_fs::Result<()> {
        Err(FsError::Unsupported)
    }
    fn unlink(&mut self) -> virtual_fs::Result<()> {
        Err(FsError::Unsupported)
    }
    fn get_special_fd(&self) -> Option<u32> {
        Some(match self.stream {
            LogStream::Stdout => 1,
            LogStream::Stderr => 2,
        })
    }
    fn poll_read_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::ErrorKind::PermissionDenied.into()))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(CHUNK_BYTES))
    }
}
