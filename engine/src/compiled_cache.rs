//! Authenticated, disposable native-code bytes. Only locally compiled output
//! may be saved here. This store is outside guest volumes and data backups.
use crate::{
    EngineError, Result,
    session::{EngineOwnership, EngineSession},
};
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"WDCACHE1";
const HEADER_BYTES: u64 = 48;
pub const ENTRY_LIMIT: u64 = 256 * 1024 * 1024;
const CACHE_LIMIT: u64 = 512 * 1024 * 1024;
const ENTRY_COUNT: usize = 512;

/// A context binds the source module, runtime build, target CPU, compiler and
/// artifact format. The caller must derive it from the actual compiler engine.
pub struct TrustedCodeCache {
    root: PathBuf,
    key: hmac::Key,
    _owner: EngineOwnership,
}
impl std::fmt::Debug for TrustedCodeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrustedCodeCache").finish_non_exhaustive()
    }
}
#[derive(Debug, Default, serde::Serialize)]
pub struct CacheRemoval {
    pub entries_removed: u64,
    pub bytes_removed: u64,
}
impl TrustedCodeCache {
    pub fn open(session: &EngineSession) -> Result<Self> {
        let root = session.root().join("compiled-cache");
        std::fs::create_dir_all(&root)?;
        if !std::fs::symlink_metadata(&root)?.file_type().is_dir() {
            return Err(EngineError::Invalid(
                "compiled cache must be a private directory".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        }
        let guard = lock(&root, true)?;
        let path = root.join("authentication.key");
        let key = if path.exists() {
            let mut key = [0u8; 32];
            let mut file = open_regular(&path)?;
            if file.metadata()?.len() != key.len() as u64 {
                return Err(EngineError::Invalid(
                    "compiled cache key is invalid; cache disabled".into(),
                ));
            }
            file.read_exact(&mut key)?;
            key
        } else {
            let mut key = [0u8; 32];
            SystemRandom::new()
                .fill(&mut key)
                .map_err(|_| EngineError::Invalid("secure cache key generation failed".into()))?;
            let mut pending = tempfile::Builder::new()
                .prefix("wasmdroid-code-")
                .tempfile_in(&root)?;
            pending.write_all(&key)?;
            pending.as_file().sync_all()?;
            pending.persist_noclobber(&path).map_err(|e| e.error)?;
            key
        };
        drop(guard);
        Ok(Self {
            root,
            key: hmac::Key::new(hmac::HMAC_SHA256, &key),
            _owner: session.ownership(),
        })
    }

    /// Authentication happens on the exact returned byte buffer, before any
    /// runtime deserializer sees it. Invalid/foreign/partial entries are misses.
    pub fn load(&self, context: &[u8]) -> Result<Option<Vec<u8>>> {
        let identity = digest::digest(&digest::SHA256, context);
        let _guard = lock(&self.root, false)?;
        let path = self.path(identity.as_ref());
        let mut file = match open_regular(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let length = file.metadata()?.len();
        if !(HEADER_BYTES..=HEADER_BYTES + ENTRY_LIMIT).contains(&length) {
            return Ok(None);
        }
        let mut header = [0u8; HEADER_BYTES as usize];
        file.read_exact(&mut header)?;
        if &header[..8] != MAGIC {
            return Ok(None);
        }
        let size = u64::from_le_bytes(header[8..16].try_into().unwrap());
        if size != length - HEADER_BYTES {
            return Ok(None);
        }
        let mut bytes = vec![0; size as usize];
        file.read_exact(&mut bytes)?;
        // Also reject a file extended while open. App cache writers publish by
        // rename and never modify an existing entry in place.
        if file.read(&mut [0u8; 1])? != 0 {
            return Ok(None);
        }
        let message = authenticated_message(identity.as_ref(), &bytes);
        if hmac::verify(&self.key, &message, &header[16..48]).is_err() {
            return Ok(None);
        }
        let _ =
            file.set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()));
        Ok(Some(bytes))
    }

    /// Only driver-produced serialization belongs here; never save downloaded
    /// native bytes, imported documents or a guest-selected cache context.
    pub fn save_local(&self, context: &[u8], bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > ENTRY_LIMIT {
            return Err(EngineError::Invalid(
                "compiled module exceeds cache entry limit".into(),
            ));
        }
        let identity = digest::digest(&digest::SHA256, context);
        let _guard = lock(&self.root, true)?;
        let path = self.path(identity.as_ref());
        let tag = hmac::sign(&self.key, &authenticated_message(identity.as_ref(), bytes));
        let mut pending = tempfile::Builder::new()
            .prefix("wasmdroid-code-")
            .tempfile_in(&self.root)?;
        pending.write_all(MAGIC)?;
        pending.write_all(&(bytes.len() as u64).to_le_bytes())?;
        pending.write_all(tag.as_ref())?;
        pending.write_all(bytes)?;
        pending.as_file().sync_all()?;
        pending.persist(&path).map_err(|e| e.error)?;
        self.trim(Some(&path), CACHE_LIMIT, ENTRY_COUNT)?;
        Ok(())
    }

    /// Loaded modules own their authenticated memory buffer. Clearing this
    /// disposable cache never removes data volumes or installed WebC blobs.
    pub fn clear(&self) -> Result<CacheRemoval> {
        let _guard = lock(&self.root, true)?;
        self.trim(None, 0, 0)
    }
    fn path(&self, identity: &[u8]) -> PathBuf {
        self.root.join(format!("{}.cache", hex::encode(identity)))
    }
    fn trim(
        &self,
        keep: Option<&Path>,
        byte_limit: u64,
        count_limit: usize,
    ) -> Result<CacheRemoval> {
        let mut entries = Vec::new();
        let mut total = 0u64;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let cache = name
                .strip_suffix(".cache")
                .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()));
            let pending = name.starts_with("wasmdroid-code-");
            if !cache && !pending {
                continue;
            }
            let metadata = entry.metadata()?;
            total = total.saturating_add(metadata.len());
            entries.push((!pending, metadata.modified()?, entry.path(), metadata.len()));
        }
        // Abandoned private staging files first, then oldest successfully used.
        entries.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
        let mut count = entries.len();
        let mut result = CacheRemoval::default();
        for (cache, _, path, size) in entries {
            if Some(path.as_path()) == keep {
                continue;
            }
            if cache && total <= byte_limit && count <= count_limit {
                continue;
            }
            std::fs::remove_file(path)?;
            total = total.saturating_sub(size);
            count -= 1;
            result.entries_removed += 1;
            result.bytes_removed = result.bytes_removed.saturating_add(size);
        }
        Ok(result)
    }
}

fn authenticated_message(identity: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(80);
    message.extend_from_slice(MAGIC);
    message.extend_from_slice(identity);
    message.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    message.extend_from_slice(digest::digest(&digest::SHA256, bytes).as_ref());
    message
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
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    options
}
fn open_regular(path: &Path) -> std::io::Result<File> {
    let file = options().read(true).open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("cache entry is not a regular file"));
    }
    Ok(file)
}
fn lock(root: &Path, exclusive: bool) -> Result<File> {
    let file = options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("cache.lock"))?;
    if !file.metadata()?.is_file() {
        return Err(EngineError::Invalid(
            "cache lock is not a regular file".into(),
        ));
    }
    crate::locking::try_lock(&file, !exclusive, "compiled cache is busy")?;
    Ok(file)
}
