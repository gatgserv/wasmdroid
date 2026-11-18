//! Private website-data identity. Runtime incarnation changes do not imply
//! provider erasure or a new browser profile for the same application spec.
use super::*;
use crate::api::InstanceState;

impl StateDb {
    /// Read-only lookup for a current actual application. Labels and caller
    /// profile strings never choose or assign this mapping.
    pub fn browser_identity(&self, id: &str) -> Result<String> {
        self.instance(id)?;
        self.connection
            .query_row(
                "SELECT profile_id FROM instance_browser_identity WHERE instance_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| EngineError::Conflict("browser identity is missing".into()))
    }

    pub(super) fn allocate_browser_identity(
        &self,
        transaction: &Connection,
        id: &str,
        event_sequence: i64,
    ) -> Result<()> {
        if event_sequence <= 0 {
            return Err(EngineError::Conflict(
                "invalid browser identity allocation".into(),
            ));
        }
        // ':' cannot occur in a valid actual/legacy instance ID. An arbitrary
        // legacy name therefore cannot collide with a future event sequence.
        let profile = format!("browser:{event_sequence}");
        transaction.execute(
            "INSERT INTO instance_browser_identity(instance_id,profile_id) VALUES (?1,?2)",
            params![id, profile],
        )?;
        transaction.execute(
            "INSERT INTO browser_origin_profiles(profile_id,tracked_from_birth) VALUES (?1,1)",
            [profile],
        )?;
        Ok(())
    }

    /// Same-spec Dead retry only. The freshly created replacement's provisional
    /// mapping has never reached a provider; remove that metadata, then retarget
    /// the retained original profile atomically. No provider data is deleted.
    pub(super) fn move_browser_identity(
        &self,
        transaction: &Connection,
        old_id: &str,
        new_id: &str,
    ) -> Result<()> {
        let old = self.instance(old_id)?;
        let new = self.instance(new_id)?;
        if old_id == new_id
            || old.state != InstanceState::Dead
            || new.state != InstanceState::Created
            || old.kind != new.kind
            || old.driver != new.driver
            || serde_json::to_value(&old.spec)? != serde_json::to_value(&new.spec)?
        {
            return Err(EngineError::Conflict(
                "browser identity retry scope changed".into(),
            ));
        }
        self.browser_identity(old_id)?;
        transaction.execute(
            "DELETE FROM instance_browser_identity WHERE instance_id=?1",
            [new_id],
        )?;
        let changed = transaction.execute(
            "UPDATE instance_browser_identity SET instance_id=?2 WHERE instance_id=?1",
            params![old_id, new_id],
        )?;
        if changed != 1 {
            return Err(EngineError::Conflict(
                "browser identity retry changed".into(),
            ));
        }
        Ok(())
    }
}
