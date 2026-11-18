//! One lifecycle owner per engine directory. Recovery only runs after acquiring
//! an exclusive OS file lock; registry-only readers/writers do not take ownership.
use crate::{Result, state::StateDb};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

pub struct EngineSession {
    db: StateDb,
    _lock: EngineOwnership,
    recovered_instances: usize,
    root: std::path::PathBuf,
}

/// Retain in every active runtime/volume owner so recovery cannot begin while
/// an execution outlives its UI facade or database connection.
#[derive(Clone)]
pub struct EngineOwnership {
    _file: std::sync::Arc<File>,
}

/// A trusted connection factory bound to one existing owner and canonical root.
/// Capturing this handle performs no SQLite admission or lifecycle recovery.
#[derive(Clone)]
pub struct EngineConnection {
    root: std::path::PathBuf,
    ownership: EngineOwnership,
}
impl EngineConnection {
    pub fn connect(&self) -> Result<EngineSession> {
        Ok(EngineSession {
            db: StateDb::open(self.root.join("engine.sqlite"))?,
            _lock: self.ownership.clone(),
            recovered_instances: 0,
            root: self.root.clone(),
        })
    }
}
impl EngineSession {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.as_ref().join("engine.lock"))?;
        crate::locking::try_lock(&lock, false, "another engine session owns this directory")?;
        let db = StateDb::open(root.as_ref().join("engine.sqlite"))?;
        let recovered_instances = db.reconcile_interrupted()?;
        let session = Self {
            db,
            _lock: EngineOwnership {
                _file: std::sync::Arc::new(lock),
            },
            recovered_instances,
            root: std::fs::canonicalize(root.as_ref())?,
        };
        crate::data::recover_imports(&session)?;
        #[cfg(unix)]
        crate::network::grants::recover_endpoints(&session)?;
        crate::local_import::recover(&session)?;
        Ok(session)
    }
    pub fn state(&self) -> &StateDb {
        &self.db
    }
    pub fn recovered_instances(&self) -> usize {
        self.recovered_instances
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn ownership(&self) -> EngineOwnership {
        self._lock.clone()
    }
    pub fn connection_handle(&self) -> EngineConnection {
        EngineConnection {
            root: self.root.clone(),
            ownership: self.ownership(),
        }
    }
    /// Another connection to the same exclusively owned engine, without recovery.
    pub fn connection(&self) -> Result<Self> {
        Ok(Self {
            db: StateDb::open(self.root.join("engine.sqlite"))?,
            _lock: self.ownership(),
            recovered_instances: 0,
            root: self.root.clone(),
        })
    }

    /// Join the coordinator's existing OS lock without running recovery. The
    /// descriptor must have been duplicated over trusted platform IPC.
    #[cfg(unix)]
    pub fn join_worker(root: impl AsRef<Path>, owner: File) -> Result<Self> {
        use std::os::unix::fs::MetadataExt;
        let root = std::fs::canonicalize(root.as_ref())?;
        let expected = std::fs::metadata(root.join("engine.lock"))?;
        let actual = owner.metadata()?;
        if expected.dev() != actual.dev() || expected.ino() != actual.ino() {
            return Err(crate::EngineError::Invalid(
                "worker ownership descriptor mismatch".into(),
            ));
        }
        crate::locking::try_lock(&owner, false, "worker does not own the engine lock")?;
        Ok(Self {
            db: StateDb::open(root.join("engine.sqlite"))?,
            _lock: EngineOwnership {
                _file: std::sync::Arc::new(owner),
            },
            recovered_instances: 0,
            root,
        })
    }

    #[cfg(unix)]
    pub fn duplicate_owner_descriptor(&self) -> Result<std::os::fd::OwnedFd> {
        Ok(self._lock._file.try_clone()?.into())
    }
}
