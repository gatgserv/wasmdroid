//! Independent filesystem observations and crafted archives. These are data-plane
//! regressions, not evidence of Android SAF or guest execution.
use std::{
    collections::BTreeMap,
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use wasmdroid_engine::{
    EngineError,
    api::{Artifact, ArtifactKind, ArtifactReference, InstanceSpec},
    backup::{self, RestoreInput},
    data::DataImport,
    session::EngineSession,
    store::{ContentStore, Digest},
    volumes::{VolumeManager, VolumeOpenOptions},
};
fn control() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn create_data(session: &EngineSession, name: &str, path: &str, bytes: &[u8]) {
    let manager = VolumeManager::open(session).unwrap();
    manager.create(name, BTreeMap::new()).unwrap();
    let directory = manager.exclusive(name).unwrap();
    wasmdroid_engine::data::ensure_parent(&directory, std::path::Path::new(path)).unwrap();
    let mut file = directory
        .open_file(
            path,
            VolumeOpenOptions {
                write: true,
                create: true,
                truncate: true,
                ..Default::default()
            },
        )
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn stream_bytes(mut reader: wasmdroid_engine::data::DataReader) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.read(65536).unwrap();
        if chunk.is_empty() {
            break;
        }
        bytes.extend(chunk);
    }
    bytes
}
fn restore(
    session: &EngineSession,
    name: &str,
    bytes: &[u8],
    budget: u64,
) -> wasmdroid_engine::Result<backup::RestoreResult> {
    let mut transfer = RestoreInput::begin(session, name, 1024 * 1024, budget, control())?;
    for chunk in bytes.chunks(64 * 1024) {
        transfer.append(chunk)?;
    }
    transfer.commit()
}
#[test]
fn import_is_atomic_cancellable_and_keeps_ownership_until_drop() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    create_data(&session, "saved", "note", b"old content");
    let cancel = control();
    let mut transfer = DataImport::begin(
        &session,
        "saved",
        "note",
        true,
        Some(3),
        1024,
        cancel.clone(),
    )
    .unwrap();
    transfer.append(b"new").unwrap();
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/saved/note")).unwrap(),
        b"old content"
    );
    assert!(matches!(
        VolumeManager::open(&session).unwrap().lease("saved", true),
        Err(EngineError::Conflict(_))
    ));
    drop(session);
    assert!(matches!(
        EngineSession::open(root.path()),
        Err(EngineError::Conflict(_))
    ));
    cancel.store(true, Ordering::Release);
    assert!(transfer.commit().is_err());
    drop(transfer);
    let session = EngineSession::open(root.path()).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/saved/note")).unwrap(),
        b"old content"
    );
    let mut transfer =
        DataImport::begin(&session, "saved", "note", true, Some(3), 1024, control()).unwrap();
    transfer.append(b"new").unwrap();
    assert_eq!(transfer.commit().unwrap(), 3);
    drop(transfer);
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/saved/note")).unwrap(),
        b"new"
    );
    assert!(DataImport::begin(&session, "saved", "note", false, None, 1024, control()).is_err());
}
#[test]
fn backup_roundtrip_preserves_binary_long_names_empty_folders_and_original() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let name = format!("nested/deeper/{}-notă.bin", "x".repeat(120));
    let payload = (0..128 * 1024).map(|n| (n % 251) as u8).collect::<Vec<_>>();
    create_data(&session, "original", &name, &payload);
    let manager = VolumeManager::open(&session).unwrap();
    manager
        .exclusive("original")
        .unwrap()
        .create_dir("empty")
        .unwrap();
    manager
        .exclusive("original")
        .unwrap()
        .create_dir("empty/inner")
        .unwrap();
    let bytes =
        stream_bytes(backup::snapshot(&session, "original", 1024 * 1024, control()).unwrap());
    // A standard independent tar reader can consume the exported format.
    let decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(decoder);
    let exported = archive
        .entries()
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(
                !entry.path_bytes().contains(&b'\\'),
                "Archive names use portable slash separators"
            );
            entry.path().unwrap().into_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        exported
            .iter()
            .any(|path| path == std::path::Path::new("wasmdroid-backup.json"))
    );
    let result = restore(&session, "restored", &bytes, 4 * 1024 * 1024).unwrap();
    assert_eq!(result.files, 1);
    assert_eq!(result.bytes, payload.len() as u64);
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/restored").join(&name)).unwrap(),
        payload
    );
    assert!(root.path().join("volumes/data/restored/empty").is_dir());
    assert!(
        root.path()
            .join("volumes/data/restored/empty/inner")
            .is_dir()
    );
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/original").join(&name)).unwrap(),
        payload
    );
    assert!(restore(&session, "original", &bytes, 4 * 1024 * 1024).is_err());
}

#[test]
fn nested_import_keeps_portable_paths_without_accepting_raw_backslashes_or_traversal() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let manager = VolumeManager::open(&session).unwrap();
    manager.create("saved", BTreeMap::new()).unwrap();
    let payload = b"nested\0binary\xff";
    let mut transfer = DataImport::begin(
        &session,
        "saved",
        "one/two/three/note",
        false,
        Some(payload.len() as u64),
        1024,
        control(),
    )
    .unwrap();
    transfer.append(payload).unwrap();
    assert_eq!(transfer.commit().unwrap(), payload.len() as u64);
    drop(transfer);
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/saved/one/two/three/note")).unwrap(),
        payload
    );
    std::fs::write(root.path().join("outside"), b"host sentinel").unwrap();
    for path in [
        "one\\two\\escape",
        "../outside",
        "one/../../outside",
        "/outside",
    ] {
        assert!(DataImport::begin(&session, "saved", path, false, None, 1024, control()).is_err());
    }
    let directory = manager.exclusive("saved").unwrap();
    assert!(
        wasmdroid_engine::data::ensure_parent(
            &directory,
            std::path::Path::new("unexpected/../../outside")
        )
        .is_err()
    );
    assert!(directory.metadata("unexpected").is_err());
    assert_eq!(
        std::fs::read(root.path().join("outside")).unwrap(),
        b"host sentinel"
    );
}
fn raw_entry(output: &mut Vec<u8>, name: &str, kind: u8, data: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o600);
    header.set_entry_type(tar::EntryType::new(kind));
    header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
    header.set_cksum();
    output.extend_from_slice(header.as_bytes());
    output.extend_from_slice(data);
    output.resize(output.len() + (512 - data.len() % 512) % 512, 0);
}
fn crafted(name: &str, kind: u8, duplicate: bool) -> Vec<u8> {
    let mut tar = Vec::new();
    let manifest =
        br#"{"format":"wasmdroid.volume","version":1,"source_volume":"source","created_at_ms":0}"#;
    raw_entry(&mut tar, "wasmdroid-backup.json", b'0', manifest);
    raw_entry(&mut tar, name, kind, b"payload");
    if duplicate {
        raw_entry(&mut tar, name, kind, b"different");
    }
    tar.extend_from_slice(&[0; 1024]);
    let mut compressor = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    compressor.write_all(&tar).unwrap();
    compressor.finish().unwrap()
}
#[test]
fn malformed_archives_never_replace_existing_data_or_escape_the_new_volume() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    create_data(&session, "existing", "note", b"keep this");
    std::fs::write(root.path().join("outside"), b"host sentinel").unwrap();
    let mut cases = vec![
        crafted("data/../../outside", b'0', false),
        crafted("/outside", b'0', false),
        crafted("data/link", b'2', false),
        crafted("data/hardlink", b'1', false),
        crafted("data/sparse", b'S', false),
        crafted("data/note", b'0', true),
        crafted("data/dir/../note", b'0', false),
        crafted("data/nested\\escape", b'0', false),
    ];
    let mut crc = crafted("data/note", b'0', false);
    let at = crc.len() - 8;
    crc[at] ^= 0x80;
    cases.push(crc);
    let mut trailing = crafted("data/note", b'0', false);
    trailing.extend(crafted("data/extra", b'0', false));
    cases.push(trailing);
    let mut header = vec![0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 255];
    header.extend(vec![b'x'; 16 * 1024 + 1]);
    header.push(0);
    cases.push(header);
    for (index, bytes) in cases.iter().enumerate() {
        let target = format!("restore-{index}");
        assert!(
            restore(&session, &target, bytes, 1024 * 1024).is_err(),
            "case {index}"
        );
        assert!(
            VolumeManager::open(&session)
                .unwrap()
                .inspect(&target)
                .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join("volumes/data/existing/note")).unwrap(),
            b"keep this"
        );
        assert_eq!(
            std::fs::read(root.path().join("outside")).unwrap(),
            b"host sentinel"
        );
    }
}
#[test]
fn decompression_and_compressed_input_budgets_are_enforced() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let bytes = crafted("data/note", b'0', false);
    assert!(restore(&session, "budget", &bytes, 512).is_err());
    let mut transfer = RestoreInput::begin(&session, "small", 1, 1024 * 1024, control()).unwrap();
    assert!(transfer.append(&bytes).is_err());
    assert!(
        VolumeManager::open(&session)
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
}
fn artifact(reference: &str, bytes: &[u8]) -> Artifact {
    Artifact {
        reference: ArtifactReference::parse(reference).unwrap(),
        digest: Digest::of(bytes),
        kind: ArtifactKind::Webc,
        driver: "wasmer-local".into(),
        platform: "wasm32-wasix".into(),
        commands: vec!["run".into()],
        size_bytes: bytes.len() as u64,
        labels: BTreeMap::new(),
    }
}
#[test]
fn cache_gc_waits_for_installation_and_uninstall_protects_references_and_data() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    let state = session.state();
    let store = ContentStore::open(root.path().join("store")).unwrap();
    let live = artifact("wasmer.io/example/live@1.0.0", b"live");
    let dead = Digest::of(b"orphan");
    store.ingest(&b"live"[..], &live.digest, 1024).unwrap();
    store.ingest(&b"orphan"[..], &dead, 1024).unwrap();
    state.put_artifact(&live).unwrap();
    create_data(&session, "saved", "note", b"persistent");
    let lease = store.installation_lease().unwrap();
    assert!(matches!(
        store.collect_garbage(state),
        Err(EngineError::Conflict(_))
    ));
    assert!(store.path(&dead).exists());
    drop(lease);
    let report = store.collect_garbage(state).unwrap();
    assert_eq!(report.blobs_removed, 1);
    assert!(store.path(&live.digest).exists());
    let spec: InstanceSpec =
        serde_json::from_value(serde_json::json!({"artifact_ref":live.reference,"command":"run"}))
            .unwrap();
    let instance = state.create_instance("using", &spec).unwrap();
    assert!(store.uninstall(state, &live.reference).is_err());
    state.remove_instance("using", instance.version).unwrap();
    let mut parent = artifact("wasmer.io/example/parent@1.0.0", b"parent");
    store.ingest(&b"parent"[..], &parent.digest, 1024).unwrap();
    parent.labels.insert(
        wasmdroid_engine::installer::DEPENDENCIES_LABEL.into(),
        serde_json::to_string(&vec![live.reference.clone()]).unwrap(),
    );
    state.put_artifact(&parent).unwrap();
    assert!(store.uninstall(state, &live.reference).is_err());
    store.uninstall(state, &parent.reference).unwrap();
    store.uninstall(state, &live.reference).unwrap();
    assert!(!store.path(&live.digest).exists());
    assert_eq!(
        std::fs::read(root.path().join("volumes/data/saved/note")).unwrap(),
        b"persistent"
    );
}
