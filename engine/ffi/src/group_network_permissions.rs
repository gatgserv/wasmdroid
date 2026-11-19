//! Owner-private opaque reviews. Display JSON never reconstructs consent.
use super::{BridgeError, EngineStorage, storage_lock_error};
use std::sync::Arc;
use wasmdroid_engine::state::{ApplicationTarget, GroupNetworkReview, ManagedRunStart};

#[derive(uniffi::Object)]
pub struct GroupNetworkPermissionReview {
    issuer: Arc<()>,
    review: GroupNetworkReview,
}
#[uniffi::export]
impl GroupNetworkPermissionReview {
    pub fn source(&self) -> Result<String, BridgeError> {
        let mut value = serde_json::to_value(self.review.source())?;
        value["supported"] =
            serde_json::Value::Bool(cfg!(all(feature = "runtime", target_os = "android")));
        Ok(value.to_string())
    }
}
impl EngineStorage {
    fn check_group_review(&self, review: &GroupNetworkPermissionReview) -> Result<(), BridgeError> {
        if !Arc::ptr_eq(&self.review_issuer, &review.issuer) {
            return Err(wasmdroid_engine::EngineError::Conflict(
                "permission owner changed; review again".into(),
            )
            .into());
        }
        Ok(())
    }
}
#[uniffi::export]
impl EngineStorage {
    pub fn group_network_permission_review(
        &self,
        target_json: String,
    ) -> Result<Arc<GroupNetworkPermissionReview>, BridgeError> {
        if target_json.len() > 4096 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "application target exceeds limit".into(),
            )
            .into());
        }
        let target: ApplicationTarget = serde_json::from_str(&target_json)?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(Arc::new(GroupNetworkPermissionReview {
            issuer: self.review_issuer.clone(),
            review: session.state().group_network_review(&target)?,
        }))
    }
    pub fn set_group_network_permissions(
        &self,
        review: Arc<GroupNetworkPermissionReview>,
        internet: bool,
        lan: bool,
    ) -> Result<Arc<GroupNetworkPermissionReview>, BridgeError> {
        self.check_group_review(&review)?;
        if (internet || lan) && !cfg!(all(feature = "runtime", target_os = "android")) {
            return Err(wasmdroid_engine::EngineError::Unsupported(
                "outbound networking is unavailable on this build".into(),
            )
            .into());
        }
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let fresh = session
            .state()
            .set_group_network_permissions(&review.review, internet, lan)?;
        Ok(Arc::new(GroupNetworkPermissionReview {
            issuer: self.review_issuer.clone(),
            review: fresh,
        }))
    }
    pub fn begin_managed_runs(
        &self,
        review: Arc<GroupNetworkPermissionReview>,
        starts_json: String,
    ) -> Result<String, BridgeError> {
        self.check_group_review(&review)?;
        if starts_json.len() > 4096 {
            return Err(wasmdroid_engine::EngineError::Invalid(
                "managed start batch exceeds limit".into(),
            )
            .into());
        }
        let starts: Vec<ManagedRunStart> = serde_json::from_str(&starts_json)?;
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session
                .state()
                .begin_managed_runs(&review.review, &starts)?,
        )?)
    }
    /// Launch hint only. The worker verifies actual effective authority again.
    pub fn worker_egress_required(&self, id: String) -> Result<bool, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        let proof = session.state().effective_worker_spec(&id)?;
        Ok(proof.internet() || proof.lan())
    }
    /// Common owner calls only before a Binder bind was ever requested.
    pub fn withdraw_unbound_managed_run(
        &self,
        id: String,
        version: u64,
        generation: String,
    ) -> Result<String, BridgeError> {
        let session = self.session.lock().map_err(|_| storage_lock_error())?;
        Ok(serde_json::to_string(
            &session
                .state()
                .withdraw_unbound_managed_run(&id, version, &generation)?,
        )?)
    }
}
