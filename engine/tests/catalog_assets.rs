//! Hermetic seed-file helpers. No package execution or Android acceptance.
use cap_std::fs::Dir;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
use wasmdroid_engine::{
    catalog_assets::{CatalogSeedFile, MAX_BYTES, MAX_FILES, manifest_digest, seed, validate},
    session::EngineSession,
    store::Digest,
    volumes::{VolumeDirectory, VolumeManager, VolumeOpenOptions},
};

fn file(path: &str, text: &str) -> CatalogSeedFile {
    CatalogSeedFile {
        volume: "site".into(),
        path: path.into(),
        text: text.into(),
        size_bytes: text.len() as u64,
        digest: Digest::of(text.as_bytes()),
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    session: EngineSession,
    staging: Dir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = EngineSession::open(root.path()).unwrap();
        let cache = root.path().join("transfers/cache");
        std::fs::create_dir_all(&cache).unwrap();
        let staging = Dir::open_ambient_dir(cache, cap_std::ambient_authority()).unwrap();
        Self {
            _root: root,
            session,
            staging,
        }
    }
    fn create(&self) {
        VolumeManager::open(&self.session)
            .unwrap()
            .create_new("scopedseed", BTreeMap::new())
            .unwrap();
    }
    fn directory(&self) -> VolumeDirectory {
        VolumeManager::open(&self.session)
            .unwrap()
            .exclusive("scopedseed")
            .unwrap()
    }
}

fn write(directory: &VolumeDirectory, path: &str, bytes: &[u8]) {
    let mut file = directory
        .open_file(
            path,
            VolumeOpenOptions {
                write: true,
                create_new: true,
                ..Default::default()
            },
        )
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn read(directory: &VolumeDirectory, path: &str) -> Vec<u8> {
    let mut result = Vec::new();
    directory
        .open_file(
            path,
            VolumeOpenOptions {
                read: true,
                ..Default::default()
            },
        )
        .unwrap()
        .read_to_end(&mut result)
        .unwrap();
    result
}

#[test]
fn inline_utf8_requires_exact_bytes_digest_and_strict_fields() {
    let valid = file("index.html", "Notes: €\n");
    validate(std::slice::from_ref(&valid)).unwrap();
    let mut wrong = valid.clone();
    wrong.size_bytes = wrong.text.chars().count() as u64;
    assert!(validate(&[wrong]).is_err());
    let mut wrong = valid.clone();
    wrong.digest = Digest::of(b"wrong");
    assert!(validate(&[wrong]).is_err());
    let mut wrong = valid.clone();
    wrong.text.push('\0');
    wrong.size_bytes = wrong.text.len() as u64;
    wrong.digest = Digest::of(wrong.text.as_bytes());
    assert!(validate(&[wrong]).is_err());
    let mut value = serde_json::to_value(&valid).unwrap();
    value["mode"] = "executable".into();
    assert!(serde_json::from_value::<CatalogSeedFile>(value).is_err());
    assert!(!format!("{valid:?}").contains("Notes"));
}

#[test]
fn count_and_total_byte_budgets_are_checked_before_io() {
    let files: Vec<_> = (0..MAX_FILES)
        .map(|n| file(&format!("{n}.txt"), ""))
        .collect();
    validate(&files).unwrap();
    let mut too_many = files;
    too_many.push(file("extra.txt", ""));
    assert!(validate(&too_many).is_err());
    validate(&[file("full.txt", &"x".repeat(MAX_BYTES))]).unwrap();
    assert!(
        validate(&[
            file("full.txt", &"x".repeat(MAX_BYTES)),
            file("extra.txt", "x")
        ])
        .is_err()
    );
    assert!(validate(&[file("large.txt", &"x".repeat(MAX_BYTES + 1))]).is_err());
}

#[test]
fn paths_reject_traversal_host_aliases_and_ambiguous_names() {
    for path in [
        "",
        "/index.html",
        "../private",
        "a/../private",
        "a/./b",
        "a//b",
        "a/",
        "C:/private",
        "C:private",
        "a\\b",
        "a\0b",
        "./index.html",
        "a\nb",
        "a:b",
        "CON",
        "con.txt",
        "NUL",
        "AUX.txt",
        "COM1.txt",
        "lpt9",
        "trailing.",
        "a /b",
    ] {
        assert!(
            validate(&[file(path, "text")]).is_err(),
            "ambiguous seed path accepted"
        );
    }
    let mut wrong_volume = file("index.html", "safe");
    wrong_volume.volume = "../existing_private".into();
    assert!(validate(&[wrong_volume]).is_err());
    assert!(validate(&[file(&format!("{}.txt", "x".repeat(256)), "")]).is_err());
    assert!(validate(&[file(&format!("{}leaf", "a/".repeat(16)), "")]).is_err());
}

#[test]
fn duplicate_prefix_and_case_collisions_fail_without_platform_aliases() {
    for files in [
        vec![file("index.html", "a"), file("index.html", "b")],
        vec![file("Index.html", "a"), file("index.html", "a")],
        vec![file("assets", "a"), file("assets/index.html", "b")],
        vec![file("Assets/a.txt", "a"), file("assets/b.txt", "b")],
    ] {
        assert!(validate(&files).is_err());
    }
}

#[test]
fn manifest_receipt_is_order_independent_and_commits_content_and_destination() {
    let a = file("index.html", "one");
    let b = file("notes.txt", "two");
    let expected = manifest_digest(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(expected, manifest_digest(&[b.clone(), a.clone()]).unwrap());
    assert_ne!(
        expected,
        manifest_digest(&[file("index.html", "changed"), b.clone()]).unwrap()
    );
    let mut moved = a;
    moved.volume = "other".into();
    assert_ne!(expected, manifest_digest(&[moved, b]).unwrap());
}

#[test]
fn seed_publishes_complete_nested_files_and_cleans_only_own_staging() {
    let fixture = Fixture::new();
    fixture.create();
    let directory = fixture.directory();
    fixture
        .staging
        .write("unrelated-private-cache", b"keep")
        .unwrap();
    let files = [
        file("index.html", "<h1>Notes</h1>\n"),
        file("assets/readme.txt", "Starter data\n"),
    ];
    seed(&directory, &files, &fixture.staging).unwrap();
    for item in &files {
        assert_eq!(read(&directory, &item.path), item.text.as_bytes());
    }
    assert_eq!(
        fixture.staging.read("unrelated-private-cache").unwrap(),
        b"keep"
    );
    assert_eq!(fixture.staging.entries().unwrap().count(), 1);
    assert!(directory.symlink_metadata("assets").unwrap().is_dir);
}

#[test]
fn exact_completed_files_are_not_rewritten_on_retry() {
    let fixture = Fixture::new();
    fixture.create();
    let directory = fixture.directory();
    let files = [
        file("index.html", "first"),
        file("assets/notes.txt", "second"),
    ];
    seed(&directory, &files, &fixture.staging).unwrap();
    let before = directory.symlink_metadata("index.html").unwrap();
    seed(&directory, &files, &fixture.staging).unwrap();
    let after = directory.symlink_metadata("index.html").unwrap();
    assert_eq!(before.modified_ns, after.modified_ns);
    assert_eq!(before.created_ns, after.created_ns);
    assert_eq!(read(&directory, "index.html"), b"first");
    assert_eq!(fixture.staging.entries().unwrap().count(), 0);
}

#[test]
fn partial_completed_seed_is_preserved_and_missing_files_can_resume() {
    let fixture = Fixture::new();
    fixture.create();
    let directory = fixture.directory();
    let files = [
        file("index.html", "first"),
        file("nested/notes.txt", "second"),
    ];
    seed(&directory, &files[..1], &fixture.staging).unwrap();
    assert!(directory.symlink_metadata("nested/notes.txt").is_err());
    seed(&directory, &files, &fixture.staging).unwrap();
    assert_eq!(read(&directory, "index.html"), b"first");
    assert_eq!(read(&directory, "nested/notes.txt"), b"second");
}

#[test]
fn differing_or_unknown_existing_data_blocks_the_entire_seed_before_writes() {
    for target in ["index.html", "unrelated.txt"] {
        let fixture = Fixture::new();
        fixture.create();
        let directory = fixture.directory();
        write(&directory, target, b"user data");
        let files = [file("new.html", "new"), file("index.html", "seed")];
        assert!(seed(&directory, &files, &fixture.staging).is_err());
        assert_eq!(read(&directory, target), b"user data");
        assert!(directory.symlink_metadata("new.html").is_err());
        assert_eq!(fixture.staging.entries().unwrap().count(), 0);
    }
}

#[test]
fn same_length_wrong_digest_and_directory_targets_are_never_replaced() {
    let fixture = Fixture::new();
    fixture.create();
    let directory = fixture.directory();
    write(&directory, "index.html", b"user");
    assert!(seed(&directory, &[file("index.html", "seed")], &fixture.staging).is_err());
    assert_eq!(read(&directory, "index.html"), b"user");
    directory.remove_file("index.html").unwrap();
    directory.create_dir("index.html").unwrap();
    assert!(seed(&directory, &[file("index.html", "seed")], &fixture.staging).is_err());
    assert!(directory.symlink_metadata("index.html").unwrap().is_dir);
}

#[test]
fn shared_read_write_leases_and_mixed_volume_batches_grant_no_seed_authority() {
    let fixture = Fixture::new();
    fixture.create();
    let manager = VolumeManager::open(&fixture.session).unwrap();
    for read_only in [true, false] {
        let shared = manager.lease("scopedseed", read_only).unwrap();
        assert!(seed(&shared, &[file("index.html", "seed")], &fixture.staging).is_err());
        assert!(shared.symlink_metadata("index.html").is_err());
    }
    let exclusive = fixture.directory();
    assert!(manager.lease("scopedseed", false).is_err());
    let mut wrong = file("other.txt", "text");
    wrong.volume = "other".into();
    assert!(
        seed(
            &exclusive,
            &[file("index.html", "seed"), wrong],
            &fixture.staging
        )
        .is_err()
    );
    assert!(exclusive.symlink_metadata("index.html").is_err());
}

#[test]
fn first_party_example_manifest_commits_the_exact_source_asset() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../examples/catalog-seeds/notes/assets.json"
    ))
    .unwrap();
    let files: Vec<CatalogSeedFile> = serde_json::from_value(value["assets"].clone()).unwrap();
    validate(&files).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].volume, "site");
    assert_eq!(files[0].path, "index.html");
    assert_eq!(
        files[0].text.as_bytes(),
        include_bytes!("../../examples/catalog-seeds/notes/index.html")
    );
}

#[cfg(unix)]
#[test]
fn target_and_parent_symlinks_are_rejected_without_touching_their_targets() {
    for parent in [false, true] {
        let fixture = Fixture::new();
        fixture.create();
        let directory = fixture.directory();
        let outside = fixture._root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("index.html"), b"private").unwrap();
        let volume = fixture._root.path().join("volumes/data/scopedseed");
        if parent {
            std::os::unix::fs::symlink(&outside, volume.join("nested")).unwrap();
            assert!(
                seed(
                    &directory,
                    &[file("nested/index.html", "seed")],
                    &fixture.staging
                )
                .is_err()
            );
        } else {
            std::os::unix::fs::symlink(outside.join("index.html"), volume.join("index.html"))
                .unwrap();
            assert!(seed(&directory, &[file("index.html", "seed")], &fixture.staging).is_err());
        }
        assert_eq!(
            std::fs::read(outside.join("index.html")).unwrap(),
            b"private"
        );
    }
}
