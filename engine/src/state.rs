use crate::{
    EngineError, Result,
    api::{Artifact, validate_name},
};
use rusqlite::{Connection, params};
use std::path::Path;

pub struct StateDb {
    connection: Connection,
}

impl StateDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > 1 {
            return Err(EngineError::Unsupported(format!(
                "database schema {version}"
            )));
        }
        if version == 0 {
            connection.execute_batch(include_str!("../migrations/001_initial.sql"))?;
        }
        Ok(Self { connection })
    }
    pub fn put_artifact(&self, artifact: &Artifact) -> Result<()> {
        crate::api::ArtifactReference::parse(artifact.reference.as_str())?;
        validate_name(&artifact.driver)?;
        self.connection.execute(
            "INSERT INTO artifacts(reference, digest, kind, driver, metadata_json) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(reference) DO UPDATE SET digest=excluded.digest,kind=excluded.kind,driver=excluded.driver,metadata_json=excluded.metadata_json",
            params![artifact.reference.as_str(), artifact.digest.as_str(), serde_json::to_string(&artifact.kind)?, artifact.driver, serde_json::to_string(artifact)?])?;
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
