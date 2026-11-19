//! Bounded stdin and WASIX terminal state. No host terminal, PTY or shell is
//! inherited. Input is scoped to a worker generation by the Android controller.
use crate::service_stdio::ServiceOutput;
use std::{
    collections::VecDeque,
    fmt, io,
    pin::Pin,
    sync::{Arc, Mutex, mpsc as replies},
    task::{Context, Poll, Waker},
};
use tokio::{
    io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf},
    sync::mpsc,
};
use wasmer_wasix::{
    WasiTtyState,
    os::{
        task::{
            process::WasiProcess,
            signal::{SignalDeliveryError, SignalHandlerAbi},
        },
        tty::{InputEvent, Tty, TtyBridge, TtyOptions},
    },
    virtual_fs::{self, FsError, VirtualFile},
};
const QUEUE_BYTES: usize = 64 * 1024;
const LINE_BYTES: usize = 32 * 1024;
pub const INPUT_BYTES: usize = 4096;

#[derive(Default)]
struct Queue {
    bytes: VecDeque<u8>,
    closed: bool,
    readers: Vec<Waker>,
    writers: Vec<Waker>,
}
#[derive(Clone)]
pub(crate) struct InputQueue(Arc<Mutex<Queue>>);
impl fmt::Debug for InputQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputQueue").finish_non_exhaustive()
    }
}
impl InputQueue {
    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, Queue>> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("stdin queue lock failed"))
    }
    pub fn close(&self) {
        if let Ok(mut queue) = self.0.lock() {
            queue.closed = true;
            let wake = std::mem::take(&mut queue.readers)
                .into_iter()
                .chain(std::mem::take(&mut queue.writers))
                .collect::<Vec<_>>();
            drop(queue);
            for waker in wake {
                waker.wake();
            }
        }
    }
    pub(crate) fn try_reply(&self, bytes: &[u8]) -> bool {
        let Ok(mut queue) = self.0.lock() else {
            return false;
        };
        if queue.closed || queue.bytes.len() + bytes.len() > QUEUE_BYTES {
            return false;
        }
        queue.bytes.extend(bytes);
        let wake = std::mem::take(&mut queue.readers);
        drop(queue);
        for waker in wake {
            waker.wake();
        }
        true
    }
}
fn register(wakers: &mut Vec<Waker>, waker: &Waker) -> io::Result<()> {
    if wakers.iter().any(|old| old.will_wake(waker)) {
        return Ok(());
    }
    if wakers.len() >= 64 {
        return Err(io::Error::other("too many pending stdin operations"));
    }
    wakers.push(waker.clone());
    Ok(())
}
#[derive(Clone, Debug)]
pub(crate) struct InputFile(InputQueue);
impl InputFile {
    pub(crate) fn queue(&self) -> InputQueue {
        self.0.clone()
    }
}
impl AsyncRead for InputFile {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let mut queue = self.0.lock()?;
        if queue.bytes.is_empty() {
            if queue.closed {
                return Poll::Ready(Ok(()));
            }
            register(&mut queue.readers, cx.waker())?;
            return Poll::Pending;
        }
        let count = buf.remaining().min(queue.bytes.len());
        for _ in 0..count {
            buf.put_slice(&[queue.bytes.pop_front().unwrap()]);
        }
        let wake = std::mem::take(&mut queue.writers);
        drop(queue);
        for waker in wake {
            waker.wake();
        }
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for InputFile {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if bytes.len() > QUEUE_BYTES {
            return Poll::Ready(Err(io::ErrorKind::InvalidInput.into()));
        }
        let mut queue = self.0.lock()?;
        if queue.closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        // Tty writes once per canonical line, so accept all or await capacity;
        // a short successful write would silently lose upstream's remaining bytes.
        if queue.bytes.len() + bytes.len() > QUEUE_BYTES {
            register(&mut queue.writers, cx.waker())?;
            return Poll::Pending;
        }
        queue.bytes.extend(bytes);
        let wake = std::mem::take(&mut queue.readers);
        drop(queue);
        for waker in wake {
            waker.wake();
        }
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.close();
        Poll::Ready(Ok(()))
    }
}
impl AsyncSeek for InputFile {
    fn start_seek(self: Pin<&mut Self>, _: io::SeekFrom) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn poll_complete(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::ErrorKind::Unsupported.into()))
    }
}
impl VirtualFile for InputFile {
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
        Some(0)
    }
    fn poll_read_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut queue = self.0.lock()?;
        if !queue.bytes.is_empty() || queue.closed {
            return Poll::Ready(Ok(queue.bytes.len()));
        }
        register(&mut queue.readers, cx.waker())?;
        Poll::Pending
    }
    fn poll_write_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let mut queue = self.0.lock()?;
        if queue.closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if queue.bytes.len() < QUEUE_BYTES {
            return Poll::Ready(Ok(QUEUE_BYTES - queue.bytes.len()));
        }
        register(&mut queue.writers, cx.waker())?;
        Poll::Pending
    }
}

pub(crate) struct TerminalBridge {
    pub options: TtyOptions,
    state: Mutex<WasiTtyState>,
    tty: bool,
}
impl fmt::Debug for TerminalBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalBridge").finish_non_exhaustive()
    }
}
impl TerminalBridge {
    fn new(tty: bool) -> Self {
        let options = TtyOptions::default();
        options.set_echo(tty);
        options.set_line_buffering(tty);
        Self {
            options,
            state: Mutex::new(WasiTtyState {
                stdin_tty: tty,
                stdout_tty: tty,
                stderr_tty: tty,
                echo: tty,
                line_buffered: tty,
                ..Default::default()
            }),
            tty,
        }
    }
    pub fn resize(&self, cols: u32, rows: u32, width: u32, height: u32) -> io::Result<()> {
        if !self.tty {
            return Err(io::ErrorKind::Unsupported.into());
        }
        if !(1..=u32::from(wasmdroid_engine::terminal::MAX_COLS)).contains(&cols)
            || !(1..=u32::from(wasmdroid_engine::terminal::MAX_ROWS)).contains(&rows)
            || width > 16384
            || height > 16384
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.cols = cols;
        state.rows = rows;
        state.width = width;
        state.height = height;
        self.options.set_cols(cols);
        self.options.set_rows(rows);
        Ok(())
    }
}
impl TtyBridge for TerminalBridge {
    fn reset(&self) {
        self.options.set_echo(self.tty);
        self.options.set_line_buffering(self.tty);
        self.options.set_line_feeds(true);
    }
    fn tty_get(&self) -> WasiTtyState {
        let guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut state = guard.clone();
        state.cols = self.options.cols();
        state.rows = self.options.rows();
        state.echo = self.options.echo();
        state.line_buffered = self.options.line_buffering();
        state.line_feeds = self.options.line_feeds();
        state
    }
    fn tty_set(&self, mut state: WasiTtyState) {
        let mut previous = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.cols = state
            .cols
            .clamp(1, u32::from(wasmdroid_engine::terminal::MAX_COLS));
        state.rows = state
            .rows
            .clamp(1, u32::from(wasmdroid_engine::terminal::MAX_ROWS));
        state.width = state.width.min(16384);
        state.height = state.height.min(16384);
        state.stdin_tty &= self.tty;
        state.stdout_tty &= self.tty;
        state.stderr_tty &= self.tty;
        self.options.set_cols(state.cols);
        self.options.set_rows(state.rows);
        self.options.set_echo(state.echo);
        self.options.set_line_buffering(state.line_buffered);
        self.options.set_line_feeds(state.line_feeds);
        *previous = state;
    }
}
struct GuestSignaler(WasiProcess);
impl fmt::Debug for GuestSignaler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GuestSignaler").finish_non_exhaustive()
    }
}
impl SignalHandlerAbi for GuestSignaler {
    fn signal(&self, signal: u8) -> Result<(), SignalDeliveryError> {
        let signal =
            wasmer_wasix_types::wasi::Signal::try_from(signal).map_err(|_| SignalDeliveryError)?;
        self.0.signal_process(signal);
        Ok(())
    }
}

enum Action {
    Data(Vec<u8>),
    Eof,
}
struct Request {
    action: Action,
    reply: replies::SyncSender<io::Result<()>>,
}
#[derive(Clone)]
pub struct InputControl {
    sender: mpsc::Sender<Request>,
    queue: InputQueue,
    bridge: Arc<TerminalBridge>,
}
impl fmt::Debug for InputControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputControl").finish_non_exhaustive()
    }
}
impl InputControl {
    fn request(&self, action: Action) -> io::Result<()> {
        let (sender, receiver) = replies::sync_channel(1);
        self.sender
            .try_send(Request {
                action,
                reply: sender,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Closed(_) => {
                    io::Error::new(io::ErrorKind::BrokenPipe, "stdin controller ended")
                }
                mpsc::error::TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "stdin control queue is full")
                }
            })?;
        receiver
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "stdin controller ended"))?
    }
    pub fn write(&self, bytes: Vec<u8>) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > INPUT_BYTES {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.request(Action::Data(bytes))
    }
    pub fn eof(&self) -> io::Result<()> {
        self.request(Action::Eof)
    }
    pub fn close(&self) {
        self.queue.close();
    }
    pub fn resize(&self, cols: u32, rows: u32, width: u32, height: u32) -> io::Result<()> {
        if !self.bridge.tty {
            return Err(io::ErrorKind::Unsupported.into());
        }
        self.bridge.resize(cols, rows, width, height)
    }
}

/// Start the input infrastructure only after the guest process exists. The
/// async line discipline can wait for a reader without blocking control IPC.
pub(crate) fn prepare(tty: bool) -> (InputFile, Arc<TerminalBridge>) {
    (
        InputFile(InputQueue(Arc::new(Mutex::new(Queue::default())))),
        Arc::new(TerminalBridge::new(tty)),
    )
}
pub(crate) fn spawn(
    executor: &tokio::runtime::Runtime,
    file: InputFile,
    bridge: Arc<TerminalBridge>,
    output: ServiceOutput,
    process: WasiProcess,
) -> InputControl {
    let (sender, mut receiver) = mpsc::channel::<Request>(16);
    let control = InputControl {
        sender,
        queue: file.0.clone(),
        bridge: bridge.clone(),
    };
    let mut terminal = Tty::new(
        Box::new(file.clone()),
        Box::new(output.echo_sink()),
        false,
        bridge.options.clone(),
    );
    terminal.set_signaler(Box::new(GuestSignaler(process)));
    executor.spawn(async move {
        let mut canonical_bound = 0usize;
        while let Some(request) = receiver.recv().await {
            let mut result = Ok(());
            if file.0.lock().map(|q| q.closed).unwrap_or(true) {
                result = Err(io::ErrorKind::BrokenPipe.into());
            } else {
                match request.action {
                    Action::Data(bytes) => {
                        // Conservatively bound upstream's canonical line/parser
                        // state before feeding any part of this input request.
                        let mut next = canonical_bound;
                        let mut max = next;
                        if bridge.options.line_buffering() {
                            for byte in &bytes {
                                if matches!(*byte, b'\n' | b'\r' | 3 | 4 | 21) {
                                    next = 0;
                                } else {
                                    next = next.saturating_add(1);
                                }
                                max = max.max(next);
                            }
                        }
                        if max > LINE_BYTES {
                            result = Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "terminal input line exceeds limit",
                            ));
                        } else {
                            terminal = terminal.on_event(InputEvent::Raw(bytes)).await;
                            canonical_bound = next;
                            result = output.status();
                            if file.0.lock().map(|q| q.closed).unwrap_or(true) {
                                result = Err(io::ErrorKind::BrokenPipe.into());
                            }
                        }
                    }
                    Action::Eof => {
                        if bridge.options.line_buffering() {
                            terminal = terminal.on_event(InputEvent::Raw(vec![4])).await;
                            result = output.status();
                        }
                        file.0.close();
                    }
                }
            }
            let _ = request.reply.send(result);
        }
        file.0.close();
    });
    control
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn bounded_stdin_waits_instead_of_silently_short_writing_a_line() {
        let (mut reader, _) = prepare(false);
        let mut writer = reader.clone();
        writer.write_all(&vec![b'a'; QUEUE_BYTES]).await.unwrap();
        let second = vec![b'b'; LINE_BYTES];
        let mut pending = Box::pin(writer.write_all(&second));
        assert!(
            poll_fn(|cx| Poll::Ready(matches!(pending.as_mut().poll(cx), Poll::Pending))).await
        );
        let mut first = vec![0; LINE_BYTES];
        reader.read_exact(&mut first).await.unwrap();
        assert!(first.iter().all(|b| *b == b'a'));
        pending.await.unwrap();
        reader.0.close();
        let mut remaining = Vec::new();
        reader.read_to_end(&mut remaining).await.unwrap();
        assert_eq!(remaining.len(), QUEUE_BYTES);
        assert!(remaining[..LINE_BYTES].iter().all(|b| *b == b'a'));
        assert!(remaining[LINE_BYTES..].iter().all(|b| *b == b'b'));
    }
    #[tokio::test]
    async fn closing_stdin_wakes_a_reader_and_preserves_already_queued_bytes() {
        let (mut reader, _) = prepare(false);
        let mut writer = reader.clone();
        let mut waiting = Box::pin(reader.read_u8());
        assert!(
            poll_fn(|cx| Poll::Ready(matches!(waiting.as_mut().poll(cx), Poll::Pending))).await
        );
        drop(waiting);
        writer.write_all(b"queued").await.unwrap();
        writer.0.close();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"queued");
        assert_eq!(
            writer.write_all(b"late").await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
