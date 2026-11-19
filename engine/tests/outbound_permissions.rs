//! Neutral permission contracts only; no positive runtime network authority.
use wasmdroid_engine::api::{InstanceSpec, NetworkMode};

fn legacy() -> InstanceSpec {
    serde_json::from_value(serde_json::json!({
        "artifact_ref":"wasmer.io/fixture/network@1.0.0", "command":"run"
    }))
    .unwrap()
}

#[test]
fn absent_and_explicit_false_lan_preserve_exact_legacy_serialization() {
    let absent = legacy();
    assert!(!absent.lan_access);
    assert!(!absent.internet_access);
    let absent_json = serde_json::to_string(&absent).unwrap();
    // Canonical pre-field serialization: old signed metadata must stay byte-for-byte identical.
    assert_eq!(
        absent_json,
        r#"{"artifact_ref":"wasmer.io/fixture/network@1.0.0","endpoint_id":"local","command":"run","args":[],"env":{},"working_dir":null,"mounts":[],"ports":[],"network":"loopback","internet_access":false,"resources":{"memory_max_bytes":null,"cpu_shares":null,"timeout_seconds":null},"restart_policy":"no","start_on_boot":false,"labels":{},"tty":false,"stdin_open":false}"#
    );
    assert!(!absent_json.contains("lan_access"));
    let mut value = serde_json::to_value(&absent).unwrap();
    value["lan_access"] = false.into();
    let explicit: InstanceSpec = serde_json::from_value(value).unwrap();
    assert_eq!(serde_json::to_string(&explicit).unwrap(), absent_json);
    let mut previous: serde_json::Value = serde_json::from_str(&absent_json).unwrap();
    assert!(
        previous
            .as_object_mut()
            .unwrap()
            .remove("lan_access")
            .is_none()
    );
    assert_eq!(serde_json::to_value(explicit).unwrap(), previous);
}

#[test]
fn internet_and_lan_are_independent_explicit_boolean_permissions() {
    for internet in [false, true] {
        for lan in [false, true] {
            let mut spec = legacy();
            spec.internet_access = internet;
            spec.lan_access = lan;
            spec.validate().unwrap();
            let wire = serde_json::to_value(&spec).unwrap();
            assert_eq!(
                wire.get("lan_access").and_then(|value| value.as_bool()),
                lan.then_some(true)
            );
            let decoded: InstanceSpec = serde_json::from_value(wire).unwrap();
            assert_eq!(
                (decoded.internet_access, decoded.lan_access),
                (internet, lan)
            );
            assert_eq!(decoded.network, NetworkMode::Loopback);
        }
    }
}

#[test]
fn publication_mode_and_labels_never_imply_outbound_permission() {
    for mode in [NetworkMode::None, NetworkMode::Loopback, NetworkMode::Lan] {
        let mut spec = legacy();
        spec.network = mode;
        spec.labels.insert("lan_access".into(), "true".into());
        spec.labels.insert("internet_access".into(), "true".into());
        let decoded: InstanceSpec =
            serde_json::from_value(serde_json::to_value(spec).unwrap()).unwrap();
        assert!(!decoded.internet_access && !decoded.lan_access);
        assert_eq!(decoded.network, mode);
    }
}

#[test]
fn lan_permission_rejects_coercion_null_and_misspelled_authority() {
    for malformed in [
        serde_json::json!(null),
        serde_json::json!("true"),
        serde_json::json!(1),
        serde_json::json!({}),
    ] {
        let mut value = serde_json::to_value(legacy()).unwrap();
        value["lan_access"] = malformed;
        assert!(serde_json::from_value::<InstanceSpec>(value).is_err());
    }
    let mut typo = serde_json::to_value(legacy()).unwrap();
    typo["outbound_lan"] = true.into();
    assert!(serde_json::from_value::<InstanceSpec>(typo).is_err());
}

fn install_metadata(session: &wasmdroid_engine::session::EngineSession) {
    session
        .state()
        .put_artifact(&wasmdroid_engine::api::Artifact {
            reference: legacy().artifact_ref,
            digest: wasmdroid_engine::store::Digest::of(b"metadata fixture"),
            kind: wasmdroid_engine::api::ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: Default::default(),
        })
        .unwrap();
}

#[test]
fn versioned_configuration_preserves_legacy_noop_then_independently_grants_and_withdraws_lan() {
    let temporary = tempfile::tempdir().unwrap();
    let session = wasmdroid_engine::session::EngineSession::open(temporary.path()).unwrap();
    install_metadata(&session);
    let created = session.state().create_instance("app", &legacy()).unwrap();
    let unchanged = session
        .state()
        .configure_instance("app", created.version, &legacy())
        .unwrap();
    assert_eq!(unchanged.version, created.version);
    assert!(!unchanged.spec.lan_access);
    let mut grant = unchanged.spec;
    grant.lan_access = true;
    let granted = session
        .state()
        .configure_instance("app", unchanged.version, &grant)
        .unwrap();
    assert!(granted.spec.lan_access);
    assert!(!granted.spec.internet_access);
    assert_eq!(granted.version, created.version + 1);
    assert!(
        session
            .state()
            .configure_instance("app", created.version, &legacy())
            .is_err()
    );
    assert!(session.state().instance("app").unwrap().spec.lan_access);
    grant.lan_access = false;
    let withdrawn = session
        .state()
        .configure_instance("app", granted.version, &grant)
        .unwrap();
    assert!(!withdrawn.spec.lan_access && !withdrawn.spec.internet_access);
    assert!(
        serde_json::to_value(withdrawn.spec)
            .unwrap()
            .get("lan_access")
            .is_none()
    );
}

#[test]
fn managed_plan_cannot_smuggle_outbound_lan_authority() {
    let temporary = tempfile::tempdir().unwrap();
    let session = wasmdroid_engine::session::EngineSession::open(temporary.path()).unwrap();
    install_metadata(&session);
    let manager = wasmdroid_engine::stacks::StackManager::new(&session);
    let mut plan = manager
        .preview(
            "group",
            r#"{"services":{"app":{"image":"wasmer.io/fixture/network@1.0.0"}}}"#,
        )
        .unwrap();
    plan.services.get_mut("app").unwrap().spec.lan_access = true;
    assert!(matches!(
        manager.create_plan(&plan),
        Err(wasmdroid_engine::EngineError::Unsupported(_))
    ));
    assert!(session.state().instances("local").unwrap().is_empty());
}

fn requested_entry() -> wasmdroid_engine::catalog::CatalogPackage {
    wasmdroid_engine::catalog::Catalog::parse(&serde_json::json!({"schema_version":2,"packages":[{
        "id":"fixture","name":"Fixture","description":"Contract fixture","publisher":"Fixture",
        "digest":wasmdroid_engine::store::Digest::of(b"fixture"),"download_url":"https://example.com/fixture.webc",
        "android_verified":true,"evidence":"fixture-only-not-device-evidence",
        "recipe":{"artifact_ref":"wasmer.io/fixture/network@1.0.0","command":"run","internet_access":true,"lan_access":true,
            "env":{"PRIVATE_FIXTURE":"unchanged"}},"web":null
    }]}).to_string()).unwrap().packages.remove(0)
}

#[test]
fn catalog_requests_survive_while_normal_effective_spec_grants_neither_permission() {
    let requested = requested_entry();
    let before = serde_json::to_vec(&requested).unwrap();
    let effective = wasmdroid_engine::catalog_install::effective_single_spec(&requested).unwrap();
    assert!(!effective.internet_access && !effective.lan_access);
    let original = requested.single_recipe().unwrap();
    assert!(original.internet_access && original.lan_access);
    assert_eq!(effective.env, original.env);
    assert_eq!(effective.artifact_ref, original.artifact_ref);
    assert_eq!(serde_json::to_vec(&requested).unwrap(), before);
}

#[test]
fn normal_effective_spec_preserves_existing_evidence_and_spec_constraints() {
    let mut unverified = requested_entry();
    unverified.android_verified = false;
    unverified.evidence = None;
    assert!(matches!(
        wasmdroid_engine::catalog_install::effective_single_spec(&unverified),
        Err(wasmdroid_engine::EngineError::Unsupported(_))
    ));
    let mut invalid = requested_entry();
    let wasmdroid_engine::catalog::CatalogRecipe::Single(spec) = &mut invalid.recipe else {
        panic!("single fixture")
    };
    spec.resources.timeout_seconds = Some(0);
    assert!(wasmdroid_engine::catalog_install::effective_single_spec(&invalid).is_err());
}
