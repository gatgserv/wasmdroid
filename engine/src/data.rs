//! Streaming data operations through private volume capabilities. No ambient
//! path is accepted from Android document providers or archive contents.
use crate::{
    EngineError, Result,
    session::{EngineOwnership, EngineSession},
    volumes::{VolumeDirectory, VolumeManager, VolumeOpenOptions},
};
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub const CHUNK_BYTES: usize = 64 * 1024;
pub fn file_path(path: &str) -> Result<PathBuf> {
    let value = PathBuf::from(path);
    crate::volumes::relative(&value)?;
    if value.file_name().is_none()
        || path.is_empty()
        || path.len() > 16 * 1024
        || path.contains('\0')
    {
        return Err(EngineError::Invalid("invalid relative data path".into()));
    }
    Ok(value)
}
/// Construct internal volume-relative paths with the same portable separators
/// accepted at the boundary. PathBuf::push/join insert backslashes on Windows;
/// accepting those by weakening relative() would also admit raw wire input.
pub(crate) fn join_relative(parent: &Path, child: &Path) -> Result<PathBuf> {
    crate::volumes::relative(parent)?;
    crate::volumes::relative(child)?;
    let mut joined = parent.as_os_str().to_os_string();
    if !joined.is_empty() && !child.as_os_str().is_empty() {
        joined.push("/");
    }
    joined.push(child.as_os_str());
    let joined = PathBuf::from(joined);
    crate::volumes::relative(&joined)?;
    Ok(joined)
}
pub fn ensure_parent(directory: &VolumeDirectory, path: &Path) -> Result<()> {
    crate::volumes::relative(path)?;
    let mut current = PathBuf::new();
    if let Some(parent) = path.parent() {
        for component in parent.components() {
            current = join_relative(&current, Path::new(component.as_os_str()))?;
            match directory.create_dir(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !directory.metadata(&current)?.is_dir {
                        return Err(EngineError::Conflict(
                            "data path parent is not a directory".into(),
                        ));
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(())
}
#[derive(Serialize)]
pub struct DataEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    pub size_bytes: u64,
    pub modified_ns: u64,
}
pub fn list(directory: &VolumeDirectory, path: &str) -> Result<Vec<DataEntry>> {
    crate::volumes::relative(Path::new(path))?;
    let mut entries = Vec::new();
    for entry in directory.read_dir(if path.is_empty() { "." } else { path })? {
        let name = entry
            .name
            .into_string()
            .map_err(|_| EngineError::Unsupported("non-UTF-8 data filename".into()))?;
        let m = entry.metadata;
        entries.push(DataEntry {
            name,
            is_dir: m.is_dir,
            is_file: m.is_file,
            is_symlink: m.is_symlink,
            size_bytes: m.len,
            modified_ns: m.modified_ns,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}
pub struct DataImport {
    directory: VolumeDirectory,
    staging: Option<tempfile::NamedTempFile>,
    cache: cap_std::fs::Dir,
    target: PathBuf,
    overwrite: bool,
    expected: Option<u64>,
    max_bytes: u64,
    written: u64,
    finished: bool,
    cancel: Arc<AtomicBool>,
}
impl DataImport {
    pub fn begin(
        session: &EngineSession,
        volume: &str,
        path: &str,
        overwrite: bool,
        expected: Option<u64>,
        max_bytes: u64,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        check_cancel(&cancel)?;
        let target = file_path(path)?;
        if max_bytes == 0 || expected.is_some_and(|n| n > max_bytes) {
            return Err(EngineError::Invalid(
                "file exceeds available transfer budget".into(),
            ));
        }
        let directory = VolumeManager::open(session)?.exclusive(volume)?;
        match directory.symlink_metadata(&target) {
            Ok(m) if !overwrite || !m.is_file || m.is_symlink => {
                return Err(EngineError::Conflict(
                    "destination exists; explicit regular-file replacement required".into(),
                ));
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let root = session.root().join("transfers/cache");
        std::fs::create_dir_all(&root)?;
        let staging = tempfile::Builder::new()
            .prefix("wasmdroid-cache-")
            .tempfile_in(&root)?;
        let cache = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
        Ok(Self {
            directory,
            staging: Some(staging),
            cache,
            target,
            overwrite,
            expected,
            max_bytes,
            written: 0,
            finished: false,
            cancel,
        })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<u64> {
        check_cancel(&self.cancel)?;
        if self.finished || bytes.len() > CHUNK_BYTES {
            return Err(EngineError::Invalid(
                "invalid transfer chunk or completed import".into(),
            ));
        }
        let total = self
            .written
            .checked_add(bytes.len() as u64)
            .filter(|n| *n <= self.max_bytes)
            .ok_or_else(|| EngineError::Invalid("transfer budget exceeded".into()))?;
        self.staging
            .as_mut()
            .ok_or_else(|| EngineError::Conflict("import is not writable".into()))?
            .write_all(bytes)?;
        self.written = total;
        Ok(total)
    }
    pub fn commit(&mut self) -> Result<u64> {
        check_cancel(&self.cancel)?;
        if self.finished {
            return Err(EngineError::Conflict("import already finished".into()));
        }
        if self.expected.is_some_and(|n| n != self.written) {
            return Err(EngineError::Invalid(
                "source size changed or transfer is incomplete".into(),
            ));
        }
        let mut file = self
            .staging
            .take()
            .ok_or_else(|| EngineError::Conflict("import is not writable".into()))?;
        file.flush()?;
        file.as_file().sync_all()?;
        ensure_parent(&self.directory, &self.target)?;
        match self.directory.symlink_metadata(&self.target) {
            Ok(m) if !self.overwrite || !m.is_file || m.is_symlink => {
                return Err(EngineError::Conflict(
                    "destination replacement denied".into(),
                ));
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        let pending = file.into_temp_path();
        let name = pending
            .file_name()
            .ok_or_else(|| EngineError::Invalid("invalid private staging file".into()))?;
        check_cancel(&self.cancel)?;
        // Both capabilities are under the same private engine filesystem.
        // Guests cannot access the staging file before this atomic rename.
        self.directory
            .publish_from(&self.cache, Path::new(name), &self.target)?;
        self.finished = true;
        Ok(self.written)
    }
    pub fn abort(&mut self) -> Result<()> {
        self.staging.take();
        self.finished = true;
        Ok(())
    }
}
fn check_cancel(cancel: &AtomicBool) -> std::io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(std::io::Error::other("data transfer cancelled"))
    } else {
        Ok(())
    }
}
// A live transfer retains the engine owner. This runs only after acquiring the
// exclusive owner, so every file in this private, reserved cache is abandoned.
// Unknown names, directories and symlinks are preserved.
pub fn recover_imports(session: &EngineSession) -> Result<()> {
    let cache = session.root().join("transfers/cache");
    let entries = match std::fs::read_dir(cache) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("wasmdroid-cache-")
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

pub struct DataReader {
    reader: Box<dyn Read + Send>,
    pub size_bytes: Option<u64>,
    _owner: EngineOwnership,
}
impl DataReader {
    pub fn file(session: &EngineSession, volume: &str, path: &str) -> Result<Self> {
        let directory = VolumeManager::open(session)?.exclusive(volume)?;
        let path = file_path(path)?;
        let metadata = directory.symlink_metadata(&path)?;
        if !metadata.is_file || metadata.is_symlink {
            return Err(EngineError::Unsupported(
                "export requires a regular data file".into(),
            ));
        }
        let file = directory.open_file(
            path,
            VolumeOpenOptions {
                read: true,
                ..Default::default()
            },
        )?;
        let size_bytes = file.metadata()?.len();
        Ok(Self {
            reader: Box::new(file),
            size_bytes: Some(size_bytes),
            _owner: session.ownership(),
        })
    }
    pub(crate) fn stream(session: &EngineSession, reader: impl Read + Send + 'static) -> Self {
        Self {
            reader: Box::new(reader),
            size_bytes: None,
            _owner: session.ownership(),
        }
    }
    pub fn read(&mut self, max_bytes: u32) -> Result<Vec<u8>> {
        if max_bytes == 0 || max_bytes as usize > CHUNK_BYTES {
            return Err(EngineError::Invalid(
                "read chunk must be 1..65536 bytes".into(),
            ));
        }
        let mut bytes = vec![0; max_bytes as usize];
        let count = self.reader.read(&mut bytes)?;
        bytes.truncate(count);
        Ok(bytes)
    }
}
