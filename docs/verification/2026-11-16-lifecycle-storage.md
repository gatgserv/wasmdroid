# Durable storage and experimental cancellation — 2026-11-16

Application code: `0eb7776cb695898420d598bf34222505a225fb5a`.
Diagnostic test-only change: `09de898d8ed0d4edd4d92b2098b2517581be2ee0`.
Author/committer: gatgserv. Earlier revision mappings are recorded in
[the attribution correction](2026-11-16-attribution.md).
Tracking: [issue #3](https://github.com/gatgserv/wasmdroid/issues/3),
[PR #2](https://github.com/gatgserv/wasmdroid/pull/2).

## Verified storage scope

Implemented schema v2 lifecycle records, CAS transition revisions, transactional
events, volume references, exclusive engine ownership/recovery and named-volume
capabilities. A stale exit cannot overwrite a restarted record. Interrupted active
records become dead with unknown exit, never fabricated success. Open volume files
retain engine ownership; referenced/open volumes cannot be removed. Read-only
capabilities reject mutation; traversal/symlink escape is denied. Removal recovery
restores pre-commit data or completes journaled committed cleanup, preserving
ambiguous/unregistered data. This is process-interruption recovery, not a tested
power-loss durability guarantee.

Windows: **35 Rust tests passed**, no skips/failures. Commands on application
revision `0eb7776`:

```text
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

Arm64 cross-target Clippy passed with all targets for default Cranelift and the
explicit experimental Singlepass selection. Android lint on application code
`0eb7776`: **0 errors, 9 warnings**. Rust 1.95.0, Wasmer 7.4.2/WASIX 0.704.2,
NDK 28.2.13676358, JDK 21; debug-signed APK with Rust release optimization.
Unix-only filesystem and signal-mask unit tests require native Linux CI; Windows
host results do not certify them. General RuntimeDriver, guest volume mounts,
foreground ownership, product UI and final live E2E are not implemented here.

Physical device: Sony G8142, Android 15/API 35, arm64-v8a. The combined run on
`09de898` passed both storage tests: persistence after close/reopen, 27 payload
bytes, read-only mutation denial, lease-protected deletion, actual symlink/traversal
denial and safe removal; generated UniFFI storage create/list/remove/events also
passed. These are real private data operations, not proof of guest mounts.

The first exploratory Android storage run failed because pinned Rust's standard
file locking is unsupported there. `fs2` supplies real OS locks; the passing
tests retain the lock-conflict assertions. There is no in-memory lock fallback.

## Cancellation is not accepted

Focused tests initially passed hot WASIX CPU-loop interruption, confirmed native
HostInterrupt traps, zero remaining callbacks and a second running instance.
Android workers inherited a blocked SIGUSR1; unblocking it only on dedicated
workers and restoring their masks was necessary for that focused result.
It is **insufficient to establish reliable stop** in a shared process.

Combined run on `0eb7776`: three bridge tests, real registry Python and the first
HTTP test passed; the simultaneous HTTP test hung until the **300-second host
timeout**. The runner stopped only WasmDroid, preserving app data and other apps.
See [partial timeout output](2026-11-16-lifecycle-combined-timeout.txt).

Adding phase messages changes no assertions or native implementation. On
`09de898`, focused HTTP checks passed: **2 tests, 24.18 s**, ADB exit 0; both
servers responded, roots stayed isolated, stopping one preserved the other, and
both close operations completed. See [focused output](2026-11-16-http-focused-09de898.txt).

The subsequent unchanged combined suite completed with **9 tests, 2 failures,
74.094 s**; the runner correctly returned failure:

- simultaneous HTTP cleanup: `guest status finished but 1 execution tasks are still active`;
- hot CPU cancellation: `CPU-loop execution did not actually unwind`, with SIGUSR1
  already unblocked (`SigBlk: 0000000080001004`).

Three bridge tests, Python, single-server HTTP and two storage tests passed.
See [combined failure output](2026-11-16-lifecycle-combined-failure.txt).
Focused successes do not erase the combined failure; cause remains unresolved.
Do not expose arbitrary user execution on the assumption this stop path is safe.

Device commands (serial intentionally omitted from public records):

```powershell
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -TestClass 'dev.wasmdroid.app.HttpProbeTest' -TimeoutSeconds 180
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -TestClass 'dev.wasmdroid.app.EngineBridgeTest,dev.wasmdroid.app.RegistryProbeTest,dev.wasmdroid.app.HttpProbeTest,dev.wasmdroid.app.CancellationProbeTest,dev.wasmdroid.app.StorageProbeTest' -TimeoutSeconds 300 -SkipBuild
```

## Next acceptance

The owner clarified a common supervisor with a service controller and descendant
execution group per package. See [service supervision](../design/service-supervision.md).
Whole-group termination must be verified independently of parent/status exit and
preserve other services. An Android worker process can be evaluated as that
termination boundary while registry/database/volumes stay common. No such
implementation or accepted forced-stop result is claimed yet. The issue remains open.
