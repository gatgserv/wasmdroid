//! Catalog request metadata and effective operator grants are distinct.
use super::{BridgeError, EngineStorage, catalog_package};

#[uniffi::export]
impl EngineStorage {
    /// Read-only normal-preparation spec. No metadata/data/guest is created and
    /// signed requests are preserved separately; Install grants neither access.
    pub fn catalog_application_spec(&self, package_json: String) -> Result<String, BridgeError> {
        application_spec(&package_json)
    }
}
fn application_spec(json: &str) -> Result<String, BridgeError> {
    let package = catalog_package(json)?;
    let effective = wasmdroid_engine::catalog_install::effective_single_spec(&package)?;
    Ok(serde_json::to_string(&effective)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> serde_json::Value {
        serde_json::json!({"id":"fixture","name":"Fixture","publisher":"Fixture","description":"Boundary fixture",
            "digest":format!("sha256:{}","a".repeat(64)),"download_url":"https://example.com/fixture.webc",
            "android_verified":true,"evidence":"fixture-only-not-device-evidence",
            "recipe":{"artifact_ref":"wasmer.io/fixture/package@1.0.0","command":"run","internet_access":true,"lan_access":true},"web":null})
    }
    #[test]
    fn normal_prepare_codec_retains_requests_but_returns_no_outbound_grants() {
        let requested = entry();
        let raw = requested.to_string();
        let effective: wasmdroid_engine::api::InstanceSpec =
            serde_json::from_str(&application_spec(&raw).unwrap()).unwrap();
        assert!(!effective.internet_access && !effective.lan_access);
        let unchanged = catalog_package(&raw).unwrap();
        let request = unchanged.single_recipe().unwrap();
        assert!(request.internet_access && request.lan_access);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&raw).unwrap(),
            requested
        );
    }
    #[test]
    fn prepare_codec_rejects_coerced_permissions_unknown_fields_and_trailing_input() {
        let mut malformed = entry();
        malformed["recipe"]["lan_access"] = serde_json::json!("true");
        assert!(application_spec(&malformed.to_string()).is_err());
        let mut unknown = entry();
        unknown["recipe"]["outbound_lan"] = true.into();
        assert!(application_spec(&unknown.to_string()).is_err());
        assert!(application_spec(&format!("{}{{}}", entry())).is_err());
        assert!(application_spec(&" ".repeat(128 * 1024 + 1)).is_err());
    }
}
