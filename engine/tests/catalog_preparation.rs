use std::collections::BTreeMap;
use wasmdroid_engine::{
    EngineError,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceState},
    catalog::{Catalog, CatalogPackage},
    catalog_install::prepare_stack,
    session::EngineSession,
    store::Digest,
};

fn fixture(session: &EngineSession) -> CatalogPackage {
    let mut artifacts = Vec::new();
    for name in ["database", "web"] {
        let artifact = Artifact {
            reference: ArtifactReference::parse(format!("wasmer.io/fixture/{name}@1.0.0")).unwrap(),
            digest: Digest::of(name.as_bytes()),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into()],
            size_bytes: 0,
            labels: BTreeMap::new(),
        };
        artifacts.push(
            serde_json::json!({"reference":artifact.reference,"digest":artifact.digest,
            "download_url":format!("https://example.com/{name}.webc")}),
        );
        session.state().put_artifact(&artifact).unwrap();
    }
    let document = "services:\n  database:\n    image: wasmer.io/fixture/database@1.0.0\n    command: [run]\n    volumes: [data:/data]\n  web:\n    image: wasmer.io/fixture/web@1.0.0\n    command: [run]\n    depends_on: [database]\n    ports: ['8080']\nvolumes: {data: {}}\n";
    let entry = serde_json::json!({"id":"fixture-app","name":"Fixture application","description":"Metadata fixture only",
        "publisher":"Fixture","digest":Digest::of(b"web"),"download_url":"https://example.com/web.webc",
        "android_verified":true,"evidence":"fixture-only-not-production-evidence",
        "recipe":{"type":"stack","document":document,"artifacts":artifacts,"primary_service":"web"},
        "web":{"instance_port":8080,"path":"/"}});
    Catalog::parse(&serde_json::json!({"schema_version":2,"packages":[entry]}).to_string())
        .unwrap()
        .packages
        .remove(0)
}

// Explicit fault injection in a uniquely owned disposable database. Normal
// StateDb writers correctly prohibit replacing an immutable version digest.
fn inject_artifact_corruption(
    session: &EngineSession,
    artifact: &Artifact,
) -> wasmdroid_engine::Result<()> {
    let raw = rusqlite::Connection::open(session.root().join("engine.sqlite"))?;
    assert_eq!(
        raw.execute(
            "UPDATE artifacts SET digest=?1,metadata_json=?2 WHERE reference=?3",
            rusqlite::params![
                artifact.digest.as_str(),
                serde_json::to_string(artifact)?,
                artifact.reference.as_str()
            ]
        )?,
        1
    );
    Ok(())
}

#[test]
fn catalog_group_uses_managed_membership_private_data_and_actual_primary_labels_without_starting() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let package = fixture(&session);
    let mut checked = 0;
    let first = prepare_stack(&session, "app-first", &package, &mut |_| {
        checked += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(checked, 2);
    assert_eq!(first.instances.len(), 2);
    for member in &first.instances {
        assert_eq!(member.state, InstanceState::Created);
        assert_eq!(
            session
                .state()
                .stack_for_instance(&member.id)
                .unwrap()
                .as_deref(),
            Some("app-first")
        );
    }
    let web = &first.plan.services["web"].spec;
    assert_eq!(web.labels["dev.wasmdroid.ui.name"], "Fixture application");
    assert_eq!(web.labels["dev.wasmdroid.catalog.primary"], "true");
    assert_eq!(web.labels["dev.wasmdroid.web.port"], "8080");
    assert!(
        !first.plan.services["database"]
            .spec
            .labels
            .contains_key("dev.wasmdroid.web.port")
    );
    let manager = wasmdroid_engine::stacks::StackManager::new(&session);
    let source = manager.edit_source("app-first").unwrap();
    let review = manager
        .preview_update("app-first", first.version, &source.document)
        .unwrap();
    assert_eq!(manager.update_plan(&review).unwrap().version, first.version);
    assert_eq!(review.plan.services["web"].spec.labels, web.labels);
    let volume = &first.plan.volumes["data"].name;
    let sentinel = temp
        .path()
        .join("volumes/data")
        .join(volume)
        .join("retained");
    std::fs::write(&sentinel, b"application data").unwrap();
    let second = prepare_stack(&session, "app-second", &package, &mut |_| Ok(())).unwrap();
    assert_ne!(second.plan.volumes["data"].name, *volume);
    assert_eq!(std::fs::read(sentinel).unwrap(), b"application data");
    assert!(prepare_stack(&session, "app-first", &package, &mut |_| Ok(())).is_err());
}

#[test]
fn verified_schema_three_media_group_reaches_actual_managed_preparation() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let mut package = fixture(&session);
    // Generated first-party transparent pixel: framing fixture, not a screenshot
    // or actual Android evidence. Both icon and screenshot slots are required.
    let image = serde_json::json!({"format":"png","size_bytes":68,
        "digest":"sha256:43739c566e26fd7cb88f69d3864ea34740372f5ee99acac169e090beffbce5c6",
        "width":1,"height":1,"data_base64":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=",
        "alt":"Synthetic test pixel","provenance":{"license":"LicenseRef-TestFixture","attribution":"First-party contract fixture","report":"docs/verification/media-fixture-only.md"}});
    package.media = Some(
        serde_json::from_value(serde_json::json!({"icon":image,"screenshots":[image]})).unwrap(),
    );
    let mut checked = 0;
    let record = prepare_stack(&session, "media-app", &package, &mut |_| {
        checked += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(checked, 2);
    assert_eq!(record.instances.len(), 2);
    assert_eq!(session.state().stack_members("media-app").unwrap().len(), 2);
    assert_eq!(session.state().volumes().unwrap().len(), 1);
    assert!(
        record
            .instances
            .iter()
            .all(|member| member.state == InstanceState::Created)
    );
    assert!(package.media.is_some());
}

#[test]
fn changed_installed_digest_and_runtime_denial_fail_before_dataset_or_membership_creation() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let package = fixture(&session);
    let failure = prepare_stack(&session, "app-denied", &package, &mut |_| {
        Err(EngineError::Unsupported("fixture runtime refusal".into()))
    });
    assert!(failure.is_err());
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(session.state().stack("app-denied").is_err());
    let mut changed = session
        .state()
        .artifact(&ArtifactReference::parse("wasmer.io/fixture/web@1.0.0").unwrap())
        .unwrap();
    changed.digest = Digest::of(b"changed");
    assert!(session.state().put_artifact(&changed).is_err());
    inject_artifact_corruption(&session, &changed).unwrap();
    assert!(prepare_stack(&session, "app-changed", &package, &mut |_| Ok(())).is_err());
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(session.state().stack("app-changed").is_err());
}

#[test]
fn failed_group_publication_retains_prepared_data_and_retry_does_not_overwrite_it() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let package = fixture(&session);
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("CREATE TRIGGER refuse_catalog BEFORE INSERT ON stack_services BEGIN SELECT RAISE(ABORT,'fixture metadata refusal'); END;").unwrap();
    assert!(prepare_stack(&session, "app-retry", &package, &mut |_| Ok(())).is_err());
    assert!(session.state().stack("app-retry").is_err());
    let volumes = session.state().volumes().unwrap();
    assert_eq!(volumes.len(), 1);
    let file = temp
        .path()
        .join("volumes/data")
        .join(&volumes[0].name)
        .join("kept");
    std::fs::write(&file, b"retained after failure").unwrap();
    raw.execute_batch("DROP TRIGGER refuse_catalog").unwrap();
    let retried = prepare_stack(&session, "app-retry", &package, &mut |_| Ok(())).unwrap();
    assert_eq!(retried.instances.len(), 2);
    assert_eq!(session.state().volumes().unwrap().len(), 1);
    assert_eq!(std::fs::read(file).unwrap(), b"retained after failure");
}

#[test]
fn actual_worker_digest_and_later_publication_recheck_cannot_follow_an_unreviewed_root() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let package = fixture(&session);
    let created = prepare_stack(&session, "app-worker", &package, &mut |_| Ok(())).unwrap();
    let spec = &created.plan.services["web"].spec;
    let mut actual = session.state().artifact(&spec.artifact_ref).unwrap();
    wasmdroid_engine::stacks::validate_worker_artifact_pin(spec, &actual).unwrap();
    actual.digest = Digest::of(b"unreviewed substituted root");
    assert!(wasmdroid_engine::stacks::validate_worker_artifact_pin(spec, &actual).is_err());
    assert!(session.state().put_artifact(&actual).is_err());
    // Even fault-injected persisted metadata between planning and final
    // admission must not prepare data or publish a substituted group.
    let initial_volumes = session.state().volumes().unwrap().len();
    let mut changed = false;
    assert!(
        prepare_stack(&session, "app-race", &package, &mut |_| {
            if !changed {
                inject_artifact_corruption(&session, &actual)?;
                changed = true;
            }
            Ok(())
        })
        .is_err()
    );
    assert!(changed);
    assert_eq!(session.state().volumes().unwrap().len(), initial_volumes);
    assert!(session.state().stack("app-race").is_err());
}

#[test]
fn catalog_seed_retry_keeps_completed_files_and_refuses_edited_or_previously_published_data() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let mut package = fixture(&session);
    if let wasmdroid_engine::catalog::CatalogRecipe::Stack(recipe) = &mut package.recipe {
        recipe
            .assets
            .push(wasmdroid_engine::catalog_assets::CatalogSeedFile {
                volume: "data".into(),
                path: "index.html".into(),
                text: "seed content".into(),
                size_bytes: 12,
                digest: Digest::of(b"seed content"),
            });
    }
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute_batch("CREATE TRIGGER refuse_seed BEFORE INSERT ON stack_services BEGIN SELECT RAISE(ABORT,'fixture publication refusal'); END;").unwrap();
    assert!(prepare_stack(&session, "app-seed", &package, &mut |_| Ok(())).is_err());
    let volumes = session.state().volumes().unwrap();
    assert_eq!(volumes.len(), 1);
    let path = temp
        .path()
        .join("volumes/data")
        .join(&volumes[0].name)
        .join("index.html");
    assert_eq!(std::fs::read(&path).unwrap(), b"seed content");
    std::fs::write(&path, b"user changed content").unwrap();
    raw.execute_batch("DROP TRIGGER refuse_seed").unwrap();
    assert!(prepare_stack(&session, "app-seed", &package, &mut |_| Ok(())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"user changed content");
    assert!(session.state().stack("app-seed").is_err());
    // Simulate an explicit disposable-fixture recovery, never production cleanup.
    std::fs::write(&path, b"seed content").unwrap();
    let committed = prepare_stack(&session, "app-seed", &package, &mut |_| Ok(())).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"seed content");
    session
        .state()
        .remove_stack("app-seed", committed.version)
        .unwrap();
    assert!(prepare_stack(&session, "app-seed", &package, &mut |_| Ok(())).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"seed content");
}
