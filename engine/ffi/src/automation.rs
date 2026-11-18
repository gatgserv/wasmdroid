//! Private common-owner run intent and exact-generation lifecycle boundaries.
use crate::{BridgeError, EngineStorage, storage_lock_error};
use wasmdroid_engine::state::RunStartSource;

#[uniffi::export]
impl EngineStorage {
    pub fn automatic_start_source(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session.state().automatic_start_source(&id)?,
        )?)
    }

    pub fn set_automatic_start(
        &self,
        id: String,
        identity: String,
        version: u64,
        start_on_boot: bool,
        restart_policy: String,
    ) -> Result<u64, BridgeError> {
        let policy = serde_json::from_value(serde_json::Value::String(restart_policy))?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(session
            .state()
            .set_automatic_start(&id, &identity, version, start_on_boot, policy)?)
    }

    pub fn browser_identity(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(session.state().browser_identity(&id)?)
    }
    pub fn run_authority(&self, id: String) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(&session.state().run_authority(&id)?)?)
    }
    pub fn stop_run_intent(
        &self,
        id: String,
        version: u64,
        revision: u64,
        generation: Option<String>,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(&session.state().stop_run_intent(
            &id,
            version,
            revision,
            generation.as_deref(),
        )?)?)
    }
    pub fn stop_all_run_intents(&self) -> Result<u64, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(session.state().stop_all_run_intents()?)
    }
    pub fn begin_authorized_run(
        &self,
        id: String,
        version: u64,
        revision: u64,
        generation: String,
        boot_epoch: Option<u64>,
        source: String,
    ) -> Result<String, BridgeError> {
        let source = match source.as_str() {
            "manual" => RunStartSource::Manual,
            "boot" => RunStartSource::Boot,
            "restart" => RunStartSource::Restart,
            "recovery" => RunStartSource::Recovery,
            _ => {
                return Err(wasmdroid_engine::EngineError::Invalid(
                    "unknown run authorization source".into(),
                )
                .into());
            }
        };
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session.state().begin_authorized_run(
                &id,
                version,
                revision,
                &generation,
                boot_epoch,
                source,
            )?,
        )?)
    }
    pub fn recovery_candidates(
        &self,
        boot_epoch: Option<u64>,
        after: Option<String>,
        limit: u32,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session
                .state()
                .recovery_candidates(boot_epoch, after.as_deref(), limit)?,
        )?)
    }
    pub fn ready_authorized_run(
        &self,
        id: String,
        version: u64,
        generation: String,
        ports_json: String,
    ) -> Result<String, BridgeError> {
        if ports_json.len() > 16 * 1024 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "runtime ports exceed input limit".into(),
            )
            .into());
        }
        let ports: Vec<wasmdroid_engine::api::PortBinding> = serde_json::from_str(&ports_json)?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let instance = session
            .state()
            .ready_authorized_run(&id, version, &generation, &ports)?;
        Ok(serde_json::json!({"instance":instance,"authority":session.state().run_authority(&id)?}).to_string())
    }
    pub fn finish_authorized_run(
        &self,
        id: String,
        version: u64,
        generation: String,
        exit_code: Option<i32>,
        reason: Option<String>,
        owner_stopped: bool,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let instance = session.state().finish_authorized_run(
            &id,
            version,
            &generation,
            exit_code,
            reason.as_deref(),
            owner_stopped,
        )?;
        Ok(serde_json::json!({"instance":instance,"authority":session.state().run_authority(&id)?}).to_string())
    }
}
