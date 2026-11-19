//! Bounded, read-only observations of fixed private storage categories. File
//! lengths are logical bytes, not physical blocks, free space or an app quota.
//! No snapshot freezes concurrent writers, follows a symlink or emits a name.
use crate::{
    EngineError, Result,
    session::{EngineOwnership, EngineSession},
};
use cap_primitives::fs::{self as capability, FollowSymlinks};
use serde::Serialize;
use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_ENTRIES: usize = 65_536;
const MAX_DEPTH: usize = 32;
const MAX_ELAPSED: Duration = Duration::from_secs(5);

/// Technical scan admission only; no product quota or persistent policy.
#[derive(Debug, Clone, Copy)]
pub struct ScanBudget {
    max_entries: usize,
    max_depth: usize,
    max_elapsed: Duration,
}
impl ScanBudget {
    pub fn new(max_entries: usize, max_depth: usize, max_elapsed: Duration) -> Result<Self> {
        if !(1..=MAX_ENTRIES).contains(&max_entries)
            || !(1..=MAX_DEPTH).contains(&max_depth)
            || max_elapsed.is_zero()
            || max_elapsed > MAX_ELAPSED
        {
            return Err(EngineError::Invalid(
                "storage scan budget exceeds supported bounds".into(),
            ));
        }
        Ok(Self {
            max_entries,
            max_depth,
            max_elapsed,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCategory {
    Artifacts,
    CompiledCache,
    Volumes,
    Logs,
}
impl StorageCategory {
    fn components(self) -> &'static [&'static str] {
        match self {
            Self::Artifacts => &["store", "blobs", "sha256"],
            Self::CompiledCache => &["compiled-cache"],
            Self::Volumes => &["volumes", "data"],
            Self::Logs => &["logs"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    Complete,
    Partial,
    Unavailable,
}

/// Fixed, bounded explanations only. Never contains an IO error/path/filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageIssue {
    PlatformUnsupported,
    EntryLimit,
    DepthLimit,
    TimeLimit,
    Cancelled,
    UnsafeEntry,
    PermissionDenied,
    EntryChanged,
    Io,
    Overflow,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryUsage {
    pub category: StorageCategory,
    pub status: UsageStatus,
    pub logical_regular_file_bytes: Option<u64>,
    pub files_counted: usize,
    pub entries_visited: usize,
    pub issues: Vec<UsageIssue>,
}
impl CategoryUsage {
    fn new(category: StorageCategory) -> Self {
        Self {
            category,
            status: UsageStatus::Unavailable,
            logical_regular_file_bytes: None,
            files_counted: 0,
            entries_visited: 0,
            issues: Vec::new(),
        }
    }
    fn issue(&mut self, issue: UsageIssue) {
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
        if self.status == UsageStatus::Complete {
            self.status = UsageStatus::Partial;
        }
    }
    fn observed(&mut self) {
        if self.status == UsageStatus::Unavailable {
            self.status = if self.issues.is_empty() {
                UsageStatus::Complete
            } else {
                UsageStatus::Partial
            };
            self.logical_regular_file_bytes = Some(0);
        }
    }
    fn add_file(&mut self, bytes: u64) -> bool {
        let Some(total) = self
            .logical_regular_file_bytes
            .and_then(|total| total.checked_add(bytes))
        else {
            self.logical_regular_file_bytes = None;
            self.issue(UsageIssue::Overflow);
            return false;
        };
        self.logical_regular_file_bytes = Some(total);
        self.files_counted += 1;
        true
    }
}

#[derive(Debug, Serialize)]
pub struct StorageUsageSnapshot {
    pub schema: u32,
    /// All bytes count regular-file lengths per directory entry. Hard links
    /// may be counted more than once; sparse files retain their logical size.
    pub metric: &'static str,
    pub consistency: &'static str,
    /// Only the four explicitly listed categories, never all app/phone data.
    pub categories: Vec<CategoryUsage>,
    pub complete: bool,
    /// Sum of observed category values; null if any category is unavailable or
    /// arithmetic overflows. Partial observations are not a current lower bound.
    pub observed_total_bytes: Option<u64>,
    pub entries_visited: usize,
    pub issues: Vec<UsageIssue>,
}

/// Captured from a trusted existing common owner. Retains OS ownership without
/// a SQLite connection, session mutex, data lease or content-store mutation.
pub struct StorageUsage {
    root: File,
    _owner: EngineOwnership,
}
impl StorageUsage {
    pub fn capture(session: &EngineSession) -> Result<Self> {
        let root = capability::open_ambient_dir(session.root(), cap_std::ambient_authority())?;
        Ok(Self {
            root,
            _owner: session.ownership(),
        })
    }
    /// Checks cancellation/budget between filesystem calls. A blocking kernel
    /// metadata call is not forcibly interrupted by the elapsed-time bound.
    pub fn snapshot(&self, budget: ScanBudget, cancel: &AtomicBool) -> StorageUsageSnapshot {
        let mut scan = Scan {
            budget,
            cancel,
            start: Instant::now(),
            visited: 0,
        };
        let mut categories = Vec::with_capacity(4);
        for category in [
            StorageCategory::Artifacts,
            StorageCategory::CompiledCache,
            StorageCategory::Volumes,
            StorageCategory::Logs,
        ] {
            let mut usage = CategoryUsage::new(category);
            // cap-primitives4 Windows read_base_dir reconstructs an ambient
            // path. It cannot supply our retained-handle race guarantee; never
            // enumerate via that fallback. Android/Unix uses fd-relative IO.
            if !cfg!(unix) {
                usage.issue(UsageIssue::PlatformUnsupported);
            } else if let Some(reason) = scan.stop() {
                usage.issue(reason);
            } else {
                match self.category_root(category, &scan) {
                    Ok(Some(directory)) => walk(directory, 0, &mut scan, &mut usage),
                    Ok(None) => usage.observed(), // Not created; never create it for a read.
                    Err(reason) => usage.issue(reason),
                }
            }
            categories.push(usage);
        }
        let observed_total_bytes = categories.iter().try_fold(0u64, |total, category| {
            total.checked_add(category.logical_regular_file_bytes?)
        });
        let issues = if observed_total_bytes.is_none()
            && categories
                .iter()
                .all(|category| category.logical_regular_file_bytes.is_some())
        {
            vec![UsageIssue::Overflow]
        } else {
            Vec::new()
        };
        let complete = issues.is_empty()
            && categories
                .iter()
                .all(|category| category.status == UsageStatus::Complete);
        StorageUsageSnapshot {
            schema: 1,
            metric: "logical_regular_file_bytes",
            consistency: "best_effort_non_atomic",
            categories,
            complete,
            observed_total_bytes,
            entries_visited: scan.visited,
            issues,
        }
    }
    fn category_root(
        &self,
        category: StorageCategory,
        scan: &Scan<'_>,
    ) -> std::result::Result<Option<File>, UsageIssue> {
        let mut directory = self.root.try_clone().map_err(|error| io_issue(&error))?;
        for component in category.components() {
            if let Some(reason) = scan.stop() {
                return Err(reason);
            }
            let path = Path::new(component);
            let metadata = match capability::stat(&directory, path, FollowSymlinks::No) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(io_issue(&error)),
            };
            if unsafe_entry(&metadata) || !metadata.is_dir() {
                return Err(UsageIssue::UnsafeEntry);
            }
            // Each component is a single trusted basename. No ancestor symlink
            // can be introduced between metadata and this nofollow handle open.
            directory = capability::open_dir_nofollow(&directory, path)
                .map_err(|error| io_issue(&error))?;
        }
        Ok(Some(directory))
    }
}

struct Scan<'a> {
    budget: ScanBudget,
    cancel: &'a AtomicBool,
    start: Instant,
    visited: usize,
}
impl Scan<'_> {
    fn stop(&self) -> Option<UsageIssue> {
        if self.cancel.load(Ordering::Acquire) {
            Some(UsageIssue::Cancelled)
        } else if self.start.elapsed() >= self.budget.max_elapsed {
            Some(UsageIssue::TimeLimit)
        } else if self.visited >= self.budget.max_entries {
            Some(UsageIssue::EntryLimit)
        } else {
            None
        }
    }
}

fn walk(directory: File, depth: usize, scan: &mut Scan<'_>, usage: &mut CategoryUsage) {
    if let Some(reason) = scan.stop() {
        usage.issue(reason);
        return;
    }
    let mut entries = match capability::read_base_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            usage.issue(io_issue(&error));
            return;
        }
    };
    usage.observed();
    loop {
        if let Some(reason) = scan.stop() {
            usage.issue(reason);
            return;
        }
        let Some(entry) = entries.next() else {
            return;
        };
        scan.visited += 1;
        usage.entries_visited += 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                usage.issue(io_issue(&error));
                continue;
            }
        };
        if let Some(reason) = scan
            .stop()
            .filter(|reason| *reason != UsageIssue::EntryLimit)
        {
            usage.issue(reason);
            return;
        }
        // DirEntry::metadata is explicitly nofollow on the opened parent;
        // neither file contents nor arbitrary ambient paths are ever opened.
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                usage.issue(io_issue(&error));
                continue;
            }
        };
        if unsafe_entry(&metadata) {
            usage.issue(UsageIssue::UnsafeEntry);
            continue;
        }
        if metadata.is_file() {
            if !usage.add_file(metadata.len()) {
                return;
            }
        } else if metadata.is_dir() {
            if depth >= scan.budget.max_depth {
                usage.issue(UsageIssue::DepthLimit);
                continue;
            }
            if let Some(reason) = scan.stop() {
                usage.issue(reason);
                return;
            }
            let name = entry.file_name();
            match open_child(&directory, &name) {
                Ok(child) => walk(child, depth + 1, scan, usage),
                Err(error) => usage.issue(io_issue(&error)),
            }
        } else {
            usage.issue(UsageIssue::UnsafeEntry);
        }
    }
}

fn open_child(parent: &File, name: &OsStr) -> io::Result<File> {
    capability::open_dir_nofollow(parent, Path::new(name))
}
fn unsafe_entry(metadata: &capability::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use cap_primitives::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        } // Any reparse point.
    }
    false
}
fn io_issue(error: &io::Error) -> UsageIssue {
    match error.kind() {
        io::ErrorKind::PermissionDenied => UsageIssue::PermissionDenied,
        io::ErrorKind::NotFound => UsageIssue::EntryChanged,
        _ => UsageIssue::Io,
    }
}
