use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
use wasmdroid_engine::{
    EngineError,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec, InstanceState},
    session::EngineSession,
    state::{StateDb, VolumeRecord},
    store::Digest,
    volumes::{VolumeManager, VolumeOpenOptions},
};

fn artifact() -> Artifact {
    Artifact {
        reference: ArtifactReference::parse("wasmer.io/example/site@1.0.0").unwrap(),
        digest: Digest::of(b"fixture"),
        kind: ArtifactKind::Webc,
        driver: "wasmer-local".into(),
        platform: "wasm32-wasix".into(),
        commands: vec!["serve".into()],
        size_bytes: 7,
        labels: BTreeMap::new(),
    }
}
fn spec(volume: Option<&str>) -> InstanceSpec {
    serde_json::from_value(serde_json::json!({"artifact_ref":"wasmer.io/example/site@1.0.0","command":"serve","env":{"PRIVATE_VALUE":"do-not-log-this"},"mounts":volume.map(|name|vec![serde_json::json!({"source":{"type":"volume","source":name},"target":"/data","read_only":false})]).unwrap_or_default()})).unwrap()
}
fn create_options() -> VolumeOpenOptions {
    VolumeOpenOptions {
        read: true,
        write: true,
        create: true,
        truncate: true,
        ..Default::default()
    }
}
fn read_options() -> VolumeOpenOptions {
    VolumeOpenOptions {
        read: true,
        ..Default::default()
    }
}

#[test]
fn state_events_are_atomic_and_old_exit_cannot_overwrite_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = StateDb::open(temp.path().join("state.sqlite")).unwrap();
    db.put_artifact(&artifact()).unwrap();
    let created = db.create_instance("site", &spec(None)).unwrap();
    let running = db
        .transition_instance("site", created.version, InstanceState::Running, None, None)
        .unwrap();
    let exited = db
        .transition_instance(
            "site",
            running.version,
            InstanceState::Exited,
            Some(0),
            None,
        )
        .unwrap();
    let restarted = db
        .transition_instance("site", exited.version, InstanceState::Running, None, None)
        .unwrap();
    assert!(matches!(
        db.transition_instance(
            "site",
            running.version,
            InstanceState::Exited,
            Some(17),
            None
        ),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(db.instance("site").unwrap().version, restarted.version);
    let events = db.events_after(0, 100).unwrap();
    assert_eq!(events.len(), 4);
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("do-not-log-this")
    );
    assert!(
        db.transition_instance("site", restarted.version, InstanceState::Exited, None, None)
            .is_err()
    );
    assert_eq!(db.events_after(0, 100).unwrap().len(), 4);
    drop(db);
    let db = StateDb::open(temp.path().join("state.sqlite")).unwrap();
    assert_eq!(db.instance("site").unwrap().state, InstanceState::Running);
    assert_eq!(db.events_after(events[1].sequence, 2).unwrap().len(), 2);
}

#[test]
fn session_lock_prevents_false_recovery_and_unknown_exit_is_not_success() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    session.state().put_artifact(&artifact()).unwrap();
    let record = session
        .state()
        .create_instance("active", &spec(None))
        .unwrap();
    session
        .state()
        .transition_instance("active", record.version, InstanceState::Running, None, None)
        .unwrap();
    session
        .state()
        .create_instance("created", &spec(None))
        .unwrap();
    let owner = session.ownership();
    assert!(matches!(
        EngineSession::open(temp.path()),
        Err(EngineError::Conflict(_))
    ));
    drop(session);
    assert!(matches!(
        EngineSession::open(temp.path()),
        Err(EngineError::Conflict(_))
    ));
    drop(owner);
    let next = EngineSession::open(temp.path()).unwrap();
    assert_eq!(next.recovered_instances(), 1);
    let interrupted = next.state().instance("active").unwrap();
    assert_eq!(interrupted.state, InstanceState::Dead);
    assert_eq!(interrupted.exit_code, None);
    assert!(interrupted.last_error.is_some());
    assert_eq!(
        next.state().instance("created").unwrap().state,
        InstanceState::Created
    );
}

#[test]
fn missing_volume_rolls_back_instance_and_references_protect_stopped_data() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    session.state().put_artifact(&artifact()).unwrap();
    assert!(
        session
            .state()
            .create_instance("missing", &spec(Some("data")))
            .is_err()
    );
    assert!(session.state().instances("local").unwrap().is_empty());
    assert!(session.state().events_after(0, 100).unwrap().is_empty());
    let volumes = VolumeManager::open(&session).unwrap();
    volumes.create("data", BTreeMap::new()).unwrap();
    let record = session
        .state()
        .create_instance("site", &spec(Some("data")))
        .unwrap();
    assert!(matches!(
        volumes.remove("data"),
        Err(EngineError::Conflict(_))
    ));
    assert_eq!(
        session.state().volume_references("data").unwrap(),
        vec!["site"]
    );
    session
        .state()
        .remove_instance("site", record.version)
        .unwrap();
    assert!(volumes.inspect("data").is_ok());
    volumes.remove("data").unwrap();
    assert!(volumes.list().unwrap().is_empty());
}

#[test]
fn volume_bytes_survive_reopen_and_files_keep_the_engine_lease() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    volumes.create("data", BTreeMap::new()).unwrap();
    let directory = volumes.lease("data", false).unwrap();
    let mut file = directory.open_file("note.txt", create_options()).unwrap();
    file.write_all(b"persistent note").unwrap();
    file.sync_all().unwrap();
    drop(directory);
    assert!(matches!(
        volumes.remove("data"),
        Err(EngineError::Conflict(_))
    ));
    drop(volumes);
    drop(session);
    assert!(matches!(
        EngineSession::open(temp.path()),
        Err(EngineError::Conflict(_))
    ));
    drop(file);
    let session = EngineSession::open(temp.path()).unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    let mut file = volumes
        .lease("data", true)
        .unwrap()
        .open_file("note.txt", read_options())
        .unwrap();
    let mut value = String::new();
    file.read_to_string(&mut value).unwrap();
    assert_eq!(value, "persistent note");
}

#[test]
fn read_only_volume_denies_every_mutation_and_absolute_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    volumes.create("data", BTreeMap::new()).unwrap();
    let directory = volumes.lease("data", false).unwrap();
    directory
        .open_file("note", create_options())
        .unwrap()
        .write_all(b"safe")
        .unwrap();
    drop(directory);
    let read_only = volumes.lease("data", true).unwrap();
    for flags in [
        VolumeOpenOptions {
            write: true,
            ..Default::default()
        },
        VolumeOpenOptions {
            append: true,
            ..Default::default()
        },
        VolumeOpenOptions {
            truncate: true,
            ..Default::default()
        },
        VolumeOpenOptions {
            create: true,
            ..Default::default()
        },
        VolumeOpenOptions {
            create_new: true,
            ..Default::default()
        },
    ] {
        assert!(read_only.open_file("note", flags).is_err());
    }
    assert!(read_only.create_dir("new").is_err());
    assert!(read_only.remove_dir("new").is_err());
    assert!(read_only.remove_file("note").is_err());
    assert!(read_only.rename("note", "other").is_err());
    let mut file = read_only.open_file("note", read_options()).unwrap();
    assert!(file.write_all(b"bad").is_err());
    assert!(file.set_len(0).is_err());
    for path in ["../escape", "/absolute", "a/../../escape", "C:\\escape"] {
        assert!(read_only.open_file(path, read_options()).is_err());
    }
    assert_eq!(read_only.read_dir(".").unwrap().len(), 1);
}

#[test]
fn unregistered_data_and_unknown_trash_are_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    std::fs::create_dir_all(temp.path().join("volumes/data/orphan")).unwrap();
    std::fs::write(temp.path().join("volumes/data/orphan/note"), b"keep").unwrap();
    std::fs::create_dir_all(temp.path().join("volumes/trash/unknown")).unwrap();
    assert!(volumes.create("orphan", BTreeMap::new()).is_err());
    assert!(volumes.create("../outside", BTreeMap::new()).is_err());
    volumes.recover_removals().unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("volumes/data/orphan/note")).unwrap(),
        b"keep"
    );
    assert!(temp.path().join("volumes/trash/unknown").exists());
}

#[test]
fn interrupted_volume_delete_restores_before_commit_and_reclaims_after_commit() {
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    volumes.create("data", BTreeMap::new()).unwrap();
    volumes
        .lease("data", false)
        .unwrap()
        .open_file("note", create_options())
        .unwrap()
        .write_all(b"keep")
        .unwrap();
    let raw = rusqlite::Connection::open(temp.path().join("engine.sqlite")).unwrap();
    raw.execute(
        "INSERT INTO volume_deletions(name,requested_at_ms) VALUES ('data',1)",
        [],
    )
    .unwrap();
    std::fs::rename(
        temp.path().join("volumes/data/data"),
        temp.path().join("volumes/trash/data"),
    )
    .unwrap();
    volumes.recover_removals().unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("volumes/data/data/note")).unwrap(),
        b"keep"
    );
    raw.execute(
        "INSERT INTO volume_deletions(name,requested_at_ms) VALUES ('data',2)",
        [],
    )
    .unwrap();
    std::fs::rename(
        temp.path().join("volumes/data/data"),
        temp.path().join("volumes/trash/data"),
    )
    .unwrap();
    raw.execute("DELETE FROM volumes WHERE name='data'", [])
        .unwrap();
    volumes.recover_removals().unwrap();
    assert!(!temp.path().join("volumes/trash/data").exists());
    assert_eq!(
        raw.query_row("SELECT count(*) FROM volume_deletions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[cfg(unix)]
#[test]
fn symlink_cannot_escape_a_volume_to_host_or_another_volume() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temp.path()).unwrap();
    let volumes = VolumeManager::open(&session).unwrap();
    volumes.create("first", BTreeMap::new()).unwrap();
    volumes.create("second", BTreeMap::new()).unwrap();
    std::fs::write(temp.path().join("volumes/data/second/private"), b"isolated").unwrap();
    symlink("../second", temp.path().join("volumes/data/first/escape")).unwrap();
    let directory = volumes.lease("first", false).unwrap();
    assert!(
        directory
            .open_file("escape/private", read_options())
            .is_err()
    );
    assert!(directory.open_file("escape/new", create_options()).is_err());
    assert!(directory.remove_file("escape/private").is_err());
    assert!(directory.rename("escape/private", "stolen").is_err());
    drop(directory);
    volumes.remove("first").unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("volumes/data/second/private")).unwrap(),
        b"isolated"
    );
}

#[test]
fn migration_backfills_legacy_volume_references_and_rolls_back_bad_legacy_spec() {
    for valid in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.sqlite");
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute_batch(include_str!("../migrations/001_initial.sql"))
            .unwrap();
        let image = artifact();
        raw.execute(
            "INSERT INTO artifacts VALUES (?1,?2,'webc','wasmer-local',?3)",
            rusqlite::params![
                image.reference.as_str(),
                image.digest.as_str(),
                serde_json::to_string(&image).unwrap()
            ],
        )
        .unwrap();
        if valid {
            let volume = VolumeRecord {
                name: "data".into(),
                kind: "directory".into(),
                driver: "local-fs".into(),
                created_at_ms: 0,
                labels: BTreeMap::new(),
            };
            raw.execute(
                "INSERT INTO volumes VALUES ('data','directory','local-fs',?1)",
                [serde_json::to_string(&volume).unwrap()],
            )
            .unwrap();
        }
        raw.execute("INSERT INTO instances VALUES ('legacy','local',?1,'webc','wasmer-local',?2,'created',NULL)",rusqlite::params![image.reference.as_str(),serde_json::to_string(&spec(Some("data"))).unwrap()]).unwrap();
        if valid {
            assert_eq!(
                StateDb::open(&path)
                    .unwrap()
                    .volume_references("data")
                    .unwrap(),
                vec!["legacy"]
            );
        } else {
            assert!(StateDb::open(&path).is_err());
            assert_eq!(
                raw.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                raw.query_row(
                    "SELECT count(*) FROM pragma_table_info('instances') WHERE name='version'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
        }
    }
}
