//! Unix filesystem observations and portable ownership/budget contracts, not
//! Android GUI/free-space evidence. Non-Unix explicitly declines enumeration.
use std::{fs, path::Path, sync::atomic::AtomicBool, time::Duration};
#[cfg(unix)]
use wasmdroid_engine::storage_usage::{CategoryUsage, StorageCategory};
use wasmdroid_engine::{
    EngineError,
    session::EngineSession,
    storage_usage::{ScanBudget, StorageUsage, StorageUsageSnapshot, UsageIssue, UsageStatus},
};

fn budget() -> ScanBudget {
    ScanBudget::new(65_536, 32, Duration::from_secs(5)).unwrap()
}
fn snapshot(usage: &StorageUsage) -> StorageUsageSnapshot {
    usage.snapshot(budget(), &AtomicBool::new(false))
}
#[cfg(unix)]
fn category(snapshot: &StorageUsageSnapshot, category: StorageCategory) -> &CategoryUsage {
    snapshot
        .categories
        .iter()
        .find(|value| value.category == category)
        .unwrap()
}
fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
#[cfg(unix)]
fn counts_only_fixed_categories_and_serializes_no_names_or_contents() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(
        temporary.path(),
        "store/blobs/sha256/credential-file",
        b"abc",
    );
    write(temporary.path(), "compiled-cache/module", b"12345");
    write(
        temporary.path(),
        "volumes/data/saved/private-env",
        b"1234567",
    );
    write(temporary.path(), "logs/run/generation", b"12345678901");
    // Neither outside known categories nor volume lock/trash/staging are part
    // of these four category totals. They must not become a fake app total.
    write(temporary.path(), "unaccounted-private-file", &[1; 101]);
    write(temporary.path(), "volumes/trash/preserved", &[1; 103]);
    let usage = StorageUsage::capture(&session).unwrap();
    let report = snapshot(&usage);
    assert!(report.complete);
    assert_eq!(report.observed_total_bytes, Some(26));
    for (kind, bytes) in [
        (StorageCategory::Artifacts, 3),
        (StorageCategory::CompiledCache, 5),
        (StorageCategory::Volumes, 7),
        (StorageCategory::Logs, 11),
    ] {
        let value = category(&report, kind);
        assert_eq!(value.status, UsageStatus::Complete);
        assert_eq!(value.logical_regular_file_bytes, Some(bytes));
        assert_eq!(value.files_counted, 1);
    }
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("best_effort_non_atomic"));
    for secret in [
        "credential-file",
        "private-env",
        "generation",
        "unaccounted-private-file",
        "12345678901",
    ] {
        assert!(!json.contains(secret));
    }
    assert!(!json.contains(temporary.path().to_string_lossy().as_ref()));
    assert_eq!(
        fs::read(temporary.path().join("volumes/trash/preserved")).unwrap(),
        vec![1; 103]
    );
}

#[test]
#[cfg(unix)]
fn uncreated_categories_are_empty_without_creating_directories() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let before = fs::read_dir(temporary.path()).unwrap().count();
    let usage = StorageUsage::capture(&session).unwrap();
    let report = snapshot(&usage);
    assert!(report.complete);
    assert_eq!(report.observed_total_bytes, Some(0));
    assert_eq!(report.entries_visited, 0);
    for directory in ["store", "compiled-cache", "volumes", "logs"] {
        assert!(!temporary.path().join(directory).exists());
    }
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), before);
}

#[test]
#[cfg(unix)]
fn global_entry_budget_is_shared_and_unscanned_categories_are_not_zero() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    for index in 0..10 {
        write(
            temporary.path(),
            &format!("store/blobs/sha256/{index}"),
            b"abc",
        );
    }
    write(
        temporary.path(),
        "compiled-cache/not-observed",
        b"123456789",
    );
    let usage = StorageUsage::capture(&session).unwrap();
    let report = usage.snapshot(
        ScanBudget::new(3, 32, Duration::from_secs(5)).unwrap(),
        &AtomicBool::new(false),
    );
    assert_eq!(report.entries_visited, 3);
    assert!(!report.complete);
    assert_eq!(report.observed_total_bytes, None);
    let artifacts = category(&report, StorageCategory::Artifacts);
    assert_eq!(artifacts.logical_regular_file_bytes, Some(9));
    assert_eq!(artifacts.status, UsageStatus::Partial);
    assert!(artifacts.issues.contains(&UsageIssue::EntryLimit));
    let cache = category(&report, StorageCategory::CompiledCache);
    assert_eq!(cache.status, UsageStatus::Unavailable);
    assert_eq!(cache.logical_regular_file_bytes, None);
    assert_eq!(
        fs::read(temporary.path().join("compiled-cache/not-observed"))
            .unwrap()
            .len(),
        9
    );
}

#[test]
#[cfg(unix)]
fn depth_limit_keeps_observed_files_and_explicitly_omits_deeper_data() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(temporary.path(), "volumes/data/top", b"abc");
    write(temporary.path(), "volumes/data/one/visible", b"12345");
    write(
        temporary.path(),
        "volumes/data/one/two/hidden",
        b"12345678901",
    );
    let usage = StorageUsage::capture(&session).unwrap();
    let report = usage.snapshot(
        ScanBudget::new(100, 1, Duration::from_secs(5)).unwrap(),
        &AtomicBool::new(false),
    );
    let volumes = category(&report, StorageCategory::Volumes);
    assert_eq!(volumes.logical_regular_file_bytes, Some(8));
    assert_eq!(volumes.status, UsageStatus::Partial);
    assert!(volumes.issues.contains(&UsageIssue::DepthLimit));
    assert!(!report.complete);
    assert_eq!(report.observed_total_bytes, Some(8)); // Observed, not current total.
}

#[test]
#[cfg(unix)]
fn cancelled_scan_reports_unavailable_without_claiming_zero_or_deleting_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(temporary.path(), "logs/kept", b"private fixture bytes");
    let usage = StorageUsage::capture(&session).unwrap();
    let report = usage.snapshot(budget(), &AtomicBool::new(true));
    assert_eq!(report.entries_visited, 0);
    assert_eq!(report.observed_total_bytes, None);
    assert!(!report.complete);
    for value in &report.categories {
        assert_eq!(value.status, UsageStatus::Unavailable);
        assert_eq!(value.logical_regular_file_bytes, None);
        assert_eq!(value.issues, vec![UsageIssue::Cancelled]);
    }
    assert_eq!(
        fs::read(temporary.path().join("logs/kept")).unwrap(),
        b"private fixture bytes"
    );
}

#[test]
#[cfg(unix)]
fn unsupported_category_root_is_unavailable_and_other_categories_remain_useful() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(
        temporary.path(),
        "compiled-cache",
        b"preserved wrong-type sentinel",
    );
    write(temporary.path(), "logs/kept", b"abc");
    let usage = StorageUsage::capture(&session).unwrap();
    let report = snapshot(&usage);
    let cache = category(&report, StorageCategory::CompiledCache);
    assert_eq!(cache.status, UsageStatus::Unavailable);
    assert_eq!(cache.logical_regular_file_bytes, None);
    assert_eq!(cache.issues, vec![UsageIssue::UnsafeEntry]);
    assert_eq!(
        category(&report, StorageCategory::Logs).logical_regular_file_bytes,
        Some(3)
    );
    assert_eq!(report.observed_total_bytes, None);
    assert_eq!(
        fs::read(temporary.path().join("compiled-cache")).unwrap(),
        b"preserved wrong-type sentinel"
    );
}

#[test]
fn captured_scan_retains_owner_after_database_session_is_dropped() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(temporary.path(), "logs/kept", b"abc");
    let usage = StorageUsage::capture(&session).unwrap();
    drop(session);
    assert!(matches!(
        EngineSession::open(temporary.path()),
        Err(EngineError::Conflict(_))
    ));
    let report = snapshot(&usage);
    #[cfg(unix)]
    assert_eq!(report.observed_total_bytes, Some(3));
    #[cfg(not(unix))]
    assert_eq!(report.observed_total_bytes, None);
    drop(usage);
    let reopened = EngineSession::open(temporary.path()).unwrap();
    assert_eq!(reopened.recovered_instances(), 0);
}

#[test]
#[cfg(unix)]
fn logical_file_length_is_not_physical_allocated_space_and_hardlinks_count_per_entry() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    let directory = temporary.path().join("volumes/data/owned");
    fs::create_dir_all(&directory).unwrap();
    let file = fs::File::create(directory.join("sparse")).unwrap();
    file.set_len(4 * 1024 * 1024).unwrap();
    fs::hard_link(directory.join("sparse"), directory.join("same-data")).unwrap();
    let report = snapshot(&StorageUsage::capture(&session).unwrap());
    assert_eq!(
        category(&report, StorageCategory::Volumes).logical_regular_file_bytes,
        Some(8 * 1024 * 1024)
    );
    assert_eq!(category(&report, StorageCategory::Volumes).files_counted, 2);
    assert!(report.complete);
}

#[test]
fn invalid_technical_budgets_are_rejected_without_a_permissive_fallback() {
    for (entries, depth, duration) in [
        (0, 1, Duration::from_secs(1)),
        (65_537, 1, Duration::from_secs(1)),
        (1, 0, Duration::from_secs(1)),
        (1, 33, Duration::from_secs(1)),
        (1, 1, Duration::ZERO),
        (1, 1, Duration::from_secs(6)),
    ] {
        assert!(ScanBudget::new(entries, depth, duration).is_err());
    }
}

#[test]
#[cfg(not(unix))]
fn unsupported_platform_never_uses_ambient_enumeration_or_fabricates_zero() {
    let temporary = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(
        temporary.path(),
        "logs/not-observed",
        b"private fixture bytes",
    );
    write(
        temporary.path(),
        "compiled-cache",
        b"wrong-type sentinel must not be inspected",
    );
    let usage = StorageUsage::capture(&session).unwrap();
    let report = snapshot(&usage);
    assert!(!report.complete);
    assert_eq!(report.entries_visited, 0);
    assert_eq!(report.observed_total_bytes, None);
    assert_eq!(report.categories.len(), 4);
    for value in &report.categories {
        assert_eq!(value.status, UsageStatus::Unavailable);
        assert_eq!(value.logical_regular_file_bytes, None);
        assert_eq!(value.entries_visited, 0);
        assert_eq!(value.issues, vec![UsageIssue::PlatformUnsupported]);
    }
    assert_eq!(
        fs::read(temporary.path().join("logs/not-observed")).unwrap(),
        b"private fixture bytes"
    );
}

#[test]
#[cfg(unix)]
fn symlink_roots_and_entries_never_escape_or_alias_another_category() {
    use std::os::unix::fs::symlink;
    let temporary = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let session = EngineSession::open(temporary.path()).unwrap();
    write(
        outside.path(),
        "sentinel",
        b"outside bytes must not be observed",
    );
    fs::create_dir_all(temporary.path().join("store/blobs")).unwrap();
    symlink(outside.path(), temporary.path().join("store/blobs/sha256")).unwrap();
    write(temporary.path(), "volumes/data/owned/regular", b"abc");
    symlink(
        outside.path(),
        temporary.path().join("volumes/data/owned/escape"),
    )
    .unwrap();
    symlink(
        temporary.path().join("volumes/data"),
        temporary.path().join("compiled-cache"),
    )
    .unwrap();
    let report = snapshot(&StorageUsage::capture(&session).unwrap());
    assert_eq!(
        category(&report, StorageCategory::Artifacts).status,
        UsageStatus::Unavailable
    );
    assert_eq!(
        category(&report, StorageCategory::CompiledCache).status,
        UsageStatus::Unavailable
    );
    let volumes = category(&report, StorageCategory::Volumes);
    assert_eq!(volumes.logical_regular_file_bytes, Some(3));
    assert_eq!(volumes.status, UsageStatus::Partial);
    assert!(volumes.issues.contains(&UsageIssue::UnsafeEntry));
    assert_eq!(
        fs::read(outside.path().join("sentinel")).unwrap(),
        b"outside bytes must not be observed"
    );
}

#[test]
#[cfg(unix)]
fn captured_root_handle_does_not_retarget_when_ambient_path_is_replaced() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("engine");
    let session = EngineSession::open(&root).unwrap();
    write(&root, "logs/original", b"abc");
    let usage = StorageUsage::capture(&session).unwrap();
    fs::rename(&root, temporary.path().join("moved")).unwrap();
    write(&root, "logs/replacement", &[1; 101]);
    assert_eq!(snapshot(&usage).observed_total_bytes, Some(3));
    assert_eq!(fs::read(root.join("logs/replacement")).unwrap().len(), 101);
}
