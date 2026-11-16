//! Finite data-plane acceptance probe in a uniquely owned temporary directory.
//! State transitions below simulate metadata recovery; no guest execution is claimed.
use crate::{
    EngineError, Result,
    session::EngineSession,
    volumes::{VolumeManager, VolumeOpenOptions},
};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
};

#[derive(Debug, serde::Serialize)]
pub struct StorageProbeResult {
    pub persisted_bytes: usize,
    pub read_only_enforced: bool,
    pub lease_prevented_remove: bool,
    pub escape_denied: bool,
    pub removed: bool,
}

pub fn run(parent: &Path) -> Result<StorageProbeResult> {
    std::fs::create_dir_all(parent)?;
    let root = tempfile::tempdir_in(parent)?;
    let session = EngineSession::open(root.path())?;
    let volumes = VolumeManager::open(&session)?;
    volumes.create("probe", BTreeMap::new())?;
    let writable = volumes.lease("probe", false)?;
    let options = VolumeOpenOptions {
        read: true,
        write: true,
        create: true,
        ..Default::default()
    };
    let mut file = writable.open_file("note", options)?;
    file.write_all(b"persistent WasmDroid volume")?;
    file.sync_all()?;
    drop(writable);
    let lease_prevented_remove = matches!(volumes.remove("probe"), Err(EngineError::Conflict(_)));
    drop(file);
    drop(volumes);
    drop(session);
    let session = EngineSession::open(root.path())?;
    let volumes = VolumeManager::open(&session)?;
    let readonly = volumes.lease("probe", true)?;
    let mut text = String::new();
    readonly
        .open_file(
            "note",
            VolumeOpenOptions {
                read: true,
                ..Default::default()
            },
        )?
        .read_to_string(&mut text)?;
    let read_only_enforced =
        readonly.open_file("note", options).is_err() && readonly.remove_file("note").is_err();
    let escape_denied = readonly
        .open_file(
            "../outside",
            VolumeOpenOptions {
                read: true,
                ..Default::default()
            },
        )
        .is_err();
    #[cfg(unix)]
    let escape_denied = {
        std::fs::write(root.path().join("host-only"), b"host sentinel")?;
        std::os::unix::fs::symlink(
            root.path().join("host-only"),
            root.path().join("volumes/data/probe/escape"),
        )?;
        escape_denied
            && readonly
                .open_file(
                    "escape",
                    VolumeOpenOptions {
                        read: true,
                        ..Default::default()
                    },
                )
                .is_err()
    };
    if text != "persistent WasmDroid volume"
        || !read_only_enforced
        || !lease_prevented_remove
        || !escape_denied
    {
        return Err(EngineError::Invalid(
            "storage acceptance invariant failed".into(),
        ));
    }
    drop(readonly);
    volumes.remove("probe")?;
    #[cfg(unix)]
    if std::fs::read(root.path().join("host-only"))? != b"host sentinel" {
        return Err(EngineError::Invalid(
            "recursive removal escaped its volume".into(),
        ));
    }
    let removed = volumes.list()?.is_empty();
    if !removed {
        return Err(EngineError::Invalid(
            "volume removal did not complete".into(),
        ));
    }
    Ok(StorageProbeResult {
        persisted_bytes: text.len(),
        read_only_enforced,
        lease_prevented_remove,
        escape_denied,
        removed,
    })
}
