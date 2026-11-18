//! Data-only, standard GNU tar.gz backups. Restore never overwrites a volume.
//! Parse bounded raw headers ourselves so untrusted PAX/sparse metadata cannot
//! make a tar decoder allocate unbounded buffers before capability checks.
use crate::{
    EngineError, Result,
    data::{self, DataReader},
    session::EngineSession,
    volumes::{VolumeDirectory, VolumeManager, VolumeOpenOptions},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
const MANIFEST: &str = "wasmdroid-backup.json";
const MAX_ENTRIES: usize = 100_000;
const MAX_NAME: usize = 16 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub format: String,
    pub version: u32,
    pub source_volume: String,
    pub created_at_ms: u64,
}
#[derive(Serialize)]
pub struct RestoreResult {
    pub volume: String,
    pub files: u64,
    pub bytes: u64,
    pub source_volume: String,
}
fn cache_file(session: &EngineSession) -> Result<tempfile::NamedTempFile> {
    let cache = session.root().join("transfers/cache");
    std::fs::create_dir_all(&cache)?;
    Ok(tempfile::Builder::new()
        .prefix("wasmdroid-cache-")
        .tempfile_in(cache)?)
}
fn header(size: u64, directory: bool) -> tar::Header {
    let mut value = tar::Header::new_gnu();
    value.set_size(size);
    value.set_uid(0);
    value.set_gid(0);
    value.set_mtime(0);
    value.set_mode(if directory { 0o700 } else { 0o600 });
    value.set_entry_type(if directory {
        tar::EntryType::Directory
    } else {
        tar::EntryType::Regular
    });
    value.set_cksum();
    value
}
pub fn snapshot(
    session: &EngineSession,
    volume: &str,
    max_bytes: u64,
    cancel: Arc<AtomicBool>,
) -> Result<DataReader> {
    let directory = VolumeManager::open(session)?.exclusive(volume)?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(4);
    let producer = sender.clone();
    let source = volume.to_owned();
    let cancellation = cancel.clone();
    std::thread::Builder::new()
        .name("wasmdroid-backup".into())
        .spawn(move || {
            let result = write_snapshot(
                directory,
                &source,
                max_bytes,
                cancellation,
                PipeWriter(producer),
            );
            let frame = match result {
                Ok(()) => BackupFrame::Finished,
                Err(error) => BackupFrame::Failed(error.to_string()),
            };
            let _ = sender.send(frame);
        })?;
    Ok(DataReader::stream(
        session,
        BackupPipe {
            receiver,
            pending: std::io::Cursor::new(Vec::new()),
            finished: false,
            failure: None,
            cancel,
        },
    ))
}
fn write_snapshot(
    directory: VolumeDirectory,
    volume: &str,
    max_bytes: u64,
    cancel: Arc<AtomicBool>,
    sink: impl Write,
) -> Result<()> {
    let compressor = flate2::write::GzEncoder::new(sink, flate2::Compression::fast());
    let mut archive = tar::Builder::new(compressor);
    let manifest = BackupManifest {
        format: "wasmdroid.volume".into(),
        version: 1,
        source_volume: volume.into(),
        created_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    };
    let json = serde_json::to_vec(&manifest)?;
    archive.append_data(&mut header(json.len() as u64, false), MANIFEST, &json[..])?;
    let mut pending = vec![PathBuf::new()];
    let mut total = 0u64;
    let mut count = 0usize;
    while let Some(parent) = pending.pop() {
        let entries = directory.entries(if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &parent
        })?;
        for entry in entries {
            let entry = entry?;
            check_cancel(&cancel)?;
            count += 1;
            if count > MAX_ENTRIES {
                return Err(EngineError::Invalid("backup entry limit exceeded".into()));
            }
            let name = entry
                .name
                .to_str()
                .ok_or_else(|| EngineError::Unsupported("non-UTF-8 backup filename".into()))?;
            let relative = data::join_relative(&parent, Path::new(name))?;
            let metadata = directory.symlink_metadata(&relative)?;
            if metadata.is_symlink || (!metadata.is_dir && !metadata.is_file) {
                return Err(EngineError::Unsupported(
                    "backup contains links or special files".into(),
                ));
            }
            let path = data::join_relative(Path::new("data"), &relative)?;
            if path.as_os_str().to_string_lossy().len() > MAX_NAME {
                return Err(EngineError::Invalid("backup path exceeds limit".into()));
            }
            if metadata.is_dir {
                archive.append_data(&mut header(0, true), path, std::io::empty())?;
                pending.push(relative);
            } else {
                total = total
                    .checked_add(metadata.len)
                    .filter(|n| *n <= max_bytes)
                    .ok_or_else(|| {
                        EngineError::Invalid("backup exceeds available space budget".into())
                    })?;
                let file = directory.open_file(
                    relative,
                    VolumeOpenOptions {
                        read: true,
                        ..Default::default()
                    },
                )?;
                archive.append_data(
                    &mut header(metadata.len, false),
                    path,
                    CancelReader {
                        inner: file,
                        cancel: cancel.clone(),
                    },
                )?;
            }
        }
    }
    let compressor = archive.into_inner()?;
    compressor.finish()?;
    Ok(())
}

enum BackupFrame {
    Bytes(Vec<u8>),
    Finished,
    Failed(String),
}
struct PipeWriter(std::sync::mpsc::SyncSender<BackupFrame>);
impl Write for PipeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for chunk in bytes.chunks(data::CHUNK_BYTES) {
            self.0
                .send(BackupFrame::Bytes(chunk.to_vec()))
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct BackupPipe {
    receiver: std::sync::mpsc::Receiver<BackupFrame>,
    pending: std::io::Cursor<Vec<u8>>,
    finished: bool,
    failure: Option<String>,
    cancel: Arc<AtomicBool>,
}
impl Read for BackupPipe {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        check_cancel(&self.cancel)?;
        if let Some(error) = &self.failure {
            return Err(std::io::Error::other(error.clone()));
        }
        loop {
            let count = self.pending.read(bytes)?;
            if count > 0 {
                return Ok(count);
            }
            if self.finished {
                return Ok(0);
            }
            match self.receiver.recv() {
                Ok(BackupFrame::Bytes(data)) => self.pending = std::io::Cursor::new(data),
                Ok(BackupFrame::Finished) => self.finished = true,
                Ok(BackupFrame::Failed(error)) => {
                    self.failure = Some(error.clone());
                    return Err(std::io::Error::other(error));
                }
                Err(_) => {
                    self.failure = Some("backup producer ended without completion".into());
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "backup producer ended without completion",
                    ));
                }
            }
        }
    }
}
impl Drop for BackupPipe {
    fn drop(&mut self) {
        if !self.finished {
            self.cancel.store(true, Ordering::Release);
        }
    }
}

pub struct RestoreInput {
    session: EngineSession,
    file: Option<tempfile::NamedTempFile>,
    new_volume: String,
    max_archive: u64,
    max_unpacked: u64,
    written: u64,
    finished: bool,
    cancel: Arc<AtomicBool>,
}
impl RestoreInput {
    pub fn limit_unpacked(&mut self, available_bytes: u64) {
        self.max_unpacked = self.max_unpacked.min(available_bytes);
    }
    pub fn begin(
        session: &EngineSession,
        new_volume: &str,
        max_archive: u64,
        max_unpacked: u64,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        crate::api::validate_name(new_volume)?;
        if max_archive == 0 || max_unpacked == 0 {
            return Err(EngineError::Invalid(
                "restore space budget must be positive".into(),
            ));
        }
        check_cancel(&cancel)?;
        Ok(Self {
            session: session.connection()?,
            file: Some(cache_file(session)?),
            new_volume: new_volume.into(),
            max_archive,
            max_unpacked,
            written: 0,
            finished: false,
            cancel,
        })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<u64> {
        check_cancel(&self.cancel)?;
        if self.finished || bytes.len() > data::CHUNK_BYTES {
            return Err(EngineError::Invalid(
                "invalid backup chunk or completed restore".into(),
            ));
        }
        let total = self
            .written
            .checked_add(bytes.len() as u64)
            .filter(|n| *n <= self.max_archive)
            .ok_or_else(|| EngineError::Invalid("compressed archive budget exceeded".into()))?;
        self.file
            .as_mut()
            .ok_or_else(|| EngineError::Conflict("restore is closed".into()))?
            .write_all(bytes)?;
        self.written = total;
        Ok(total)
    }
    pub fn commit(&mut self) -> Result<RestoreResult> {
        check_cancel(&self.cancel)?;
        if self.finished {
            return Err(EngineError::Conflict("restore already completed".into()));
        }
        let mut file = self
            .file
            .take()
            .ok_or_else(|| EngineError::Conflict("restore input is closed".into()))?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.as_file_mut().rewind()?;
        validate_gzip_header(file.as_file_mut())?;
        file.as_file_mut().rewind()?;
        let manager = VolumeManager::open(&self.session)?;
        // Never adopt an existing record, even if its name happened to collide.
        manager.create_new(&self.new_volume, BTreeMap::new())?;
        let restored = (|| -> Result<RestoreResult> {
            let directory = manager.exclusive(&self.new_volume)?;
            let decoder = flate2::bufread::GzDecoder::new(BufReader::new(file.as_file_mut()));
            let mut input = BudgetReader {
                inner: decoder,
                remaining: self.max_unpacked,
                cancel: self.cancel.clone(),
            };
            let mut manifest: Option<BackupManifest> = None;
            let mut long_name: Option<String> = None;
            let mut paths = BTreeSet::new();
            let mut files = 0u64;
            let mut bytes = 0u64;
            let mut count = 0usize;
            let mut logical = 0usize;
            loop {
                let mut raw = [0u8; 512];
                input.read_exact(&mut raw)?;
                if raw.iter().all(|b| *b == 0) {
                    let mut second = [0u8; 512];
                    input.read_exact(&mut second)?;
                    if second.iter().any(|b| *b != 0) {
                        return Err(EngineError::Invalid("invalid archive terminator".into()));
                    }
                    let mut tail = Vec::new();
                    (&mut input).take(64 * 1024 + 1).read_to_end(&mut tail)?;
                    if tail.len() > 64 * 1024 || tail.iter().any(|b| *b != 0) {
                        return Err(EngineError::Invalid(
                            "unexpected archive trailing data".into(),
                        ));
                    }
                    break;
                }
                count += 1;
                if count > MAX_ENTRIES * 2 + 2 {
                    return Err(EngineError::Invalid("restore entry limit exceeded".into()));
                }
                let header = tar::Header::from_byte_slice(&raw);
                let checksum: u32 = raw
                    .iter()
                    .enumerate()
                    .map(|(i, b)| {
                        if (148..156).contains(&i) {
                            32
                        } else {
                            *b as u32
                        }
                    })
                    .sum();
                if header.cksum()? != checksum {
                    return Err(EngineError::Invalid(
                        "archive header checksum mismatch".into(),
                    ));
                }
                let size = header.size()?;
                let kind = header.entry_type();
                if kind.is_gnu_longname() {
                    if long_name.is_some() || size == 0 || size > MAX_NAME as u64 {
                        return Err(EngineError::Invalid("invalid archive long name".into()));
                    }
                    let mut data = vec![0; size as usize];
                    input.read_exact(&mut data)?;
                    padding(&mut input, size)?;
                    if data.last() != Some(&0) {
                        return Err(EngineError::Invalid("unterminated archive name".into()));
                    }
                    data.pop();
                    let value = String::from_utf8(data)
                        .map_err(|_| EngineError::Unsupported("non-UTF-8 archive path".into()))?;
                    long_name = Some(value);
                    continue;
                }
                if !kind.is_file() && !kind.is_dir() {
                    return Err(EngineError::Unsupported(
                        "archive links, sparse entries and special files are not supported".into(),
                    ));
                }
                logical += 1;
                if logical > MAX_ENTRIES + 1 {
                    return Err(EngineError::Invalid("restore entry limit exceeded".into()));
                }
                if kind.is_dir() && size != 0 {
                    return Err(EngineError::Invalid("directory entry contains data".into()));
                }
                let name = match long_name.take() {
                    Some(value) => value,
                    None => String::from_utf8(header.path_bytes().into_owned())
                        .map_err(|_| EngineError::Unsupported("non-UTF-8 archive path".into()))?,
                };
                if manifest.is_none() {
                    if name != MANIFEST || !kind.is_file() || size > 64 * 1024 {
                        return Err(EngineError::Invalid("not a WasmDroid volume backup".into()));
                    }
                    let mut data = vec![0; size as usize];
                    input.read_exact(&mut data)?;
                    padding(&mut input, size)?;
                    let value: BackupManifest = serde_json::from_slice(&data)?;
                    if value.format != "wasmdroid.volume" || value.version != 1 {
                        return Err(EngineError::Unsupported("backup format/version".into()));
                    }
                    crate::api::validate_name(&value.source_volume)?;
                    manifest = Some(value);
                    continue;
                }
                let relative = archive_path(&name, kind.is_dir())?;
                let key = relative.to_string_lossy().to_string();
                let key = if cfg!(windows) {
                    key.to_lowercase()
                } else {
                    key
                };
                if !paths.insert(key) {
                    return Err(EngineError::Invalid("duplicate archive destination".into()));
                }
                if kind.is_dir() {
                    data::ensure_parent(
                        &directory,
                        &data::join_relative(&relative, Path::new("child"))?,
                    )?;
                } else {
                    data::ensure_parent(&directory, &relative)?;
                    let mut target = directory.open_file(
                        &relative,
                        VolumeOpenOptions {
                            write: true,
                            create_new: true,
                            ..Default::default()
                        },
                    )?;
                    let copied = std::io::copy(&mut (&mut input).take(size), &mut target)?;
                    if copied != size {
                        return Err(EngineError::Invalid("truncated archive entry".into()));
                    }
                    target.flush()?;
                    target.sync_all()?;
                    padding(&mut input, size)?;
                    files += 1;
                    bytes = bytes
                        .checked_add(size)
                        .ok_or_else(|| EngineError::Invalid("backup size overflow".into()))?;
                }
            }
            if long_name.is_some() {
                return Err(EngineError::Invalid("unused archive name extension".into()));
            }
            let manifest = manifest
                .ok_or_else(|| EngineError::Invalid("backup manifest is missing".into()))?;
            if !input.inner.into_inner().fill_buf()?.is_empty() {
                return Err(EngineError::Invalid(
                    "multiple gzip streams or trailing compressed data".into(),
                ));
            }
            Ok(RestoreResult {
                volume: self.new_volume.clone(),
                files,
                bytes,
                source_volume: manifest.source_volume,
            })
        })();
        match restored {
            Ok(result) => {
                self.finished = true;
                Ok(result)
            }
            Err(error) => {
                // Only the uniquely created, unreferenced target belongs to this
                // failed restore. The source/old volume is never modified.
                let _ = manager.remove(&self.new_volume);
                self.finished = true;
                Err(error)
            }
        }
    }
    pub fn abort(&mut self) {
        self.file.take();
        self.finished = true;
    }
}
fn archive_path(name: &str, directory: bool) -> Result<PathBuf> {
    let name = name
        .strip_prefix("data/")
        .ok_or_else(|| EngineError::Invalid("archive entry is outside data root".into()))?;
    let name = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    if name.is_empty()
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || name.len() > MAX_NAME
    {
        return Err(EngineError::Invalid("unsafe archive data path".into()));
    }
    data::file_path(name)
}
fn padding(input: &mut impl Read, size: u64) -> Result<()> {
    let mut buffer = [0u8; 512];
    let count = ((512 - size % 512) % 512) as usize;
    input.read_exact(&mut buffer[..count])?;
    Ok(())
}
/// Bound optional gzip header fields before the decoder allocates strings for
/// them. The archive payload remains streamed through a separate byte budget.
fn validate_gzip_header(input: &mut impl Read) -> Result<()> {
    let mut base = [0u8; 10];
    input.read_exact(&mut base)?;
    if base[..3] != [0x1f, 0x8b, 8] || base[3] & 0xe0 != 0 {
        return Err(EngineError::Invalid("invalid gzip backup header".into()));
    }
    if base[3] & 4 != 0 {
        let mut length = [0u8; 2];
        input.read_exact(&mut length)?;
        let mut remaining = u16::from_le_bytes(length) as usize;
        let mut buffer = [0u8; 1024];
        while remaining > 0 {
            let count = remaining.min(buffer.len());
            input.read_exact(&mut buffer[..count])?;
            remaining -= count;
        }
    }
    for flag in [8, 16] {
        if base[3] & flag != 0 {
            let mut count = 0usize;
            loop {
                let mut byte = [0u8; 1];
                input.read_exact(&mut byte)?;
                if byte[0] == 0 {
                    break;
                }
                count += 1;
                if count > MAX_NAME {
                    return Err(EngineError::Invalid(
                        "gzip header field exceeds limit".into(),
                    ));
                }
            }
        }
    }
    if base[3] & 2 != 0 {
        let mut checksum = [0u8; 2];
        input.read_exact(&mut checksum)?;
    }
    Ok(())
}
fn check_cancel(cancel: &AtomicBool) -> std::io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(std::io::Error::other("data transfer cancelled"))
    } else {
        Ok(())
    }
}
struct CancelReader<R> {
    inner: R,
    cancel: Arc<AtomicBool>,
}
impl<R: Read> Read for CancelReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        check_cancel(&self.cancel)?;
        self.inner.read(buffer)
    }
}
struct BudgetReader<R> {
    inner: R,
    remaining: u64,
    cancel: Arc<AtomicBool>,
}
impl<R: Read> Read for BudgetReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        check_cancel(&self.cancel)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0u8; 1];
            if self.inner.read(&mut probe)? == 0 {
                return Ok(0);
            }
            return Err(std::io::Error::other("decompressed backup budget exceeded"));
        }
        let count = buffer
            .len()
            .min(self.remaining.min(usize::MAX as u64) as usize);
        let read = self.inner.read(&mut buffer[..count])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}
