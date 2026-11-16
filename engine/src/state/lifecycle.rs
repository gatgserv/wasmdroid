//! Durable neutral lifecycle metadata. Runtime side effects are supervised
//! separately; a database transition is never evidence of guest execution.
use super::*;
use crate::api::{ArtifactKind, InstanceSpec, InstanceState, MountSource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRecord {
    pub id: String,
    pub kind: ArtifactKind,
    pub driver: String,
    pub spec: InstanceSpec,
    pub state: InstanceState,
    /// Compare-and-swap revision prevents an old exit watcher overwriting restart.
    pub version: u64,
    pub exit_code: Option<i32>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeRecord {
    pub name: String,
    pub kind: String,
    pub driver: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineEvent {
    pub sequence: u64,
    pub instance_id: Option<String>,
    pub kind: String,
    pub driver: String,
    pub timestamp_ms: u64,
    /// Event payloads deliberately exclude instance args, env and mount paths.
    pub payload: serde_json::Value,
}

fn state_name(state: InstanceState) -> &'static str {
    use InstanceState::*;
    match state {
        Created => "created",
        Running => "running",
        Paused => "paused",
        Restarting => "restarting",
        Exited => "exited",
        Dead => "dead",
    }
}
fn kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Webc => "webc",
        ArtifactKind::Wasm => "wasm",
        ArtifactKind::OciImage => "oci_image",
    }
}
fn now_ms() -> Result<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| EngineError::Invalid("system clock precedes epoch".into()))?
        .as_millis()
        .try_into()
        .map_err(|_| EngineError::Invalid("system clock out of range".into()))
}
fn event(
    connection: &Connection,
    id: Option<&str>,
    kind: &str,
    driver: &str,
    payload: serde_json::Value,
) -> Result<()> {
    connection.execute("INSERT INTO events(instance_id,kind,driver,timestamp_ms,payload_json) VALUES (?1,?2,?3,?4,?5)",params![id,kind,driver,now_ms()?,serde_json::to_string(&payload)?])?;
    Ok(())
}

impl StateDb {
    pub fn artifact(&self, reference: &crate::api::ArtifactReference) -> Result<Artifact> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT metadata_json FROM artifacts WHERE reference=?1",
                [reference.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(&json.ok_or_else(|| {
            EngineError::NotFound("installed artifact".into())
        })?)?)
    }

    pub fn instance(&self, id: &str) -> Result<InstanceRecord> {
        validate_name(id)?;
        let raw=self.connection.query_row("SELECT kind,driver,spec_json,state,version,exit_code,last_error FROM instances WHERE id=?1",[id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?,row.get::<_,Option<i32>>(5)?,row.get::<_,Option<String>>(6)?))).optional()?;
        let (kind, driver, spec, state, version, exit_code, last_error) =
            raw.ok_or_else(|| EngineError::NotFound("instance".into()))?;
        Ok(InstanceRecord {
            id: id.into(),
            kind: serde_json::from_value(kind.into())?,
            driver,
            spec: serde_json::from_str(&spec)?,
            state: serde_json::from_value(state.into())?,
            version: wire_u64(version)?,
            exit_code,
            last_error,
        })
    }

    pub fn instances(&self, endpoint_id: &str) -> Result<Vec<InstanceRecord>> {
        let mut query = self
            .connection
            .prepare("SELECT id FROM instances WHERE endpoint_id=?1 ORDER BY id")?;
        let ids = query
            .query_map([endpoint_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter().map(|id| self.instance(&id)).collect()
    }

    pub fn create_instance(&self, id: &str, spec: &InstanceSpec) -> Result<InstanceRecord> {
        validate_name(id)?;
        spec.validate()?;
        let transaction = self.connection.unchecked_transaction()?;
        if transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM instances WHERE id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(EngineError::Conflict(
                "instance name is already in use".into(),
            ));
        }
        let artifact = self.artifact(&spec.artifact_ref)?;
        if !artifact
            .commands
            .iter()
            .any(|command| command == &spec.command)
        {
            return Err(EngineError::Invalid(
                "command is not present in the installed artifact".into(),
            ));
        }
        transaction.execute("INSERT INTO instances(id,endpoint_id,artifact_ref,kind,driver,spec_json,state) VALUES (?1,?2,?3,?4,?5,?6,'created')",params![id,spec.endpoint_id,spec.artifact_ref.as_str(),kind_name(artifact.kind),artifact.driver,serde_json::to_string(spec)?])?;
        for mount in &spec.mounts {
            if let MountSource::Volume(name) = &mount.source {
                if self.volume_deletions()?.contains(name) {
                    return Err(EngineError::Conflict("volume removal is pending".into()));
                }
                if !transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM volumes WHERE name=?1)",
                    [name],
                    |row| row.get::<_, bool>(0),
                )? {
                    return Err(EngineError::NotFound("named volume".into()));
                }
                transaction.execute("INSERT INTO instance_volumes(instance_id,volume_name,target,read_only) VALUES (?1,?2,?3,?4)",params![id,name,mount.target,mount.read_only])?;
            }
        }
        event(
            &transaction,
            Some(id),
            kind_name(artifact.kind),
            &artifact.driver,
            serde_json::json!({"type":"instance_created","state":"created"}),
        )?;
        transaction.commit()?;
        self.instance(id)
    }

    /// Updates state and event together. Call only after verified runtime outcome.
    pub fn transition_instance(
        &self,
        id: &str,
        expected_version: u64,
        next: InstanceState,
        exit_code: Option<i32>,
        error: Option<&str>,
    ) -> Result<InstanceRecord> {
        let transaction = self.connection.unchecked_transaction()?;
        let previous = self.instance(id)?;
        if previous.version != expected_version {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        previous.state.transition(next)?;
        if (next == InstanceState::Exited && exit_code.is_none())
            || (!matches!(next, InstanceState::Exited | InstanceState::Dead)
                && (exit_code.is_some() || error.is_some()))
        {
            return Err(EngineError::Invalid(
                "exit information contradicts instance state".into(),
            ));
        }
        let version = expected_version
            .checked_add(1)
            .ok_or_else(|| EngineError::Invalid("instance revision overflow".into()))?;
        let changed=transaction.execute("UPDATE instances SET state=?1,version=?2,exit_code=?3,last_error=?4 WHERE id=?5 AND version=?6",params![state_name(next),sql_u64(version)?,exit_code,error,id,sql_u64(expected_version)?])?;
        if changed != 1 {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        if matches!(next, InstanceState::Exited | InstanceState::Dead) {
            transaction.execute("DELETE FROM port_bindings WHERE instance_id=?1", [id])?;
        }
        event(
            &transaction,
            Some(id),
            kind_name(previous.kind),
            &previous.driver,
            serde_json::json!({"type":"instance_state_changed","from":previous.state,"to":next,"exit_code":exit_code}),
        )?;
        transaction.commit()?;
        self.instance(id)
    }

    pub fn remove_instance(&self, id: &str, expected_version: u64) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let previous = self.instance(id)?;
        if previous.version != expected_version {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        if matches!(
            previous.state,
            InstanceState::Running | InstanceState::Paused | InstanceState::Restarting
        ) {
            return Err(EngineError::Conflict(
                "stop the instance before removal".into(),
            ));
        }
        transaction.execute("DELETE FROM instances WHERE id=?1", [id])?;
        event(
            &transaction,
            Some(id),
            kind_name(previous.kind),
            &previous.driver,
            serde_json::json!({"type":"instance_removed"}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn events_after(&self, sequence: u64, limit: u32) -> Result<Vec<EngineEvent>> {
        if !(1..=1000).contains(&limit) {
            return Err(EngineError::Invalid(
                "event page size must be 1..1000".into(),
            ));
        }
        let mut query=self.connection.prepare("SELECT sequence,instance_id,kind,driver,timestamp_ms,payload_json FROM events WHERE sequence>?1 ORDER BY sequence LIMIT ?2")?;
        let rows = query.query_map(params![sql_u64(sequence)?, limit], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (sequence, instance_id, kind, driver, timestamp_ms, payload) = row?;
            Ok(EngineEvent {
                sequence: wire_u64(sequence)?,
                instance_id,
                kind,
                driver,
                timestamp_ms: wire_u64(timestamp_ms)?,
                payload: serde_json::from_str(&payload)?,
            })
        })
        .collect()
    }

    pub(crate) fn reconcile_interrupted(&self) -> Result<usize> {
        let transaction = self.connection.unchecked_transaction()?;
        let mut query = transaction.prepare(
            "SELECT id FROM instances WHERE state IN ('running','paused','restarting') ORDER BY id",
        )?;
        let ids = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(query);
        for id in &ids {
            let previous = self.instance(id)?;
            let version = sql_u64(
                previous
                    .version
                    .checked_add(1)
                    .ok_or_else(|| EngineError::Invalid("instance revision overflow".into()))?,
            )?;
            transaction.execute("UPDATE instances SET state='dead',version=?2,exit_code=NULL,last_error='Engine session ended before a verified exit' WHERE id=?1",params![id,version])?;
            transaction.execute("DELETE FROM port_bindings WHERE instance_id=?1", [id])?;
            event(
                &transaction,
                Some(id),
                kind_name(previous.kind),
                &previous.driver,
                serde_json::json!({"type":"instance_recovered","from":previous.state,"to":"dead"}),
            )?;
        }
        transaction.commit()?;
        Ok(ids.len())
    }

    pub fn volume(&self, name: &str) -> Result<VolumeRecord> {
        validate_name(name)?;
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT metadata_json FROM volumes WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()?;
        Ok(serde_json::from_str(&json.ok_or_else(|| {
            EngineError::NotFound("named volume".into())
        })?)?)
    }
    pub fn volumes(&self) -> Result<Vec<VolumeRecord>> {
        let mut query = self
            .connection
            .prepare("SELECT metadata_json FROM volumes ORDER BY name")?;
        query
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect()
    }
    pub fn volume_references(&self, name: &str) -> Result<Vec<String>> {
        validate_name(name)?;
        let mut query=self.connection.prepare("SELECT DISTINCT instance_id FROM instance_volumes WHERE volume_name=?1 ORDER BY instance_id")?;
        Ok(query
            .query_map([name], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub(crate) fn insert_volume(
        &self,
        name: &str,
        labels: std::collections::BTreeMap<String, String>,
    ) -> Result<VolumeRecord> {
        validate_name(name)?;
        let transaction = self.connection.unchecked_transaction()?;
        if self
            .volume_deletions()?
            .iter()
            .any(|pending| pending == name)
        {
            return Err(EngineError::Conflict("volume removal is pending".into()));
        }
        if transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM volumes WHERE name=?1)",
            [name],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(EngineError::Conflict(
                "volume name is already in use".into(),
            ));
        }
        let record = VolumeRecord {
            name: name.into(),
            kind: "directory".into(),
            driver: "local-fs".into(),
            created_at_ms: wire_u64(now_ms()?)?,
            labels,
        };
        transaction.execute(
            "INSERT INTO volumes(name,kind,driver,metadata_json) VALUES (?1,?2,?3,?4)",
            params![
                name,
                record.kind,
                record.driver,
                serde_json::to_string(&record)?
            ],
        )?;
        event(
            &transaction,
            None,
            &record.kind,
            &record.driver,
            serde_json::json!({"type":"volume_created","name":name}),
        )?;
        transaction.commit()?;
        Ok(record)
    }
    pub(crate) fn delete_volume_record(&self, name: &str) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let record = self.volume(name)?;
        if !self
            .volume_deletions()?
            .iter()
            .any(|pending| pending == name)
        {
            return Err(EngineError::Conflict(
                "missing volume removal intent".into(),
            ));
        }
        if !self.volume_references(name)?.is_empty() {
            return Err(EngineError::Conflict(
                "volume is referenced by an instance".into(),
            ));
        }
        transaction.execute("DELETE FROM volumes WHERE name=?1", [name])?;
        event(
            &transaction,
            None,
            &record.kind,
            &record.driver,
            serde_json::json!({"type":"volume_removed","name":name}),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn begin_volume_removal(&self, name: &str) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        self.volume(name)?;
        if !self.volume_references(name)?.is_empty() {
            return Err(EngineError::Conflict(
                "volume is referenced by an instance".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO volume_deletions(name,requested_at_ms) VALUES (?1,?2)",
            params![name, now_ms()?],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub(crate) fn volume_deletions(&self) -> Result<Vec<String>> {
        let mut query = self
            .connection
            .prepare("SELECT name FROM volume_deletions ORDER BY name")?;
        Ok(query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub(crate) fn clear_volume_removal(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        self.connection
            .execute("DELETE FROM volume_deletions WHERE name=?1", [name])?;
        Ok(())
    }
}

fn sql_u64(value: u64) -> Result<i64> {
    value
        .try_into()
        .map_err(|_| EngineError::Invalid("integer exceeds SQLite range".into()))
}
fn wire_u64(value: i64) -> Result<u64> {
    value
        .try_into()
        .map_err(|_| EngineError::Invalid("negative integer in durable state".into()))
}
