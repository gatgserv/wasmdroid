use super::{BridgeError, EngineStorage};
#[uniffi::export]
impl EngineStorage {
    pub fn network_permission_source(&self, id: String) -> Result<String, BridgeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| super::storage_lock_error())?;
        let mut value = serde_json::to_value(session.state().network_permission_source(&id)?)?;
        value["supported"] =
            serde_json::Value::Bool(cfg!(all(feature = "runtime", target_os = "android")));
        Ok(value.to_string())
    }
    pub fn set_network_permissions(
        &self,
        id: String,
        identity: String,
        version: u64,
        internet: bool,
        lan: bool,
    ) -> Result<String, BridgeError> {
        if (internet || lan) && !cfg!(all(feature = "runtime", target_os = "android")) {
            return Err(wasmdroid_engine::EngineError::Unsupported(
                "outbound networking is unavailable on this build".into(),
            )
            .into());
        }
        let session = self
            .session
            .lock()
            .map_err(|_| super::storage_lock_error())?;
        Ok(serde_json::to_string(
            &session
                .state()
                .set_network_permissions(&id, &identity, version, internet, lan)?,
        )?)
    }
}
