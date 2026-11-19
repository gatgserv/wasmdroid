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
pub(super) fn kind_name(kind: ArtifactKind) -> &'static str {
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
pub(super) fn event(
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
    pub fn remove_artifact(&self, reference: &crate::api::ArtifactReference) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let artifact = self.artifact(reference)?;
        let users: i64 = transaction.query_row(
            "SELECT count(*) FROM instances WHERE artifact_ref=?1",
            [reference.as_str()],
            |row| row.get(0),
        )?;
        if users != 0 {
            return Err(EngineError::Conflict(
                "package is used by an instance".into(),
            ));
        }
        for image in self.artifacts()? {
            if image.reference == *reference {
                continue;
            }
            if let Some(deps) = image.labels.get(crate::installer::DEPENDENCIES_LABEL) {
                let dependencies: Vec<crate::api::ArtifactReference> = serde_json::from_str(deps)?;
                if dependencies.contains(reference) {
                    return Err(EngineError::Conflict(
                        "package is required by another installed package".into(),
                    ));
                }
            }
        }
        transaction.execute(
            "DELETE FROM artifacts WHERE reference=?1",
            [reference.as_str()],
        )?;
        event(
            &transaction,
            None,
            kind_name(artifact.kind),
            &artifact.driver,
            serde_json::json!({"type":"image_removed","reference":reference}),
        )?;
        transaction.commit()?;
        Ok(())
    }
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
        if spec.labels.contains_key(crate::stacks::STACK_LABEL)
            || spec.labels.contains_key(crate::stacks::SERVICE_LABEL)
        {
            return Err(EngineError::Unsupported(
                "reserved stack ownership labels".into(),
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        self.insert_instance_row(&transaction, id, spec)?;
        transaction.commit()?;
        self.instance(id)
    }
    pub(super) fn insert_instance_row(
        &self,
        transaction: &Connection,
        id: &str,
        spec: &InstanceSpec,
    ) -> Result<()> {
        validate_name(id)?;
        spec.validate()?;
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
            transaction,
            Some(id),
            kind_name(artifact.kind),
            &artifact.driver,
            serde_json::json!({"type":"instance_created","state":"created"}),
        )?;
        self.allocate_browser_identity(transaction, id, self.connection.last_insert_rowid())?;
        Ok(())
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
        self.transition_with_ports(id, expected_version, next, exit_code, error, (None, None))
    }
    pub fn start_instance(
        &self,
        id: &str,
        expected_version: u64,
        ports: &[crate::api::PortBinding],
    ) -> Result<InstanceRecord> {
        self.transition_with_ports(
            id,
            expected_version,
            InstanceState::Running,
            None,
            None,
            (Some(ports), None),
        )
    }
    pub(super) fn transition_with_ports(
        &self,
        id: &str,
        expected_version: u64,
        next: InstanceState,
        exit_code: Option<i32>,
        error: Option<&str>,
        runtime: (Option<&[crate::api::PortBinding]>, Option<&str>),
    ) -> Result<InstanceRecord> {
        let (ports, generation) = runtime;
        let transaction = self.connection.unchecked_transaction()?;
        let previous = self.instance(id)?;
        let authority = self.guard_automation_transition(id, generation)?;
        if previous.version != expected_version {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        previous.state.transition(next)?;
        if let Some(ports) = ports {
            self.validate_published_browser_origins(id, ports)?;
            if ports.len() != previous.spec.ports.len()
                || ports
                    .iter()
                    .zip(&previous.spec.ports)
                    .any(|(actual, requested)| {
                        actual.host_port == 0
                            || actual.instance_port != requested.instance_port
                            || actual.protocol != requested.protocol
                            || (requested.host_port != 0 && requested.host_port != actual.host_port)
                    })
            {
                return Err(EngineError::Invalid(
                    "runtime port mappings disagree with specification".into(),
                ));
            }
            transaction.execute("DELETE FROM port_bindings WHERE instance_id=?1", [id])?;
            for port in ports {
                let protocol = match port.protocol {
                    crate::api::Protocol::Tcp => "tcp",
                    crate::api::Protocol::Udp => "udp",
                };
                transaction.execute("INSERT INTO port_bindings(instance_id,endpoint_id,host_port,instance_port,protocol,kind,driver) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![id,previous.spec.endpoint_id,port.host_port,port.instance_port,protocol,kind_name(previous.kind),previous.driver])?;
            }
        }
        if (next == InstanceState::Exited
            && exit_code.is_none()
            && error.is_none_or(|value| value.trim().is_empty()))
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
        self.record_automation_transition(&transaction, authority, next, exit_code, error)?;
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
        let committed = self.instance(id)?;
        transaction.commit()?;
        Ok(committed)
    }

    pub fn instance_ports(&self, id: &str) -> Result<Vec<crate::api::PortBinding>> {
        self.instance(id)?;
        let mut query=self.connection.prepare("SELECT host_port,instance_port,protocol FROM port_bindings WHERE instance_id=?1 ORDER BY instance_port")?;
        let rows = query.query_map([id], |row| {
            Ok((
                row.get::<_, u16>(0)?,
                row.get::<_, u16>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (host_port, instance_port, protocol) = row?;
            Ok(crate::api::PortBinding {
                host_port,
                instance_port,
                protocol: serde_json::from_value(protocol.into())?,
            })
        })
        .collect()
    }

    pub fn remove_instance(&self, id: &str, expected_version: u64) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        if self.stack_for_instance(id)?.is_some() {
            return Err(EngineError::Conflict(
                "instance belongs to a stack; remove through the stack".into(),
            ));
        }
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
        self.clear_run_intent_for_edit(&transaction, id)?;
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
    /// An explicitly requested retry creates a new identity/generation. Dead
    /// stays terminal; stale callbacks for the old id cannot affect the retry.
    pub fn retry_dead_instance(
        &self,
        id: &str,
        expected_version: u64,
        new_id: &str,
    ) -> Result<InstanceRecord> {
        validate_name(new_id)?;
        let transaction = self.connection.unchecked_transaction()?;
        if self.stack_for_instance(id)?.is_some() {
            return Err(EngineError::Unsupported(
                "managed stack retry requires its stack controller".into(),
            ));
        }
        let old = self.instance(id)?;
        if old.version != expected_version || old.state != InstanceState::Dead {
            return Err(EngineError::Conflict(
                "only the current dead instance can be replaced".into(),
            ));
        }
        transaction.execute("INSERT INTO instances(id,endpoint_id,artifact_ref,kind,driver,spec_json,state) VALUES (?1,?2,?3,?4,?5,?6,'created')",
            params![new_id,old.spec.endpoint_id,old.spec.artifact_ref.as_str(),kind_name(old.kind),old.driver,serde_json::to_string(&old.spec)?])?;
        transaction.execute("INSERT INTO instance_volumes(instance_id,volume_name,target,read_only) SELECT ?1,volume_name,target,read_only FROM instance_volumes WHERE instance_id=?2",params![new_id,id])?;
        self.copy_run_authority(&transaction, id, new_id)?;
        self.move_browser_identity(&transaction, id, new_id)?;
        transaction.execute("DELETE FROM instances WHERE id=?1", [id])?;
        event(
            &transaction,
            Some(id),
            kind_name(old.kind),
            &old.driver,
            serde_json::json!({"type":"instance_replaced","replacement":new_id}),
        )?;
        event(
            &transaction,
            Some(new_id),
            kind_name(old.kind),
            &old.driver,
            serde_json::json!({"type":"instance_created","state":"created","replaces":id}),
        )?;
        transaction.commit()?;
        self.instance(new_id)
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
            "SELECT id FROM instances WHERE state IN ('running','paused','restarting') OR EXISTS(SELECT 1 FROM instance_automation WHERE instance_id=instances.id AND outcome IN ('starting','running')) ORDER BY id",
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
            self.record_automation_transition(
                &transaction,
                self.run_authority(id)?,
                InstanceState::Dead,
                None,
                Some("Engine session ended before a verified exit"),
            )?;
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
    /// Attach restored data only to an inactive instance, preserving the old
    /// volume as an independent recovery copy. Spec, references and event agree.
    pub fn replace_instance_volume(
        &self,
        id: &str,
        expected_version: u64,
        old_volume: &str,
        new_volume: &str,
    ) -> Result<InstanceRecord> {
        validate_name(old_volume)?;
        validate_name(new_volume)?;
        if old_volume == new_volume {
            return Err(EngineError::Invalid(
                "restore requires a new data volume".into(),
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        let previous = self.instance(id)?;
        if previous.version != expected_version
            || matches!(
                previous.state,
                InstanceState::Running | InstanceState::Paused | InstanceState::Restarting
            )
        {
            return Err(EngineError::Conflict(
                "stop the current instance before replacing its data".into(),
            ));
        }
        self.volume(new_volume)?;
        if self.volume_deletions()?.contains(&new_volume.to_owned()) {
            return Err(EngineError::Conflict(
                "restored volume removal is pending".into(),
            ));
        }
        let mut spec = previous.spec.clone();
        let mut changed = 0usize;
        for mount in &mut spec.mounts {
            if matches!(&mount.source,MountSource::Volume(name) if name==old_volume) {
                mount.source = MountSource::Volume(new_volume.into());
                changed += 1;
            }
        }
        if changed == 0 {
            return Err(EngineError::NotFound(
                "selected instance data volume".into(),
            ));
        }
        let version = sql_u64(
            expected_version
                .checked_add(1)
                .ok_or_else(|| EngineError::Invalid("instance revision overflow".into()))?,
        )?;
        self.clear_run_intent_for_edit(&transaction, id)?;
        let updated = transaction.execute(
            "UPDATE instances SET spec_json=?1,version=?2 WHERE id=?3 AND version=?4",
            params![
                serde_json::to_string(&spec)?,
                version,
                id,
                sql_u64(expected_version)?
            ],
        )?;
        if updated != 1 {
            return Err(EngineError::Conflict("stale instance revision".into()));
        }
        let references = transaction.execute(
            "UPDATE instance_volumes SET volume_name=?1 WHERE instance_id=?2 AND volume_name=?3",
            params![new_volume, id, old_volume],
        )?;
        if references != changed {
            return Err(EngineError::Conflict(
                "instance mount references do not match its specification".into(),
            ));
        }
        event(
            &transaction,
            Some(id),
            kind_name(previous.kind),
            &previous.driver,
            serde_json::json!({"type":"instance_data_replaced","old_volume":old_volume,"new_volume":new_volume}),
        )?;
        transaction.commit()?;
        self.instance(id)
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
        if !self.volume_references(name)?.is_empty() || self.stack_references_volume(name)? {
            return Err(EngineError::Conflict(
                "volume is referenced by an instance or stack".into(),
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
        if !self.volume_references(name)?.is_empty() || self.stack_references_volume(name)? {
            return Err(EngineError::Conflict(
                "volume is referenced by an instance or stack".into(),
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
