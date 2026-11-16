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
}

impl ContentStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let blobs = root.as_ref().join("blobs/sha256");
        fs::create_dir_all(&blobs)?;
        Ok(Self { blobs })
    }
    pub fn path(&self, digest: &Digest) -> PathBuf {
        self.blobs.join(digest.hex())
    }

    /// Verify a cached blob without allocating its full contents in memory.
    pub fn verify(&self, digest: &Digest, max_bytes: u64) -> Result<u64> {
        let mut file = File::open(self.path(digest))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
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
    pub fn ingest(&self, mut reader: impl Read, expected: &Digest, max_bytes: u64) -> Result<u64> {
        let mut temporary = tempfile::NamedTempFile::new_in(&self.blobs)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = reader.read(&mut buffer)?;
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
        let actual = format!("sha256:{}", hex::encode(hasher.finalize()));
        if actual != expected.as_str() {
            return Err(EngineError::DigestMismatch {
                expected: expected.as_str().into(),
                actual,
            });
        }
        temporary.as_file().sync_all()?;
        // Replace an existing (possibly corrupt) cache entry only with verified bytes.
        temporary
            .persist(self.path(expected))
            .map_err(|err| err.error)?;
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
