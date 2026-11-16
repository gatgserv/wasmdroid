use crate::{
    EngineError, Result,
    api::{Artifact, validate_name},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
mod lifecycle;
pub use lifecycle::{EngineEvent, InstanceRecord, VolumeRecord};

pub struct StateDb {
    connection: Connection,
}

impl StateDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 2 {
            return Err(EngineError::Unsupported(format!(
                "database schema {version}"
            )));
        }
        if version == 0 {
            connection.execute_batch(include_str!("../migrations/001_initial.sql"))?;
        }
        if version < 2 {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute_batch(include_str!("../migrations/002_lifecycle.sql"))?;
            let mut query = transaction.prepare("SELECT id,spec_json FROM instances")?;
            let instances = query
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(query);
            for (id, spec) in instances {
                let spec: crate::api::InstanceSpec = serde_json::from_str(&spec)?;
                spec.validate()?;
                for mount in spec.mounts {
                    if let crate::api::MountSource::Volume(name) = mount.source {
                        transaction.execute("INSERT INTO instance_volumes(instance_id,volume_name,target,read_only) VALUES (?1,?2,?3,?4)",params![id,name,mount.target,mount.read_only])?;
                    }
                }
            }
            transaction.commit()?;
        }
        Ok(Self { connection })
    }
    pub fn put_artifact(&self, artifact: &Artifact) -> Result<()> {
        self.put_artifacts(std::slice::from_ref(artifact))
    }

    /// Publish an entire verified dependency graph atomically.
    pub fn put_artifacts(&self, artifacts: &[Artifact]) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        for artifact in artifacts {
            crate::api::ArtifactReference::parse(artifact.reference.as_str())?;
            validate_name(&artifact.driver)?;
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT digest FROM artifacts WHERE reference=?1",
                    [artifact.reference.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if existing
                .as_ref()
                .is_some_and(|digest| digest != artifact.digest.as_str())
            {
                return Err(EngineError::Conflict(
                    "an installed immutable version has a different digest".into(),
                ));
            }
            let kind = match artifact.kind {
                crate::api::ArtifactKind::Webc => "webc",
                crate::api::ArtifactKind::Wasm => "wasm",
                crate::api::ArtifactKind::OciImage => "oci_image",
            };
            transaction.execute(
            "INSERT INTO artifacts(reference, digest, kind, driver, metadata_json) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(reference) DO UPDATE SET digest=excluded.digest,kind=excluded.kind,driver=excluded.driver,metadata_json=excluded.metadata_json",
            params![artifact.reference.as_str(), artifact.digest.as_str(), kind, artifact.driver, serde_json::to_string(artifact)?])?;
        }
        transaction.commit()?;
        Ok(())
    }
    pub fn artifacts(&self) -> Result<Vec<Artifact>> {
        let mut query = self
            .connection
            .prepare("SELECT metadata_json FROM artifacts ORDER BY reference")?;
        let rows = query.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
}
