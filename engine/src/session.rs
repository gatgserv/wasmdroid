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
        Ok(Self {
            db,
            _lock: EngineOwnership {
                _file: std::sync::Arc::new(lock),
            },
            recovered_instances,
            root: std::fs::canonicalize(root.as_ref())?,
        })
    }
    pub fn state(&self) -> &StateDb {
        &self.db
    }
    pub fn recovered_instances(&self) -> usize {
        self.recovered_instances
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub fn ownership(&self) -> EngineOwnership {
        self._lock.clone()
    }
}
