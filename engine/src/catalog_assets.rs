//! Explicit bounded inline seed files. Data, not executable/host capabilities.
//! Only root-owned new scoped volumes with matching seed receipts may be seeded.
use crate::{EngineError, Result, store::Digest};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_FILES: usize = 32;
pub const MAX_BYTES: usize = 64 * 1024;
pub const SEED_LABEL: &str = "dev.wasmdroid.catalog.seed";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSeedFile {
    /// Compose logical volume, never an actual dataset name or host path.
    pub volume: String,
    pub path: String,
    pub text: String,
    /// Exact UTF-8 byte length, not character count.
    pub size_bytes: u64,
    pub digest: Digest,
}

impl std::fmt::Debug for CatalogSeedFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CatalogSeedFile(<redacted>)")
    }
}

fn invalid() -> EngineError {
    EngineError::Invalid("invalid bounded catalog seed files".into())
}

fn path(value: &str) -> Result<PathBuf> {
    let result = crate::data::file_path(value)?;
    if value.len() > 1024 || value.split('/').count() > 16 {
        return Err(invalid());
    }
    for component in value.split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix)
                    .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
            });
        if component.is_empty()
            || component.len() > 255
            || matches!(component, "." | "..")
            || component.ends_with('.')
            || device
            || !component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(invalid());
        }
    }
    Ok(result)
}

/// Pure structural/content validation, before creating or accessing any volume.
pub fn validate(files: &[CatalogSeedFile]) -> Result<()> {
    if files.len() > MAX_FILES {
        return Err(invalid());
    }
    let mut bytes = 0usize;
    let mut targets = BTreeSet::new();
    let mut spellings = BTreeMap::new();
    for file in files {
        if file.volume.is_empty()
            || file.volume.len() > 63
            || !file.volume.as_bytes()[0].is_ascii_alphanumeric()
            || !file
                .volume
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            || file.text.contains('\0')
            || file.size_bytes != file.text.len() as u64
            || file.digest != Digest::of(file.text.as_bytes())
        {
            return Err(invalid());
        }
        Digest::parse(file.digest.as_str())?;
        path(&file.path)?;
        let mut prefix = String::new();
        for part in file.path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if let Some(previous) =
                spellings.insert((&file.volume, prefix.to_ascii_lowercase()), prefix.clone())
                && previous != prefix
            {
                return Err(invalid());
            }
        }
        bytes = bytes
            .checked_add(file.text.len())
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(invalid)?;
        if !targets.insert((&file.volume, file.path.to_ascii_lowercase())) {
            return Err(invalid());
        }
    }
    for (volume, target) in &targets {
        let mut current = target.as_str();
        while let Some((parent, _)) = current.rsplit_once('/') {
            if targets.contains(&(*volume, parent.to_owned())) {
                return Err(invalid());
            }
            current = parent;
        }
    }
    Ok(())
}

/// Immutable receipt over all seed metadata and text, independent of JSON input
/// formatting and declaration order. Persist only this digest in volume labels.
pub fn manifest_digest(files: &[CatalogSeedFile]) -> Result<Digest> {
    validate(files)?;
    let mut sorted: Vec<_> = files.iter().collect();
    sorted.sort_by(|a, b| (&a.volume, &a.path).cmp(&(&b.volume, &b.path)));
    let mut bytes = b"WasmDroid catalog seed files v1\0".to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(&sorted)?);
    Ok(Digest::of(&bytes))
}

fn conflict() -> EngineError {
    EngineError::Conflict("catalog seed data differs; existing data preserved".into())
}

fn matches(directory: &crate::volumes::VolumeDirectory, file: &CatalogSeedFile) -> Result<bool> {
    let metadata = match directory.symlink_metadata(&file.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file || metadata.is_symlink || metadata.len != file.size_bytes {
        return Err(conflict());
    }
    let reader = directory.open_file(
        &file.path,
        crate::volumes::VolumeOpenOptions {
            read: true,
            ..Default::default()
        },
    )?;
    let mut bytes = Vec::new();
    reader.take(file.size_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != file.size_bytes || Digest::of(&bytes) != file.digest {
        return Err(conflict());
    }
    Ok(true)
}

/// Check the whole existing tree before any writes. A receipt retry can contain
/// only exact completed seed files and their real parent directories.
fn inspect_existing(
    directory: &crate::volumes::VolumeDirectory,
    files: &[CatalogSeedFile],
) -> Result<()> {
    let targets: BTreeMap<_, _> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut parents = BTreeSet::new();
    for file in files {
        let mut remaining = file.path.as_str();
        while let Some((parent, _)) = remaining.rsplit_once('/') {
            parents.insert(parent);
            remaining = parent;
        }
    }
    let mut pending = vec![String::new()];
    let mut count = 0usize;
    while let Some(current) = pending.pop() {
        for entry in directory.entries(if current.is_empty() { "." } else { &current })? {
            count += 1;
            if count > targets.len() + parents.len() {
                return Err(conflict());
            }
            let entry = entry?;
            let name = entry.name.to_str().ok_or_else(conflict)?;
            let value = if current.is_empty() {
                name.to_owned()
            } else {
                format!("{current}/{name}")
            };
            let metadata = directory.symlink_metadata(&value)?;
            if metadata.is_symlink {
                return Err(conflict());
            }
            if metadata.is_dir && parents.contains(value.as_str()) {
                pending.push(value);
            } else if let Some(file) = targets.get(value.as_str()) {
                if !matches(directory, file)? {
                    return Err(conflict());
                }
            } else {
                return Err(conflict());
            }
        }
    }
    Ok(())
}

struct Staging<'a> {
    directory: &'a cap_std::fs::Dir,
    name: String,
    file: cap_std::fs::File,
}

impl<'a> Staging<'a> {
    fn create(directory: &'a cap_std::fs::Dir) -> Result<Self> {
        use ring::rand::SecureRandom;
        for _ in 0..8 {
            let mut random = [0; 16];
            ring::rand::SystemRandom::new()
                .fill(&mut random)
                .map_err(|_| EngineError::Invalid("catalog seed staging unavailable".into()))?;
            let name = format!("wasmdroid-cache-{}", hex::encode(random));
            match directory.open_with(
                &name,
                cap_std::fs::OpenOptions::new().write(true).create_new(true),
            ) {
                Ok(file) => {
                    return Ok(Self {
                        directory,
                        name,
                        file,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(EngineError::Conflict("catalog seed staging busy".into()))
    }
}

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        // Never clean a caller-supplied or pre-existing name.
        let _ = self.directory.remove_file(&self.name);
    }
}

/// Seed one logical volume through an exclusive writable capability. The caller
/// must first verify its immutable receipt, stack/logical labels and unpublished
/// ownership, and hold this lease through final stack publication. Staging must
/// be the existing private engine transfer cache on the same filesystem.
pub fn seed(
    directory: &crate::volumes::VolumeDirectory,
    files: &[CatalogSeedFile],
    staging: &cap_std::fs::Dir,
) -> Result<()> {
    validate(files)?;
    if !directory.exclusive_write() {
        return Err(EngineError::Conflict(
            "catalog seeding requires an exclusive writable lease".into(),
        ));
    }
    if let Some(first) = files.first()
        && files.iter().any(|file| file.volume != first.volume)
    {
        return Err(invalid());
    }
    inspect_existing(directory, files)?;
    for file in files {
        let parent = file.path.rsplit_once('/').map_or(".", |(parent, _)| parent);
        if matches(directory, file)? {
            directory.sync_directory_at(Path::new(parent))?;
            continue;
        }
        let mut current = String::new();
        let parts: Vec<_> = file.path.split('/').collect();
        for component in &parts[..parts.len() - 1] {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(component);
            match directory.symlink_metadata(&current) {
                Ok(metadata) if metadata.is_dir && !metadata.is_symlink => {}
                Ok(_) => return Err(conflict()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    directory.create_dir(&current)?;
                    let parent = current.rsplit_once('/').map_or(".", |(parent, _)| parent);
                    directory.sync_directory_at(Path::new(parent))?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let mut pending = Staging::create(staging)?;
        pending.file.write_all(file.text.as_bytes())?;
        pending.file.flush()?;
        pending.file.sync_all()?;
        directory.publish_new_from(staging, Path::new(&pending.name), &path(&file.path)?)?;
        directory.sync_directory_at(Path::new(parent))?;
    }
    directory.sync_directory()?;
    Ok(())
}
