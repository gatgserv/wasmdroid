//! Definition/storage regressions only. No guest, host shell or registry download.
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
use wasmdroid_engine::{
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceState},
    session::EngineSession,
    stacks::{self, STACK_LABEL, StackManager},
    store::Digest,
    volumes::{VolumeManager, VolumeOpenOptions},
};

fn fixture(session: &EngineSession) {
    session
        .state()
        .put_artifact(&Artifact {
            reference: ArtifactReference::parse("wasmer.io/example/stack-fixture@1.0.0").unwrap(),
            digest: Digest::of(b"metadata fixture, not executable"),
            kind: ArtifactKind::Webc,
            driver: "wasmer-local".into(),
            platform: "wasm32-wasix".into(),
            commands: vec!["run".into(), "serve".into()],
            size_bytes: 0,
            labels: BTreeMap::from([("dev.wasmdroid.entrypoint".into(), "serve".into())]),
        })
        .unwrap();
}
const VALID: &str = r#"
services:
  db:
    image: wasmer.io/example/stack-fixture@1.0.0
    command: [run, '--data', /data]
    volumes: ['data:/data']
    environment: {PORT: 5432}
  web:
    image: wasmer.io/example/stack-fixture@1.0.0
    command: 'serve --label "hello world"'
    depends_on: {db: {condition: service_started, required: true, restart: false}}
    environment: ['PRIVATE_VALUE=marker-not-for-events', 'DOLLAR=$$HOME']
    ports:
      - '127.0.0.1:8080:80/tcp'
      - {target: 81, published: 0, protocol: tcp}
    volumes:
      - {type: volume, source: data, target: /data, read_only: true}
volumes: {data: {}}
"#;

#[test]
fn normalizes_without_side_effects_and_publishes_membership_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session);
    let manager = StackManager::new(&session);
    let plan = manager.preview("demo", VALID).unwrap();
    assert_eq!(plan.startup_order, vec!["db", "web"]);
    assert!(session.state().instances("local").unwrap().is_empty());
    assert!(session.state().volumes().unwrap().is_empty());
    assert_eq!(plan.services["db"].spec.env["PORT"], "5432");
    assert_eq!(plan.services["web"].spec.env["DOLLAR"], "$HOME");
    assert_eq!(
        plan.services["web"].spec.args,
        vec!["--label", "hello world"]
    );
    assert_eq!(plan.services["web"].spec.ports[0].host_port, 8080);
    assert!(plan.services["web"].spec.mounts[0].read_only);
    let created = manager.create_plan(&plan).unwrap();
    assert_eq!(created.instances.len(), 2);
    assert!(
        created
            .instances
            .iter()
            .all(|instance| instance.state == InstanceState::Created)
    );
    assert_eq!(
        session
            .state()
            .stack_for_instance(&plan.services["db"].instance_id)
            .unwrap()
            .as_deref(),
        Some("demo")
    );
    assert_eq!(session.state().stack_members("demo").unwrap().len(), 2);
    let summaries =
        serde_json::to_string(&session.state().stack_summaries(None, 10).unwrap()).unwrap();
    assert!(!summaries.contains("PRIVATE_VALUE"));
    assert!(!summaries.contains("marker-not-for-events"));
    let summary = session.state().stack_summary("demo").unwrap();
    assert_eq!(summary["version"], created.version);
    assert_eq!(summary["service_count"], 2);
    assert!(!summary.to_string().contains("PRIVATE_VALUE"));
    assert!(session.state().stack_summary("missing").is_err());
    let events = serde_json::to_string(&session.state().events_after(0, 100).unwrap()).unwrap();
    assert!(!events.contains("marker-not-for-events"));
    assert!(!events.contains("$HOME"));
    assert!(
        session
            .state()
            .remove_instance(&plan.services["db"].instance_id, 0)
            .is_err()
    );
    assert!(
        VolumeManager::open(&session)
            .unwrap()
            .remove(&plan.volumes["data"].name)
            .is_err()
    );
    assert!(
        session
            .state()
            .remove_stack("demo", created.version + 1)
            .is_err()
    );
    session
        .state()
        .remove_stack("demo", created.version)
        .unwrap();
    assert!(session.state().instances("local").unwrap().is_empty());
    assert_eq!(session.state().volumes().unwrap().len(), 1);
    let replacement = manager.create_plan(&plan).unwrap();
    assert!(replacement.version > created.version);
    assert!(
        session
            .state()
            .remove_stack("demo", created.version)
            .is_err()
    );
    session
        .state()
        .remove_stack("demo", replacement.version)
        .unwrap();
}

#[test]
fn reviewed_plan_rejects_replaced_installed_artifact_before_preparing_data() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session);
    let manager = StackManager::new(&session);
    let reviewed = manager.preview("reviewed", VALID).unwrap();
    let mut changed = session
        .state()
        .artifact(&reviewed.services["db"].spec.artifact_ref)
        .unwrap();
    changed.digest = Digest::of(b"different installed artifact after preview");
    assert!(session.state().put_artifact(&changed).is_err());
    assert_eq!(
        session.state().artifact(&changed.reference).unwrap().digest,
        reviewed.services["db"].digest
    );
    // An unreferenced image can be explicitly removed/reinstalled. The reviewed
    // plan must still reject the new bytes before preparing any datasets.
    session.state().remove_artifact(&changed.reference).unwrap();
    session.state().put_artifact(&changed).unwrap();
    assert!(manager.create_plan(&reviewed).is_err());
    assert!(session.state().instances("local").unwrap().is_empty());
    assert!(session.state().volumes().unwrap().is_empty());
    assert!(session.state().stack_summary("reviewed").is_err());
}

#[test]
fn rejects_authority_cycles_duplicates_and_host_expansion_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session);
    let invalid = [
        VALID.replace("condition: service_started", "condition: service_healthy"),
        VALID.replace("command: [run, '--data', /data]", "build: ."),
        VALID.replace("volumes: ['data:/data']", "volumes: ['/host:/data']"),
        VALID.replace("DOLLAR=$$HOME", "DOLLAR=$HOME"),
        VALID.replace("environment: {PORT: 5432}", "environment: [INHERIT_HOST]"),
        VALID.replace("protocol: tcp", "protocol: udp"),
        VALID.replace("127.0.0.1:8080:80/tcp", "0.0.0.0:8080:80/tcp"),
        VALID.replace("  db:\n", "  db:\n    depends_on: [web]\n"),
        VALID.replace(
            "condition: service_started",
            "condition: service_started, unknown: ignored",
        ),
        VALID.replace(
            "environment: {PORT: 5432}",
            "environment: {PORT: 5432, PORT: 9999}",
        ),
        VALID.replace("  db:\n", "  db: &node\n"),
        VALID.replace(
            "environment: {PORT: 5432}",
            "environment: {PORT: !!str 5432}",
        ),
        format!("{VALID}\n---\nservices: {{}}\n"),
    ];
    for document in invalid {
        assert!(
            StackManager::new(&session)
                .create("bad", &document)
                .is_err()
        );
        assert!(session.state().instances("local").unwrap().is_empty());
        assert!(session.state().volumes().unwrap().is_empty());
        assert!(
            session
                .state()
                .stack_summaries(None, 10)
                .unwrap()
                .is_empty()
        );
    }
    let deep = format!("services: {}0{}", "[".repeat(24), "]".repeat(24));
    assert!(stacks::plan("bad", &deep, session.state()).is_err());
    let malformed = "services: {command: \"PRIVATE_MARKER_MUST_NOT_ECHO";
    let error = stacks::plan("bad", malformed, session.state())
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("PRIVATE_MARKER"));
}

#[test]
fn sql_failure_keeps_all_members_out_and_preserves_prepared_data_for_retry() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session);
    let plan = stacks::plan("retry", VALID, session.state()).unwrap();
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute_batch(&format!("CREATE TRIGGER fail_stack_member BEFORE INSERT ON instances WHEN NEW.id='{}' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;",plan.services["web"].instance_id)).unwrap();
    assert!(StackManager::new(&session).create_plan(&plan).is_err());
    assert!(session.state().instances("local").unwrap().is_empty());
    assert!(
        session
            .state()
            .stack_summaries(None, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        !session
            .state()
            .events_after(0, 100)
            .unwrap()
            .iter()
            .any(|event| event.payload["type"] == "instance_created")
    );
    let volumes = VolumeManager::open(&session).unwrap();
    let dataset = volumes.exclusive(&plan.volumes["data"].name).unwrap();
    let mut file = dataset
        .open_file(
            "sentinel",
            VolumeOpenOptions {
                write: true,
                create: true,
                ..Default::default()
            },
        )
        .unwrap();
    file.write_all(b"preserve").unwrap();
    drop(file);
    drop(dataset);
    raw.execute_batch("DROP TRIGGER fail_stack_member").unwrap();
    let created = StackManager::new(&session).create_plan(&plan).unwrap();
    session
        .state()
        .remove_stack("retry", created.version)
        .unwrap();
    let dataset = volumes.lease(&plan.volumes["data"].name, true).unwrap();
    let mut bytes = Vec::new();
    dataset
        .open_file(
            "sentinel",
            VolumeOpenOptions {
                read: true,
                ..Default::default()
            },
        )
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"preserve");
}

#[test]
fn reopen_retains_membership_and_unused_declared_volume_references() {
    let temp = tempfile::tempdir().unwrap();
    let plan;
    {
        let session = EngineSession::open(temp.path()).unwrap();
        fixture(&session);
        let document = VALID.replace("volumes: {data: {}}", "volumes: {data: {}, unused: {}}");
        plan = StackManager::new(&session)
            .create("persist", &document)
            .unwrap()
            .plan;
        assert!(
            VolumeManager::open(&session)
                .unwrap()
                .remove(&plan.volumes["unused"].name)
                .is_err()
        );
        let mut forged = plan.services["db"].spec.clone();
        forged.labels.insert(STACK_LABEL.into(), "fake".into());
        assert!(session.state().create_instance("forged", &forged).is_err());
    }
    let session = EngineSession::open(temp.path()).unwrap();
    assert_eq!(session.state().stack("persist").unwrap().instances.len(), 2);
    let member = &plan.services["db"].instance_id;
    session.state().start_instance(member, 0, &[]).unwrap(); // Metadata guard only, no guest execution.
    let version = session.state().stack("persist").unwrap().version;
    assert!(session.state().remove_stack("persist", version).is_err());
}

#[test]
fn default_entrypoint_ranges_external_data_and_explicit_argv_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    fixture(&session);
    VolumeManager::open(&session)
        .unwrap()
        .create("chosen_data", BTreeMap::new())
        .unwrap();
    let document = r#"
services:
  site:
    image: wasmer.io/example/stack-fixture@1.0.0
    ports: ['9000-9002:80-82']
    environment: {TEXT: 'false', ZERO: 0}
    volumes: ['chosen:/data:ro']
volumes:
  chosen: {external: true, name: chosen_data}
"#;
    let plan = stacks::plan("external", document, session.state()).unwrap();
    assert_eq!(plan.services["site"].spec.command, "serve");
    assert_eq!(
        plan.services["site"]
            .spec
            .ports
            .iter()
            .map(|port| port.host_port)
            .collect::<Vec<_>>(),
        vec![9000, 9001, 9002]
    );
    assert_eq!(plan.volumes["chosen"].name, "chosen_data");
    assert!(plan.volumes["chosen"].external);
    assert_eq!(plan.services["site"].spec.env["ZERO"], "0");
    assert!(
        stacks::plan(
            "bad",
            &document.replace("TEXT: 'false'", "TEXT: false"),
            session.state()
        )
        .is_err()
    );
    assert!(
        stacks::plan(
            "bad",
            &document.replace("'9000-9002:80-82'", "'9000-9001:80-82'"),
            session.state()
        )
        .is_err()
    );
    let mut tampered = plan.clone();
    tampered
        .services
        .get_mut("site")
        .unwrap()
        .spec
        .internet_access = true;
    assert!(StackManager::new(&session).create_plan(&tampered).is_err());
    let mut changed = plan.clone();
    changed.services.get_mut("site").unwrap().digest = Digest::of(b"changed");
    assert!(StackManager::new(&session).create_plan(&changed).is_err());
    assert!(session.state().instances("local").unwrap().is_empty());
    let created = StackManager::new(&session).create_plan(&plan).unwrap();
    session
        .state()
        .remove_stack("external", created.version)
        .unwrap();
    assert!(
        VolumeManager::open(&session)
            .unwrap()
            .inspect("chosen_data")
            .is_ok()
    );
}
