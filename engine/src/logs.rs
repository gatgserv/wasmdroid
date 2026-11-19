//! Bounded app-private per-run output. Guest output may contain secrets; it is
//! never an event/telemetry payload. Keep three completed/current run snapshots.
pub mod live;
use crate::{EngineError, Result, api::validate_name};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub struct LogStore {
    root: PathBuf,
}
impl LogStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        std::fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: std::fs::canonicalize(root.as_ref())?,
        })
    }
    pub fn store(&self, id: &str, generation: &str, json: &str) -> Result<()> {
        validate_name(id)?;
        validate_name(generation)?;
        if json.len() > 1024 * 1024 {
            return Err(EngineError::Invalid("output snapshot exceeds limit".into()));
        }
        let _: serde_json::Value = serde_json::from_str(json)?;
        let directory = self.root.join(id);
        std::fs::create_dir_all(&directory)?;
        let mut pending = tempfile::NamedTempFile::new_in(&directory)?;
        pending.write_all(json.as_bytes())?;
        pending.as_file().sync_all()?;
        pending
            .persist(directory.join(format!("{generation}.json")))
            .map_err(|e| e.error)?;
        let mut previous = Vec::new();
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !entry.file_type()?.is_file()
                || !name.ends_with(".json")
                || name == format!("{generation}.json")
            {
                continue;
            }
            if validate_name(name.trim_end_matches(".json")).is_err() {
                continue;
            }
            previous.push((entry.metadata()?.modified()?, entry.path()));
        }
        previous.sort_by_key(|item| std::cmp::Reverse(item.0));
        for (_, path) in previous.into_iter().skip(2) {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
    pub fn latest(&self, id: &str) -> Result<Option<String>> {
        validate_name(id)?;
        let directory = self.root.join(id);
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "json")
            {
                files.push((entry.metadata()?.modified()?, entry.path()));
            }
        }
        files.sort_by_key(|item| std::cmp::Reverse(item.0));
        if let Some((_, path)) = files.first() {
            use std::io::Read;
            let mut text = String::new();
            std::fs::File::open(path)?
                .take(1024 * 1024 + 1)
                .read_to_string(&mut text)?;
            if text.len() > 1024 * 1024 {
                return Err(EngineError::Invalid("stored output exceeds limit".into()));
            }
            Ok(Some(text))
        } else {
            Ok(None)
        }
    }
}
