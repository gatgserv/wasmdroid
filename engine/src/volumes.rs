//! Capability-scoped private named volumes. Host paths never appear in wire
//! records. OS leases protect open directories/files; a durable removal intent
//! makes rename/delete recovery preserve data across interrupted operations.
use crate::{EngineError, Result, api::validate_name, session::EngineSession, state::VolumeRecord};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, Write},
    path::{Component, Path},
    sync::Arc,
};

pub struct VolumeManager<'a> {
    session: &'a EngineSession,
    data: Dir,
    locks: Dir,
    trash: Dir,
}
impl<'a> VolumeManager<'a> {
    pub fn open(session: &'a EngineSession) -> Result<Self> {
        let root = session.root().join("volumes");
        std::fs::create_dir_all(&root)?;
        let root = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
        for name in ["data", "locks", "trash"] {
            root.create_dir_all(name)?;
        }
        let manager = Self {
            session,
            data: root.open_dir("data")?,
            locks: root.open_dir("locks")?,
            trash: root.open_dir("trash")?,
        };
        manager.recover_removals()?;
        Ok(manager)
    }
    fn lock(&self, name: &str, shared: bool) -> Result<File> {
        validate_name(name)?;
        let file = self
            .locks
            .open_with(
                format!("{name}.lock"),
                OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(false),
            )?
            .into_std();
        crate::locking::try_lock(&file, shared, "volume has active leases")?;
        Ok(file)
    }
    pub fn create(&self, name: &str, labels: BTreeMap<String, String>) -> Result<VolumeRecord> {
        self.create_record(name, labels, true)
    }
    pub fn create_new(&self, name: &str, labels: BTreeMap<String, String>) -> Result<VolumeRecord> {
        self.create_record(name, labels, false)
    }
    fn create_record(
        &self,
        name: &str,
        labels: BTreeMap<String, String>,
        reuse: bool,
    ) -> Result<VolumeRecord> {
        validate_name(name)?;
        let _lock = self.lock(name, false)?;
        match self.session.state().volume(name) {
            Ok(record) => {
                if !reuse {
                    return Err(EngineError::Conflict("volume already exists".into()));
                }
                if !exists(&self.data, name)? {
                    return Err(EngineError::Conflict(
                        "registered volume data is missing; not recreated".into(),
                    ));
                }
                return Ok(record);
            }
            Err(EngineError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        if exists(&self.data, name)? || exists(&self.trash, name)? {
            return Err(EngineError::Conflict(
                "unregistered volume data exists; preserve it for recovery".into(),
            ));
        }
        self.data.create_dir(name)?;
        match self.session.state().insert_volume(name, labels) {
            Ok(record) => Ok(record),
            Err(error) => {
                // Only the newly created empty directory can be rolled back.
                // Never recursively remove unregistered data on an error.
                let _ = self.data.remove_dir(name);
                Err(error)
            }
        }
    }
    pub fn list(&self) -> Result<Vec<VolumeRecord>> {
        self.session.state().volumes()
    }
    pub fn inspect(&self, name: &str) -> Result<VolumeRecord> {
        self.session.state().volume(name)
    }
    pub fn lease(&self, name: &str, read_only: bool) -> Result<VolumeDirectory> {
        self.lease_mode(name, read_only, true)
    }
    /// Administrative imports and consistent snapshots exclude every guest/file
    /// lease for their entire lifetime. Stop users of the volume first.
    pub fn exclusive(&self, name: &str) -> Result<VolumeDirectory> {
        self.lease_mode(name, false, false)
    }
    fn lease_mode(&self, name: &str, read_only: bool, shared: bool) -> Result<VolumeDirectory> {
        let lock = self.lock(name, shared)?;
        self.session.state().volume(name)?;
        if self
            .session
            .state()
            .volume_deletions()?
            .iter()
            .any(|pending| pending == name)
        {
            return Err(EngineError::Conflict("volume removal is pending".into()));
        }
        let metadata = self.data.symlink_metadata(name)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(EngineError::Invalid(
                "volume root must be a real directory".into(),
            ));
        }
        Ok(VolumeDirectory {
            inner: Arc::new(VolumeInner {
                dir: self.data.open_dir(name)?,
                _lock: lock,
                _owner: self.session.ownership(),
            }),
            read_only,
            exclusive: !shared,
        })
    }
    /// Explicitly deletes this named volume's data; instance removal never calls it.
    pub fn remove(&self, name: &str) -> Result<()> {
        let _lock = self.lock(name, false)?;
        if !exists(&self.data, name)? {
            return Err(EngineError::NotFound("volume data".into()));
        }
        if exists(&self.trash, name)? {
            return Err(EngineError::Conflict(
                "volume removal recovery is pending".into(),
            ));
        }
        self.session.state().begin_volume_removal(name)?;
        if let Err(error) = self.data.rename(name, &self.trash, name) {
            self.session.state().clear_volume_removal(name)?;
            return Err(error.into());
        }
        if let Err(error) = self.session.state().delete_volume_record(name) {
            if self.trash.rename(name, &self.data, name).is_ok() {
                self.session.state().clear_volume_removal(name)?;
            }
            return Err(error);
        }
        self.trash.remove_dir_all(name)?;
        self.session.state().clear_volume_removal(name)?;
        Ok(())
    }
    pub fn recover_removals(&self) -> Result<()> {
        for name in self.session.state().volume_deletions()? {
            let _lock = self.lock(&name, false)?;
            let source = exists(&self.data, &name)?;
            let trash = exists(&self.trash, &name)?;
            match self.session.state().volume(&name) {
                Ok(_) => match (source, trash) {
                    (false, true) => self.trash.rename(&name, &self.data, &name)?,
                    (true, false) => {}
                    _ => {
                        return Err(EngineError::Conflict(
                            "ambiguous interrupted volume removal; data preserved".into(),
                        ));
                    }
                },
                Err(EngineError::NotFound(_)) => {
                    if source {
                        return Err(EngineError::Conflict(
                            "unexpected data after committed volume removal; preserved".into(),
                        ));
                    }
                    if trash {
                        self.trash.remove_dir_all(&name)?;
                    }
                }
                Err(error) => return Err(error),
            }
            self.session.state().clear_volume_removal(&name)?;
        }
        Ok(())
    }
}

fn exists(dir: &Dir, name: &str) -> Result<bool> {
    match dir.symlink_metadata(name) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(EngineError::Invalid(
                    "managed volume entry must be a real directory".into(),
                ));
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone)]
pub struct VolumeDirectory {
    inner: Arc<VolumeInner>,
    read_only: bool,
    exclusive: bool,
}
struct VolumeInner {
    dir: Dir,
    _lock: File,
    _owner: crate::session::EngineOwnership,
}

pub struct VolumeEntry {
    pub name: std::ffi::OsString,
    pub metadata: VolumeMetadata,
}
pub struct VolumeEntries {
    inner: cap_std::fs::ReadDir,
    _lease: Arc<VolumeInner>,
}
impl Iterator for VolumeEntries {
    type Item = std::io::Result<VolumeEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|entry| {
            let entry = entry?;
            Ok(VolumeEntry {
                name: entry.file_name(),
                metadata: metadata(entry.metadata()?),
            })
        })
    }
}

#[derive(Clone, Debug)]
pub struct VolumeMetadata {
    pub is_dir: bool,
    pub is_file: bool,
    pub is_symlink: bool,
    pub len: u64,
    pub accessed_ns: u64,
    pub modified_ns: u64,
    pub created_ns: u64,
}
fn metadata(value: cap_std::fs::Metadata) -> VolumeMetadata {
    let ns = |time: std::io::Result<cap_std::time::SystemTime>| {
        time.ok()
            .and_then(|time| time.into_std().duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|time| time.as_nanos().try_into().ok())
            .unwrap_or(0)
    };
    VolumeMetadata {
        is_dir: value.is_dir(),
        is_file: value.is_file(),
        is_symlink: value.file_type().is_symlink(),
        len: value.len(),
        accessed_ns: ns(value.accessed()),
        modified_ns: ns(value.modified()),
        created_ns: ns(value.created()),
    }
}

#[derive(Default, Clone, Copy)]
pub struct VolumeOpenOptions {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub truncate: bool,
    pub create: bool,
    pub create_new: bool,
}
impl VolumeDirectory {
    pub(crate) fn exclusive_write(&self) -> bool {
        self.exclusive && !self.read_only
    }
    pub(crate) fn publish_new_from(
        &self,
        source: &Dir,
        name: &Path,
        target: &Path,
    ) -> std::io::Result<()> {
        if !self.exclusive_write() {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        relative(name)?;
        relative(target)?;
        source.hard_link(name, &self.inner.dir, target)
    }
    pub(crate) fn sync_directory(&self) -> std::io::Result<()> {
        self.sync_directory_at(Path::new("."))
    }
    pub(crate) fn sync_directory_at(&self, path: &Path) -> std::io::Result<()> {
        if !self.exclusive_write() {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        relative(path)?;
        let mut checked = std::path::PathBuf::new();
        for component in path.components() {
            if let Component::Normal(name) = component {
                checked.push(name);
                let metadata = self.inner.dir.symlink_metadata(&checked)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(std::io::ErrorKind::PermissionDenied.into());
                }
            }
        }
        #[cfg(unix)]
        self.inner.dir.open_dir(path)?.into_std_file().sync_all()?;
        // Windows does not provide the same directory-flush contract. Local
        // tests verify content/atomic no-replace; Android uses the Unix branch.
        Ok(())
    }
    fn writable(&self) -> std::io::Result<()> {
        if self.read_only {
            Err(std::io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(())
        }
    }
    pub fn open_file(
        &self,
        path: impl AsRef<Path>,
        options: VolumeOpenOptions,
    ) -> std::io::Result<VolumeFile> {
        relative(path.as_ref())?;
        if options.write
            || options.append
            || options.truncate
            || options.create
            || options.create_new
        {
            self.writable()?;
        }
        let file = self
            .inner
            .dir
            .open_with(
                path,
                OpenOptions::new()
                    .read(options.read)
                    .write(options.write)
                    .append(options.append)
                    .truncate(options.truncate)
                    .create(options.create)
                    .create_new(options.create_new),
            )?
            .into_std();
        Ok(VolumeFile {
            file,
            _lease: self.inner.clone(),
            read_only: self.read_only,
        })
    }
    pub fn create_dir(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        self.writable()?;
        relative(path.as_ref())?;
        self.inner.dir.create_dir(path)
    }
    pub fn remove_dir(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        self.writable()?;
        relative(path.as_ref())?;
        self.inner.dir.remove_dir(path)
    }
    pub fn remove_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        self.writable()?;
        relative(path.as_ref())?;
        self.inner.dir.remove_file(path)
    }
    pub fn rename(&self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<()> {
        self.writable()?;
        relative(from.as_ref())?;
        relative(to.as_ref())?;
        self.inner.dir.rename(from, &self.inner.dir, to)
    }
    pub(crate) fn publish_from(
        &self,
        source: &Dir,
        name: &Path,
        target: &Path,
    ) -> std::io::Result<()> {
        self.writable()?;
        relative(name)?;
        relative(target)?;
        source.rename(name, &self.inner.dir, target)
    }
    pub fn metadata(&self, path: impl AsRef<Path>) -> std::io::Result<VolumeMetadata> {
        relative(path.as_ref())?;
        self.inner.dir.metadata(path).map(metadata)
    }
    pub fn symlink_metadata(&self, path: impl AsRef<Path>) -> std::io::Result<VolumeMetadata> {
        relative(path.as_ref())?;
        self.inner.dir.symlink_metadata(path).map(metadata)
    }
    pub fn read_dir(&self, path: impl AsRef<Path>) -> std::io::Result<Vec<VolumeEntry>> {
        let entries = self
            .entries(path)?
            .take(100_001)
            .collect::<std::io::Result<Vec<_>>>()?;
        if entries.len() > 100_000 {
            return Err(std::io::Error::other(
                "directory listing exceeds entry budget",
            ));
        }
        Ok(entries)
    }
    pub fn entries(&self, path: impl AsRef<Path>) -> std::io::Result<VolumeEntries> {
        relative(path.as_ref())?;
        Ok(VolumeEntries {
            inner: self.inner.dir.read_dir(path)?,
            _lease: self.inner.clone(),
        })
    }
    pub fn read_only(&self) -> bool {
        self.read_only
    }
}
pub(crate) fn relative(path: &Path) -> std::io::Result<()> {
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) || path.as_os_str().to_string_lossy().contains(['\\', '\0'])
    {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

pub struct VolumeFile {
    file: File,
    _lease: Arc<VolumeInner>,
    read_only: bool,
}
impl VolumeFile {
    pub fn metadata(&self) -> std::io::Result<std::fs::Metadata> {
        self.file.metadata()
    }
    pub fn set_len(&self, len: u64) -> std::io::Result<()> {
        if self.read_only {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        self.file.set_len(len)
    }
    pub fn sync_all(&self) -> std::io::Result<()> {
        self.file.sync_all()
    }
}
impl Read for VolumeFile {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}
impl Write for VolumeFile {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.read_only {
            return Err(std::io::ErrorKind::PermissionDenied.into());
        }
        self.file.write(buffer)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
impl Seek for VolumeFile {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}
