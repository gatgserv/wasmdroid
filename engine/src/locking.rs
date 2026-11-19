//! std::fs::File locking is unsupported on Android in the pinned Rust toolchain.
//! Use the same real OS flock/LockFileEx semantics on all supported hosts.
use crate::{EngineError, Result};
use std::fs::File;

pub(crate) fn try_lock(file: &File, shared: bool, conflict: &str) -> Result<()> {
    let result = if shared {
        fs2::FileExt::try_lock_shared(file)
    } else {
        fs2::FileExt::try_lock_exclusive(file)
    };
    match result {
        Ok(()) => Ok(()),
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Err(EngineError::Conflict(conflict.into()))
        }
        Err(error) => Err(error.into()),
    }
}
