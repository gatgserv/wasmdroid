//! Prospective numeric loopback-origin history, not browser network isolation.
//! Only the socket allocator claims assignments; instance/profile-data removal
//! never releases them. Legacy missing history cannot be reconstructed.
use super::*;
use crate::api::{InstanceSpec, InstanceState, NetworkMode, PortBinding, Protocol};
use std::{collections::BTreeSet, path::PathBuf};

/// Opaque native capture. Contains private identity/specification; deliberately
/// has no serde, Debug, public constructor or caller-chosen profile fields.
pub struct BrowserOriginCapture {
    database: PathBuf,
    id: String,
    profile: String,
    version: u64,
    specification: String,
    authority: String,
    ports: Vec<PortBinding>,
}

impl BrowserOriginCapture {
    pub(crate) fn bindings(&self) -> &[PortBinding] {
        &self.ports
    }
}

impl StateDb {
    fn origin_database(&self) -> Result<PathBuf> {
        let path = self.connection.path().ok_or_else(|| {
            EngineError::Conflict("browser origin requires an existing database".into())
        })?;
        Ok(std::fs::canonicalize(path)?)
    }

    /// Capture before preparation, then revalidate at reservation/publication.
    /// This is not permission to run: common-owner generation admission remains
    /// required. Existing durable run authority is captured to observe Stop.
    pub fn capture_browser_origins(
        &self,
        id: &str,
        specification: &InstanceSpec,
    ) -> Result<BrowserOriginCapture> {
        let record = self.instance(id)?;
        let serialized = serde_json::to_string(specification)?;
        if serde_json::to_string(&record.spec)? != serialized
            || !matches!(record.state, InstanceState::Created | InstanceState::Exited)
        {
            return Err(EngineError::Conflict(
                "browser origin instance changed or is active".into(),
            ));
        }
        if specification.endpoint_id != "local" {
            return Err(EngineError::Unsupported(
                "browser origins require the local endpoint".into(),
            ));
        }
        let mut guests = BTreeSet::new();
        for port in &specification.ports {
            if specification.network != NetworkMode::Loopback || port.protocol != Protocol::Tcp {
                return Err(EngineError::Unsupported(
                    "browser origins require loopback TCP".into(),
                ));
            }
            if port.instance_port == 0 || !guests.insert(port.instance_port) {
                return Err(EngineError::Unsupported(
                    "stable browser origins require unambiguous guest TCP ports".into(),
                ));
            }
        }
        Ok(BrowserOriginCapture {
            database: self.origin_database()?,
            id: id.into(),
            profile: self.browser_identity(id)?,
            version: record.version,
            specification: serialized,
            authority: serde_json::to_string(&self.run_authority(id)?)?,
            ports: specification.ports.clone(),
        })
    }

    pub(crate) fn validate_origin_capture(&self, capture: &BrowserOriginCapture) -> Result<()> {
        let current = self.instance(&capture.id)?;
        if self.origin_database()? != capture.database
            || current.version != capture.version
            || !matches!(
                current.state,
                InstanceState::Created | InstanceState::Exited
            )
            || self.browser_identity(&capture.id)? != capture.profile
            || serde_json::to_string(&current.spec)? != capture.specification
            || serde_json::to_string(&self.run_authority(&capture.id)?)? != capture.authority
        {
            return Err(EngineError::Conflict(
                "browser origin capture is stale".into(),
            ));
        }
        Ok(())
    }

    /// False for migrated profiles even when their active mapping was recovered:
    /// earlier removed/stopped origins were not persisted. No full browser or
    /// transport exclusivity is implied by this metadata coverage flag.
    pub fn browser_origin_history_from_birth(&self, id: &str) -> Result<bool> {
        let profile = self.browser_identity(id)?;
        Ok(self.connection.query_row(
            "SELECT tracked_from_birth FROM browser_origin_profiles WHERE profile_id=?1",
            [profile],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn saved_origin_ports(
        &self,
        capture: &BrowserOriginCapture,
    ) -> Result<Vec<Option<u16>>> {
        self.validate_origin_capture(capture)?;
        capture.ports.iter().map(|port| {
            Ok(self.connection.query_row(
                "SELECT host_port FROM browser_origin_assignments WHERE profile_id=?1 AND guest_port=?2",
                params![capture.profile, port.instance_port], |row| row.get(0),
            ).optional()?)
        }).collect()
    }

    pub(crate) fn origin_candidate_available(
        &self,
        capture: &BrowserOriginCapture,
        guest: u16,
        host: u16,
    ) -> Result<bool> {
        self.validate_origin_capture(capture)?;
        let owner: Option<(String, u16)> = self
            .connection
            .query_row(
                "SELECT profile_id,guest_port FROM browser_origin_assignments WHERE host_port=?1",
                [host],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(owner.is_none_or(|(profile, port)| profile == capture.profile && port == guest))
    }

    /// Called only with still-owned actual listeners. One transaction claims the
    /// complete set; a stale/concurrent claim rolls back every new assignment.
    pub(crate) fn claim_browser_origins(
        &self,
        capture: &BrowserOriginCapture,
        actual: &[PortBinding],
    ) -> Result<()> {
        let transaction = rusqlite::Transaction::new_unchecked(
            &self.connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        self.validate_origin_capture(capture)?;
        self.check_origin_set(capture, actual, false)?;
        for port in actual {
            transaction.execute(
                "INSERT INTO browser_origin_assignments(profile_id,guest_port,host_port) VALUES (?1,?2,?3) ON CONFLICT(profile_id,guest_port) DO NOTHING",
                params![capture.profile,port.instance_port,port.host_port],
            )?;
        }
        self.check_origin_set(capture, actual, true)?;
        transaction.commit()?;
        Ok(())
    }

    fn check_origin_set(
        &self,
        capture: &BrowserOriginCapture,
        actual: &[PortBinding],
        require_all: bool,
    ) -> Result<()> {
        if actual.len() != capture.ports.len() {
            return Err(EngineError::Conflict(
                "browser origin listener set changed".into(),
            ));
        }
        let saved = self.saved_origin_ports(capture)?;
        for ((requested, port), saved) in capture.ports.iter().zip(actual).zip(saved) {
            if port.host_port == 0
                || port.protocol != requested.protocol
                || port.instance_port != requested.instance_port
                || (requested.host_port != 0 && requested.host_port != port.host_port)
                || saved.is_some_and(|value| value != port.host_port)
                || (require_all && saved.is_none())
                || !self.origin_candidate_available(capture, port.instance_port, port.host_port)?
            {
                return Err(EngineError::Conflict(
                    "browser origin assignment changed".into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn validate_browser_origin_listeners(
        &self,
        capture: &BrowserOriginCapture,
        actual: &[PortBinding],
    ) -> Result<()> {
        self.validate_origin_capture(capture)?;
        self.check_origin_set(capture, actual, true)
    }

    // Lifecycle publication independently checks known assignments. Legacy and
    // neutral drivers with no claim retain the previous metadata contract.
    pub(super) fn validate_published_browser_origins(
        &self,
        id: &str,
        ports: &[PortBinding],
    ) -> Result<()> {
        if self.instance(id)?.spec.endpoint_id != "local" {
            return Ok(());
        }
        let profile = self.browser_identity(id)?;
        for port in ports.iter().filter(|port| port.protocol == Protocol::Tcp) {
            let saved: Option<u16> = self.connection.query_row(
                "SELECT host_port FROM browser_origin_assignments WHERE profile_id=?1 AND guest_port=?2",
                params![profile,port.instance_port], |row| row.get(0),
            ).optional()?;
            if saved.is_some_and(|host| host != port.host_port) {
                return Err(EngineError::Conflict(
                    "runtime ports disagree with stable browser origin".into(),
                ));
            }
            let owner: Option<(String,u16)> = self.connection.query_row(
                "SELECT profile_id,guest_port FROM browser_origin_assignments WHERE host_port=?1",
                [port.host_port], |row| Ok((row.get(0)?,row.get(1)?)),
            ).optional()?;
            if owner
                .is_some_and(|(identity, guest)| identity != profile || guest != port.instance_port)
            {
                return Err(EngineError::Conflict(
                    "runtime port belongs to another browser origin".into(),
                ));
            }
        }
        Ok(())
    }
}
