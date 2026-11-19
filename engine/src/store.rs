use crate::{EngineError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest(String);

impl Digest {
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let hash = value.strip_prefix("sha256:").unwrap_or("");
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(EngineError::Invalid(
                "digest must be sha256:<64 lowercase hex digits>".into(),
            ));
        }
        Ok(Self(value))
    }
    pub fn of(bytes: &[u8]) -> Self {
        Self(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn hex(&self) -> &str {
        &self.0[7..]
    }
}
impl TryFrom<String> for Digest {
    type Error = EngineError;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(value)
    }
}
impl From<Digest> for String {
    fn from(value: Digest) -> Self {
        value.0
    }
}

pub struct ContentStore {
    blobs: PathBuf,
    root: PathBuf,
}
pub struct StoreLease {
    _file: File,
}
/// Per-operation CAS policy, not a whole-app/process or filesystem quota.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallBudget {
    pub max_new_bytes: u64,
    pub max_store_bytes: Option<u64>,
}
pub struct StoreWriter {
    _file: File,
    root: PathBuf,
    written: u64,
}
#[derive(Debug, Serialize)]
pub struct GarbageCollectionReport {
    pub blobs_removed: u64,
    pub logical_bytes_removed: u64,
    pub blobs_kept: u64,
}

impl ContentStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let blobs = root.as_ref().join("blobs/sha256");
        fs::create_dir_all(&blobs)?;
        let root = fs::canonicalize(root.as_ref())?;
        Ok(Self {
            blobs: root.join("blobs/sha256"),
            root,
        })
    }
    pub fn installation_lease(&self) -> Result<StoreLease> {
        self.lease(true)
    }
    /// All ingest paths share this exclusive writer admission. It never blocks
    /// runtime reads or discards another writer's data to make room.
    pub fn writer(&self) -> Result<StoreWriter> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("writes.lock"))?;
        crate::locking::try_lock(&file, false, "another package cache writer is active")?;
        Ok(StoreWriter {
            _file: file,
            root: self.root.clone(),
            written: 0,
        })
    }
    fn logical_blob_bytes(&self, control: &crate::operation::OperationControl) -> Result<u64> {
        let mut total = 0u64;
        for (index, entry) in fs::read_dir(&self.blobs)?.enumerate() {
            control.check()?;
            if index >= 65_536 {
                return Err(EngineError::Invalid(
                    "cache inventory exceeds inspection limit".into(),
                ));
            }
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                return Err(EngineError::Invalid(
                    "unexpected non-file cache entry".into(),
                ));
            }
            total = total
                .checked_add(entry.metadata()?.len())
                .ok_or_else(|| EngineError::Invalid("cache size overflow".into()))?;
        }
        Ok(total)
    }
    fn lease(&self, shared: bool) -> Result<StoreLease> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("store.lock"))?;
        crate::locking::try_lock(
            &file,
            shared,
            "package store maintenance or installation is active",
        )?;
        Ok(StoreLease { _file: file })
    }
    pub fn uninstall(
        &self,
        state: &crate::state::StateDb,
        reference: &crate::api::ArtifactReference,
    ) -> Result<GarbageCollectionReport> {
        let _lease = self.lease(false)?;
        state.remove_artifact(reference)?;
        self.collect_locked(state)
    }
    pub fn collect_garbage(
        &self,
        state: &crate::state::StateDb,
    ) -> Result<GarbageCollectionReport> {
        let _lease = self.lease(false)?;
        self.collect_locked(state)
    }
    fn collect_locked(&self, state: &crate::state::StateDb) -> Result<GarbageCollectionReport> {
        let live = state
            .artifacts()?
            .into_iter()
            .map(|image| image.digest.as_str().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut report = GarbageCollectionReport {
            blobs_removed: 0,
            logical_bytes_removed: 0,
            blobs_kept: 0,
        };
        for entry in fs::read_dir(&self.blobs)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with("wasmdroid-download-") {
                let size = entry.metadata()?.len();
                fs::remove_file(entry.path())?;
                report.blobs_removed += 1;
                report.logical_bytes_removed = report.logical_bytes_removed.saturating_add(size);
                continue;
            }
            let Ok(digest) = Digest::parse(format!("sha256:{name}")) else {
                continue;
            };
            if live.contains(digest.as_str()) {
                report.blobs_kept += 1;
                continue;
            }
            let size = entry.metadata()?.len();
            fs::remove_file(entry.path())?;
            report.blobs_removed += 1;
            report.logical_bytes_removed = report.logical_bytes_removed.saturating_add(size);
        }
        Ok(report)
    }
    pub fn path(&self, digest: &Digest) -> PathBuf {
        self.blobs.join(digest.hex())
    }

    /// Verify a cached blob without allocating its full contents in memory.
    pub fn verify(&self, digest: &Digest, max_bytes: u64) -> Result<u64> {
        self.verify_controlled(
            digest,
            max_bytes,
            &crate::operation::OperationControl::default(),
        )
    }
    pub fn verify_controlled(
        &self,
        digest: &Digest,
        max_bytes: u64,
        control: &crate::operation::OperationControl,
    ) -> Result<u64> {
        control.check()?;
        let mut file = File::open(self.path(digest))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            control.check()?;
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            total = total
                .checked_add(read as u64)
                .ok_or_else(|| EngineError::Invalid("blob too large".into()))?;
            if total > max_bytes {
                return Err(EngineError::Invalid(
                    "cached blob exceeds size limit".into(),
                ));
            }
            hasher.update(&buffer[..read]);
        }
        control.check()?;
        let actual = format!("sha256:{}", hex::encode(hasher.finalize()));
        if actual != digest.as_str() {
            return Err(EngineError::DigestMismatch {
                expected: digest.as_str().into(),
                actual,
            });
        }
        Ok(total)
    }

    /// Streams into a temporary file on the same filesystem. Failed verification
    /// never exposes a blob. max_bytes bounds untrusted downloads independently of Content-Length.
    pub fn ingest(&self, reader: impl Read, expected: &Digest, max_bytes: u64) -> Result<u64> {
        let mut writer = self.writer()?;
        self.ingest_controlled(
            reader,
            expected,
            max_bytes,
            &mut writer,
            InstallBudget {
                max_new_bytes: max_bytes,
                max_store_bytes: None,
            },
            &crate::operation::OperationControl::default(),
        )
    }
    pub fn check_write_admission(
        &self,
        writer: &StoreWriter,
        budget: InstallBudget,
        max_bytes: u64,
        control: &crate::operation::OperationControl,
    ) -> Result<()> {
        control.check()?;
        if writer.root != self.root {
            return Err(EngineError::Invalid(
                "cache writer belongs to another store".into(),
            ));
        }
        if writer
            .written
            .checked_add(max_bytes)
            .is_none_or(|total| total > budget.max_new_bytes)
        {
            return Err(EngineError::Invalid(
                "download space budget exceeded".into(),
            ));
        }
        if let Some(limit) = budget.max_store_bytes {
            // Include the full staging file even when replacing a corrupt blob.
            if self
                .logical_blob_bytes(control)?
                .checked_add(max_bytes)
                .is_none_or(|total| total > limit)
            {
                return Err(EngineError::Invalid(
                    "package cache space budget exceeded".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn ingest_controlled(
        &self,
        mut reader: impl Read,
        expected: &Digest,
        max_bytes: u64,
        writer: &mut StoreWriter,
        budget: InstallBudget,
        control: &crate::operation::OperationControl,
    ) -> Result<u64> {
        let _lease = self.installation_lease()?;
        self.check_write_admission(writer, budget, max_bytes, control)?;
        let mut temporary = tempfile::Builder::new()
            .prefix("wasmdroid-download-")
            .tempfile_in(&self.blobs)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            control.check()?;
            let read = reader.read(&mut buffer);
            control.check()?;
            let n = read?;
            if n == 0 {
                break;
            }
            total = total
                .checked_add(n as u64)
                .ok_or_else(|| EngineError::Invalid("blob too large".into()))?;
            if total > max_bytes {
                return Err(EngineError::Invalid("blob exceeds download limit".into()));
            }
            hasher.update(&buffer[..n]);
            temporary.write_all(&buffer[..n])?;
        }
        control.check()?;
        let actual = format!("sha256:{}", hex::encode(hasher.finalize()));
        if actual != expected.as_str() {
            return Err(EngineError::DigestMismatch {
                expected: expected.as_str().into(),
                actual,
            });
        }
        temporary.as_file().sync_all()?;
        control.check()?;
        // Replace an existing (possibly corrupt) cache entry only with verified bytes.
        temporary
            .persist(self.path(expected))
            .map_err(|err| err.error)?;
        writer.written = writer
            .written
            .checked_add(total)
            .ok_or_else(|| EngineError::Invalid("cache write size overflow".into()))?;
        Ok(total)
    }
    pub fn read_verified(&self, digest: &Digest, max_bytes: u64) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        File::open(self.path(digest))?
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(EngineError::Invalid("blob exceeds read limit".into()));
        }
        let actual = Digest::of(&bytes);
        if &actual != digest {
            return Err(EngineError::DigestMismatch {
                expected: digest.as_str().into(),
                actual: actual.0,
            });
        }
        Ok(bytes)
    }
}
