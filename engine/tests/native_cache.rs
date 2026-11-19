//! Trust-boundary regressions independent of Wasmer deserialization.
use std::io::{Seek, SeekFrom, Write};
use wasmdroid_engine::{compiled_cache::TrustedCodeCache, session::EngineSession};

#[test]
fn disk_cache_reopens_but_never_accepts_modified_or_foreign_native_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let session = EngineSession::open(directory.path()).unwrap();
    let cache = TrustedCodeCache::open(&session).unwrap();
    cache
        .save_local(b"source-A/build-A/cpu-A", b"locally compiled output")
        .unwrap();
    drop(cache);
    let cache = TrustedCodeCache::open(&session).unwrap();
    assert_eq!(
        cache.load(b"source-A/build-A/cpu-A").unwrap().unwrap(),
        b"locally compiled output"
    );
    for context in [
        b"source-B/build-A/cpu-A",
        b"source-A/build-B/cpu-A",
        b"source-A/build-A/cpu-B",
    ] {
        assert!(cache.load(context).unwrap().is_none());
    }
    let path = std::fs::read_dir(directory.path().join("compiled-cache"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|e| e == "cache"))
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    // Valid lengths/magic and unchanged payload still cannot authenticate a
    // forged tag, and authentic headers cannot authorize modified code bytes.
    for index in [0, 8, 16, original.len() - 1] {
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(index as u64)).unwrap();
        file.write_all(&[original[index] ^ 0x80]).unwrap();
        drop(file);
        assert!(cache.load(b"source-A/build-A/cpu-A").unwrap().is_none());
        std::fs::write(&path, &original).unwrap();
    }
    // A cache from another install has a different private authentication key.
    let foreign = tempfile::tempdir().unwrap();
    let other = EngineSession::open(foreign.path()).unwrap();
    let other_cache = TrustedCodeCache::open(&other).unwrap();
    std::fs::copy(
        &path,
        foreign
            .path()
            .join("compiled-cache")
            .join(path.file_name().unwrap()),
    )
    .unwrap();
    assert!(
        other_cache
            .load(b"source-A/build-A/cpu-A")
            .unwrap()
            .is_none()
    );
}

#[test]
fn cache_cleanup_preserves_unknown_files_and_application_data() {
    let directory = tempfile::tempdir().unwrap();
    let session = EngineSession::open(directory.path()).unwrap();
    let cache = TrustedCodeCache::open(&session).unwrap();
    cache.save_local(b"source", b"compiled output").unwrap();
    std::fs::write(directory.path().join("compiled-cache/unknown"), b"preserve").unwrap();
    std::fs::write(directory.path().join("saved-data"), b"user data").unwrap();
    let report = cache.clear().unwrap();
    assert_eq!(report.entries_removed, 1);
    assert!(cache.load(b"source").unwrap().is_none());
    assert!(
        directory
            .path()
            .join("compiled-cache/authentication.key")
            .is_file()
    );
    assert_eq!(
        std::fs::read(directory.path().join("compiled-cache/unknown")).unwrap(),
        b"preserve"
    );
    assert_eq!(
        std::fs::read(directory.path().join("saved-data")).unwrap(),
        b"user data"
    );
}
