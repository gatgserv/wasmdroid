use std::{collections::BTreeMap, fs, io::Cursor};
use wasmdroid_engine::{
    EngineError,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState},
    catalog::Catalog,
    state::StateDb,
    store::{ContentStore, Digest},
};

fn recipe() -> serde_json::Value {
    serde_json::json!({
        "artifact_ref": "wasmer.io/example/site@1.0.0", "command": "serve",
        "ports": [{"host_port": 0, "instance_port": 8080, "protocol": "tcp"}]
    })
}

#[test]
fn spec_rejects_ambiguous_guest_paths_and_invalid_process_values() {
    for path in [
        "relative",
        "/data/../other",
        "/data/./item",
        "/data//item",
        "/data/",
        "/data\\item",
        "/data\nitem",
    ] {
        let mut value = recipe();
        value["working_dir"] = path.into();
        assert!(
            serde_json::from_value::<InstanceSpec>(value)
                .unwrap()
                .validate()
                .is_err(),
            "{path:?}"
        );
    }
    for (field, value) in [
        ("command", serde_json::json!("bad\u{0}command")),
        ("args", serde_json::json!(["bad\u{0}arg"])),
        ("env", serde_json::json!({"": "value"})),
        ("env", serde_json::json!({"BAD=NAME": "value"})),
        ("env", serde_json::json!({"NAME": "bad\u{0}value"})),
        (
            "mounts",
            serde_json::json!([
                {"source":{"type":"volume","source":"first"},"target":"/data","read_only":false},
                {"source":{"type":"volume","source":"second"},"target":"/data","read_only":true}
            ]),
        ),
    ] {
        let mut spec = recipe();
        spec[field] = value;
        assert!(
            serde_json::from_value::<InstanceSpec>(spec)
                .unwrap()
                .validate()
                .is_err(),
            "{field}"
        );
    }
}

fn catalog() -> serde_json::Value {
    serde_json::json!({"schema_version": 1, "packages": [{
        "id": "example-site", "name": "Example", "description": "Test fixture, not a real package",
        "publisher": "example", "digest": Digest::of(b"test"),
        "download_url": "https://example.invalid/site.webc", "android_verified": false,
        "recipe": recipe(), "web": {"instance_port": 8080, "path": "/"}
    }]})
}

#[test]
fn references_require_registry_and_cannot_escape_paths() {
    for invalid in [
        "python/python",
        "../python",
        "wasmer.io/../escape",
        "wasmer.io//x",
        "https://wasmer.io/x",
        "wasmer.io/x\ny",
    ] {
        assert!(ArtifactReference::parse(invalid).is_err(), "{invalid}");
        assert!(serde_json::from_value::<ArtifactReference>(invalid.into()).is_err());
    }
    assert!(ArtifactReference::parse("docker.io/library/nginx:1.27").is_ok());
    assert!(ArtifactReference::parse("wasmer.io/baran/python@3.12.9+build.9").is_ok());
}

#[test]
fn digest_deserialization_rejects_traversal_and_noncanonical_hashes() {
    for invalid in [
        "sha256:../../outside",
        &format!("sha256:{}", "A".repeat(64)),
        "md5:abc",
    ] {
        assert!(serde_json::from_value::<Digest>(invalid.into()).is_err());
    }
    assert_eq!(
        Digest::of(b"abc").as_str(),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn corrupt_download_is_not_visible_and_does_not_replace_good_blob() {
    let root = tempfile::tempdir().unwrap();
    let store = ContentStore::open(root.path()).unwrap();
    let digest = Digest::of(b"verified");
    assert!(matches!(
        store.ingest(Cursor::new(b"corrupt"), &digest, 1024),
        Err(EngineError::DigestMismatch { .. })
    ));
    assert!(!store.path(&digest).exists());
    store
        .ingest(Cursor::new(b"verified"), &digest, 1024)
        .unwrap();
    assert!(
        store
            .ingest(Cursor::new(b"corrupt"), &digest, 1024)
            .is_err()
    );
    assert_eq!(store.read_verified(&digest, 1024).unwrap(), b"verified");
    assert_eq!(
        fs::read_dir(root.path().join("blobs/sha256"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn download_limits_work_without_content_length_and_cleanup_partial_files() {
    let root = tempfile::tempdir().unwrap();
    let store = ContentStore::open(root.path()).unwrap();
    let bytes = vec![0; 100_000];
    let digest = Digest::of(&bytes);
    assert!(store.ingest(Cursor::new(&bytes), &digest, 65_536).is_err());
    assert_eq!(
        fs::read_dir(root.path().join("blobs/sha256"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn cache_corruption_is_detected_and_can_be_repaired() {
    let root = tempfile::tempdir().unwrap();
    let store = ContentStore::open(root.path()).unwrap();
    let digest = Digest::of(b"valid");
    store.ingest(Cursor::new(b"valid"), &digest, 5).unwrap();
    fs::write(store.path(&digest), b"bad").unwrap();
    assert!(store.read_verified(&digest, 5).is_err());
    store.ingest(Cursor::new(b"valid"), &digest, 5).unwrap();
    assert_eq!(store.read_verified(&digest, 5).unwrap(), b"valid");
}

#[test]
fn default_recipe_is_local_loopback_without_internet_or_tty() {
    let spec: InstanceSpec = serde_json::from_value(recipe()).unwrap();
    spec.validate().unwrap();
    assert_eq!(spec.endpoint_id, "local");
    assert_eq!(spec.network, wasmdroid_engine::api::NetworkMode::Loopback);
    assert!(!spec.internet_access && !spec.tty && !spec.stdin_open);
}

#[test]
fn misspelled_options_are_not_silently_ignored() {
    let mut value = recipe();
    value["memory_max"] = 1024.into();
    assert!(serde_json::from_value::<InstanceSpec>(value).is_err());
}

#[test]
fn unsafe_mounts_and_invalid_ports_are_rejected() {
    for target in ["relative", "/data/../private"] {
        let mut value = recipe();
        value["mounts"] = serde_json::json!([{"source":{"type":"volume","source":"data"}, "target":target,"read_only":false}]);
        assert!(
            serde_json::from_value::<InstanceSpec>(value)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let mut value = recipe();
    value["ports"][0]["instance_port"] = 0.into();
    assert!(
        serde_json::from_value::<InstanceSpec>(value)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn catalog_never_claims_android_verification_without_evidence() {
    let mut value = catalog();
    Catalog::parse(&value.to_string()).unwrap();
    value["packages"][0]["android_verified"] = true.into();
    assert!(Catalog::parse(&value.to_string()).is_err());
}

#[test]
fn catalog_rejects_duplicates_unknown_versions_and_external_web_launches() {
    let mut duplicate = catalog();
    let item = duplicate["packages"][0].clone();
    duplicate["packages"].as_array_mut().unwrap().push(item);
    assert!(Catalog::parse(&duplicate.to_string()).is_err());
    let mut unknown = catalog();
    // Schemas1/2/3 are supported;4 remains an unknown feature contract.
    unknown["schema_version"] = 4.into();
    assert!(Catalog::parse(&unknown.to_string()).is_err());
    for path in [
        "//evil.example",
        "https://evil.example",
        "/\\evil.example",
        "/\n",
    ] {
        let mut value = catalog();
        value["packages"][0]["web"]["path"] = path.into();
        assert!(Catalog::parse(&value.to_string()).is_err());
    }
}

#[test]
fn empty_curated_catalog_is_valid_without_invented_verified_packages() {
    assert!(
        Catalog::parse(include_str!("../../catalog/catalog.v1.json"))
            .unwrap()
            .packages
            .is_empty()
    );
}

#[test]
fn catalog_downloads_require_well_formed_https_without_embedded_credentials() {
    for url in [
        "http://example.invalid/site.webc",
        "https://",
        "https://?path",
        "https://example.invalid/file#fragment",
        "https://user:example@example.invalid/file",
        "https://example.invalid/ file",
    ] {
        let mut value = catalog();
        value["packages"][0]["download_url"] = url.into();
        assert!(Catalog::parse(&value.to_string()).is_err(), "{url}");
    }
}

#[test]
fn dead_instances_cannot_restart_but_exited_instances_can() {
    use InstanceState::*;
    let state = Created
        .transition(Running)
        .unwrap()
        .transition(Exited)
        .unwrap()
        .transition(Running)
        .unwrap();
    assert_eq!(state, Running);
    assert!(Dead.transition(Running).is_err());
    assert!(Created.transition(Paused).is_err());
    assert!(Running.transition(Created).is_err());
}

#[test]
fn sqlite_migration_and_artifact_metadata_survive_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("engine.sqlite");
    let artifact = Artifact {
        reference: ArtifactReference::parse("wasmer.io/example/site@1.0.0").unwrap(),
        digest: Digest::of(b"test"),
        kind: ArtifactKind::Webc,
        driver: "wasmer-local".into(),
        platform: "wasm32-wasix".into(),
        commands: vec!["serve".into()],
        size_bytes: 4,
        labels: BTreeMap::new(),
    };
    StateDb::open(&path)
        .unwrap()
        .put_artifact(&artifact)
        .unwrap();
    let saved = StateDb::open(&path).unwrap().artifacts().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].digest, artifact.digest);
    assert_eq!(saved[0].reference, artifact.reference);
}

#[test]
fn newer_database_is_rejected_without_downgrade() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("engine.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 42).unwrap();
    assert!(matches!(
        StateDb::open(&path),
        Err(EngineError::Unsupported(_))
    ));
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        42
    );
}
