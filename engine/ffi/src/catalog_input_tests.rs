use super::*;

fn entry() -> String {
    serde_json::json!({"id":"fixture","name":"Fixture","publisher":"Fixture","description":"Boundary fixture",
        "digest":format!("sha256:{}","a".repeat(64)),"download_url":"https://example.com/fixture.webc",
        "android_verified":false,"evidence":null,"recipe":{"artifact_ref":"wasmer.io/fixture/package@1.0.0","command":"run"},"web":null}).to_string()
}

#[test]
fn one_entry_boundary_rejects_comma_separated_catalog_objects_and_private_trailing_input() {
    let valid = entry();
    assert!(catalog_package(&valid).is_ok());
    assert!(catalog_package(&format!("{valid},{valid}")).is_err());
    let failure = catalog_package(&format!("{valid} PRIVATE_MARKER")).unwrap_err();
    assert!(matches!(failure,BridgeError::Operation{reason} if reason=="Invalid catalog entry"));
    assert!(catalog_package(&" ".repeat(128 * 1024 + 1)).is_err());
}

#[test]
fn media_entry_uses_its_explicit_feature_schema_without_changing_legacy_shape() {
    // Generated first-party transparent pixel; not Android screenshot evidence.
    let image = serde_json::json!({
        "format":"png","size_bytes":68,
        "digest":"sha256:43739c566e26fd7cb88f69d3864ea34740372f5ee99acac169e090beffbce5c6",
        "width":1,"height":1,
        "data_base64":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=",
        "alt":"Synthetic test pixel",
        "provenance":{"license":"LicenseRef-TestFixture","attribution":"First-party contract fixture",
            "report":"docs/verification/media-fixture-only.md"}
    });
    let legacy = catalog_package(&entry()).unwrap();
    assert!(serde_json::to_value(legacy).unwrap().get("media").is_none());
    let mut value: serde_json::Value = serde_json::from_str(&entry()).unwrap();
    value["media"] = serde_json::json!({"icon":image,"screenshots":[image]});
    assert!(catalog_package(&value.to_string()).unwrap().media.is_some());
    value["media"]["icon"]["width"] = serde_json::json!(2);
    assert!(catalog_package(&value.to_string()).is_err());
    value["media"] = serde_json::Value::Null;
    assert!(catalog_package(&value.to_string()).is_err());
}
