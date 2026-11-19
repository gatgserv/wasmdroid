//! Generation-aware raw stdout/stderr with bounded private disk rotation.
//! Logs are user data, never event/analytics fields. No guest-selected paths.
use crate::{
    EngineError, Result,
    api::validate_name,
    session::{EngineOwnership, EngineSession},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const CHUNK_BYTES: usize = 4096;
const HEADER_BYTES: usize = 24;
const SEGMENT_BYTES: u64 = 256 * 1024;
const SEGMENTS: usize = 4;
const PAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    Stdout,
    Stderr,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub stream: LogStream,
    /// Base64 preserves arbitrary guest bytes and split UTF-8/terminal sequences.
    pub data_base64: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogQuery {
    pub generation: Option<String>,
    pub after: u64,
    pub since_ms: Option<u64>,
    pub tail: Option<u32>,
    pub limit: u32,
}
impl Default for LogQuery {
    fn default() -> Self {
        Self {
            generation: None,
            after: 0,
            since_ms: None,
            tail: None,
            limit: 256,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct LogPage {
    pub generation: String,
    pub records: Vec<LogRecord>,
    pub next_sequence: u64,
    pub first_sequence: Option<u64>,
    pub gap: bool,
    pub incomplete_tail: bool,
    pub has_more: bool,
}
#[derive(Debug, Serialize, Deserialize)]
struct Run {
    generation: String,
    ordinal: u64,
    started_at_ms: u64,
}

fn now_ms() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| EngineError::Invalid("clock precedes epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| EngineError::Invalid("clock exceeds log timestamp range".into()))
}
fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    if !std::fs::symlink_metadata(path)?.is_dir() {
        return Err(EngineError::Invalid(
            "log storage is not a private directory".into(),
        ));
    }
    Ok(())
}
fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    options
}
fn regular(path: &Path) -> Result<File> {
    let file = options().read(true).open(path)?;
    if !file.metadata()?.is_file() {
        return Err(EngineError::Invalid(
            "log entry is not a regular file".into(),
        ));
    }
    Ok(file)
}
fn lock(path: &Path, shared: bool, blocking: bool) -> Result<File> {
    let file = options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(EngineError::Invalid(
            "log lock is not a regular file".into(),
        ));
    }
    if blocking {
        if shared {
            fs2::FileExt::lock_shared(&file)?;
        } else {
            fs2::FileExt::lock_exclusive(&file)?;
        }
    } else {
        crate::locking::try_lock(&file, shared, "log run still has a writer")?;
    }
    Ok(file)
}
fn metadata(path: &Path) -> Result<Run> {
    let mut bytes = Vec::new();
    regular(path)?.take(1025).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 {
        return Err(EngineError::Invalid("log metadata exceeds limit".into()));
    }
    let run: Run = serde_json::from_slice(&bytes)?;
    validate_name(&run.generation)?;
    Ok(run)
}
fn publish(path: &Path, run: &Run) -> Result<()> {
    let mut pending = tempfile::Builder::new()
        .prefix("wasmdroid-log-")
        .tempfile_in(path.parent().unwrap())?;
    pending.write_all(&serde_json::to_vec(run)?)?;
    pending.as_file().sync_all()?;
    pending.persist(path).map_err(|e| e.error)?;
    Ok(())
}
fn base(session: &EngineSession, id: &str) -> Result<PathBuf> {
    validate_name(id)?;
    let logs = session.root().join("logs");
    private_directory(&logs)?;
    let instance = logs.join(id);
    private_directory(&instance)?;
    let base = instance.join("live");
    private_directory(&base)?;
    Ok(base)
}

/// Coordinator-only publication before binding the worker. Old writers cannot
/// publish over a new generation or make a stale result look current.
pub fn begin_run(session: &EngineSession, id: &str, generation: &str) -> Result<()> {
    validate_name(generation)?;
    let base = base(session, id)?;
    let _control = lock(&base.join("control.lock"), false, true)?;
    let previous = match metadata(&base.join("current.json")) {
        Ok(run) => Some(run),
        Err(EngineError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let _previous_owner = previous
        .as_ref()
        .map(|run| lock(&base.join(&run.generation).join("owner.lock"), false, false))
        .transpose()?;
    let run = Run {
        generation: generation.into(),
        ordinal: previous
            .map(|p| {
                p.ordinal
                    .checked_add(1)
                    .ok_or_else(|| EngineError::Invalid("log run revision overflow".into()))
            })
            .transpose()?
            .unwrap_or(1),
        started_at_ms: now_ms()?,
    };
    let directory = base.join(generation);
    std::fs::create_dir(&directory)?;
    publish(&directory.join("run.json"), &run)?;
    publish(&base.join("current.json"), &run)?;
    // Pruning is maintenance. A partial cleanup must not turn a published run
    // into a false failed start; unrecognized content is always retained.
    let _ = prune(&base, &run);
    Ok(())
}
fn segment_files(directory: &Path) -> Result<Vec<(u64, PathBuf)>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(number) = name.strip_suffix(".segment") else {
            continue;
        };
        if number.len() != 20 || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let first = number
            .parse::<u64>()
            .map_err(|_| EngineError::Invalid("invalid log segment sequence".into()))?;
        if first == 0 {
            return Err(EngineError::Invalid("invalid log segment sequence".into()));
        }
        files.push((first, entry.path()));
        if files.len() > 1024 {
            return Err(EngineError::Invalid("too many log segments".into()));
        }
    }
    files.sort_by_key(|f| f.0);
    Ok(files)
}
fn prune(base: &Path, current: &Run) -> Result<()> {
    let mut runs = Vec::new();
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if validate_name(name).is_err() || name == current.generation {
            continue;
        }
        let Ok(run) = metadata(&entry.path().join("run.json")) else {
            continue;
        };
        if run.generation == name {
            runs.push((run.ordinal, entry.path()));
        }
    }
    runs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    for (_, directory) in runs.into_iter().skip(2) {
        let Ok(owner) = lock(&directory.join("owner.lock"), false, false) else {
            continue;
        };
        let io = lock(&directory.join("io.lock"), false, true)?;
        for (_, file) in segment_files(&directory)? {
            std::fs::remove_file(file)?;
        }
        for row in (0..crate::terminal::MAX_ROWS).step_by(16) {
            let path = directory.join(format!("terminal-{row:03}.json"));
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                std::fs::remove_file(path)?;
            }
        }
        for name in ["run.json", "opened", "terminal-index.json"] {
            let path = directory.join(name);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                std::fs::remove_file(path)?;
            }
        }
        drop(io);
        drop(owner);
        for name in ["owner.lock", "io.lock"] {
            let _ = std::fs::remove_file(directory.join(name));
        }
        // Never recursively delete unknown files or subdirectories.
        let _ = std::fs::remove_dir(directory);
    }
    Ok(())
}

pub struct LogWriter {
    directory: PathBuf,
    _owner: File,
    _engine: EngineOwnership,
    current: Option<File>,
    size: u64,
    next: u64,
    failed: bool,
}
impl std::fmt::Debug for LogWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogWriter").finish_non_exhaustive()
    }
}
impl LogWriter {
    /// Completed display pages are optional cached user output, never code.
    /// The index is published last so interrupted persistence is not complete.
    pub fn save_terminal(&mut self, pages: &[crate::terminal::TerminalPage]) -> Result<()> {
        let first = pages
            .first()
            .ok_or_else(|| EngineError::Invalid("empty terminal snapshot".into()))?;
        let _io = lock(&self.directory.join("io.lock"), false, true)?;
        let mut next = 0u16;
        for page in pages {
            page.validate()?;
            if page.start_row != next
                || page.rows != first.rows
                || page.cols != first.cols
                || page.token != first.token
                || page.version != first.version
            {
                return Err(EngineError::Invalid("terminal pages differ".into()));
            }
            next += page.cells.len() as u16;
        }
        if next != first.rows {
            return Err(EngineError::Invalid("incomplete terminal snapshot".into()));
        }
        for page in pages {
            let bytes = serde_json::to_vec(page)?;
            if bytes.len() > crate::terminal::FRAME_JSON_LIMIT {
                return Err(EngineError::Invalid("terminal page exceeds limit".into()));
            }
            publish_bytes(
                &self
                    .directory
                    .join(format!("terminal-{:03}.json", page.start_row)),
                &bytes,
            )?;
        }
        let index = TerminalIndex {
            token: first.token,
            version: first.version,
            rows: first.rows,
            cols: first.cols,
        };
        publish_bytes(
            &self.directory.join("terminal-index.json"),
            &serde_json::to_vec(&index)?,
        )?;
        Ok(())
    }
    pub fn open(session: &EngineSession, id: &str, generation: &str) -> Result<Self> {
        validate_name(generation)?;
        let base = base(session, id)?;
        let _control = lock(&base.join("control.lock"), true, true)?;
        if metadata(&base.join("current.json"))?.generation != generation {
            return Err(EngineError::Conflict("stale log generation".into()));
        }
        let directory = base.join(generation);
        let owner = lock(&directory.join("owner.lock"), false, false)?;
        if !segment_files(&directory)?.is_empty() {
            return Err(EngineError::Conflict(
                "log generation already contains output".into(),
            ));
        }
        options()
            .write(true)
            .create_new(true)
            .open(directory.join("opened"))?;
        Ok(Self {
            directory,
            _owner: owner,
            _engine: session.ownership(),
            current: None,
            size: 0,
            next: 1,
            failed: false,
        })
    }
    pub fn append(&mut self, stream: LogStream, bytes: &[u8]) -> Result<usize> {
        if self.failed {
            return Err(EngineError::Invalid(
                "log writer failed; restart the service".into(),
            ));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = bytes.len().min(CHUNK_BYTES);
        let _io = lock(&self.directory.join("io.lock"), false, true)?;
        let result = (|| -> Result<usize> {
            if self.current.is_none()
                || self.size + HEADER_BYTES as u64 + count as u64 > SEGMENT_BYTES
            {
                self.current.take();
                let files = segment_files(&self.directory)?;
                let remove = files.len().saturating_add(1).saturating_sub(SEGMENTS);
                for (_, file) in files.into_iter().take(remove) {
                    std::fs::remove_file(file)?;
                }
                self.current = Some(
                    options()
                        .write(true)
                        .create_new(true)
                        .open(self.directory.join(format!("{:020}.segment", self.next)))?,
                );
                self.size = 0;
            }
            let following = self
                .next
                .checked_add(1)
                .ok_or_else(|| EngineError::Invalid("log sequence overflow".into()))?;
            let mut header = [0u8; HEADER_BYTES];
            header[..8].copy_from_slice(&self.next.to_le_bytes());
            header[8..16].copy_from_slice(&now_ms()?.to_le_bytes());
            header[16..20].copy_from_slice(&(count as u32).to_le_bytes());
            header[20] = match stream {
                LogStream::Stdout => 1,
                LogStream::Stderr => 2,
            };
            let file = self.current.as_mut().unwrap();
            file.write_all(&header)?;
            file.write_all(&bytes[..count])?;
            self.size += HEADER_BYTES as u64 + count as u64;
            self.next = following;
            Ok(count)
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    pub fn flush(&mut self) -> Result<()> {
        if let Some(file) = &mut self.current {
            file.flush()?;
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct TerminalIndex {
    token: u64,
    version: u64,
    rows: u16,
    cols: u16,
}
fn publish_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = tempfile::Builder::new()
        .prefix("wasmdroid-terminal-")
        .tempfile_in(path.parent().unwrap())?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn read_terminal(
    session: &EngineSession,
    id: &str,
    generation: &str,
    start_row: u16,
) -> Result<Option<String>> {
    validate_name(generation)?;
    if start_row >= crate::terminal::MAX_ROWS || !start_row.is_multiple_of(16) {
        return Err(EngineError::Invalid("invalid terminal page".into()));
    }
    let base = base(session, id)?;
    let _control = lock(&base.join("control.lock"), true, true)?;
    let directory = base.join(generation);
    if !directory.is_dir() {
        return Ok(None);
    }
    let _io = lock(&directory.join("io.lock"), true, true)?;
    let mut index = Vec::new();
    match regular(&directory.join("terminal-index.json")) {
        Ok(file) => {
            file.take(1025).read_to_end(&mut index)?;
        }
        Err(EngineError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }
    if index.len() > 1024 {
        return Err(EngineError::Invalid("terminal index exceeds limit".into()));
    }
    let index: TerminalIndex = serde_json::from_slice(&index)?;
    let mut bytes = Vec::new();
    regular(&directory.join(format!("terminal-{start_row:03}.json")))?
        .take(crate::terminal::FRAME_JSON_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > crate::terminal::FRAME_JSON_LIMIT {
        return Err(EngineError::Invalid("terminal page exceeds limit".into()));
    }
    let page: crate::terminal::TerminalPage = serde_json::from_slice(&bytes)?;
    page.validate()?;
    if page.start_row != start_row
        || page.token != index.token
        || page.version != index.version
        || page.rows != index.rows
        || page.cols != index.cols
    {
        return Err(EngineError::Invalid(
            "terminal snapshot index mismatch".into(),
        ));
    }
    Ok(Some(serde_json::to_string(&page)?))
}

/// Follow is pagination on a generation/sequence cursor. Readers never infer
/// process completion from EOF; lifecycle status belongs to the supervisor.
pub fn read(session: &EngineSession, id: &str, query: &LogQuery) -> Result<Option<LogPage>> {
    if !(1..=256).contains(&query.limit) || query.tail.is_some_and(|n| n == 0 || n > 256) {
        return Err(EngineError::Invalid(
            "log page limits must be 1..256".into(),
        ));
    }
    let base = base(session, id)?;
    let _control = lock(&base.join("control.lock"), true, true)?;
    let generation = if let Some(generation) = &query.generation {
        validate_name(generation)?;
        generation.clone()
    } else {
        match metadata(&base.join("current.json")) {
            Ok(run) => run.generation,
            Err(EngineError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
    };
    let directory = base.join(&generation);
    let run = metadata(&directory.join("run.json"))?;
    if run.generation != generation {
        return Err(EngineError::Invalid("log run identity mismatch".into()));
    }
    let _io = lock(&directory.join("io.lock"), true, true)?;
    let mut records = VecDeque::new();
    let mut bytes_kept = 0;
    let mut next = query.after;
    let mut first = None;
    let mut previous = None;
    let mut incomplete = false;
    let mut has_more = false;
    'segments: for (_, path) in segment_files(&directory)? {
        let mut file = regular(&path)?;
        if file.metadata()?.len() > SEGMENT_BYTES {
            return Err(EngineError::Invalid("log segment exceeds limit".into()));
        }
        loop {
            let mut header = [0u8; HEADER_BYTES];
            let count = read_frame(&mut file, &mut header)?;
            if count == 0 {
                break;
            }
            if count != HEADER_BYTES {
                incomplete = true;
                break;
            }
            let sequence = u64::from_le_bytes(header[..8].try_into().unwrap());
            let timestamp_ms = u64::from_le_bytes(header[8..16].try_into().unwrap());
            let length = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
            let stream = match header[20] {
                1 => LogStream::Stdout,
                2 => LogStream::Stderr,
                _ => return Err(EngineError::Invalid("unknown log stream".into())),
            };
            if sequence == 0
                || previous.is_some_and(|p: u64| p.checked_add(1) != Some(sequence))
                || !(1..=CHUNK_BYTES).contains(&length)
                || header[21..].iter().any(|b| *b != 0)
            {
                return Err(EngineError::Invalid("invalid log frame".into()));
            }
            let mut data = vec![0u8; length];
            if read_frame(&mut file, &mut data)? != length {
                incomplete = true;
                break;
            }
            first.get_or_insert(sequence);
            previous = Some(sequence);
            if sequence <= query.after {
                continue;
            }
            if query.since_ms.is_some_and(|since| timestamp_ms < since) {
                next = sequence;
                continue;
            }
            if query.tail.is_none()
                && (records.len() >= query.limit as usize || bytes_kept + length > PAGE_BYTES)
            {
                has_more = true;
                break 'segments;
            }
            next = sequence;
            bytes_kept += length;
            records.push_back((
                LogRecord {
                    sequence,
                    timestamp_ms,
                    stream,
                    data_base64: STANDARD.encode(&data),
                },
                length,
            ));
            if let Some(tail) = query.tail {
                while records.len() > tail.min(query.limit) as usize || bytes_kept > PAGE_BYTES {
                    bytes_kept -= records.pop_front().unwrap().1;
                }
            }
        }
    }
    Ok(Some(LogPage {
        generation,
        records: records.into_iter().map(|r| r.0).collect(),
        next_sequence: next,
        first_sequence: first,
        gap: first.is_some_and(|first| query.after.saturating_add(1) < first),
        incomplete_tail: incomplete,
        has_more,
    }))
}
fn read_frame(file: &mut File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut offset = 0;
    while offset < buffer.len() {
        match file.read(&mut buffer[offset..]) {
            Ok(0) => break,
            Ok(count) => offset += count,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(offset)
}
