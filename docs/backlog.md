# Delivery backlog

The owner selected Apache-2.0 and the public repository `gatgserv/wasmdroid`.
`origin` is configured. The repository was verified empty (no branches, issues,
PRs or CI runs). GitHub CLI authentication as `gatgserv` and repository write
permissions are now verified in the executing context.
The baseline is on `main` through the explicitly approved initialization; new work
is in [PR #2](https://github.com/gatgserv/wasmdroid/pull/2). The remaining Stage 0
acceptance is tracked by [issue #1](https://github.com/gatgserv/wasmdroid/issues/1).
See `implementation-status.md` for the full scope.

## S0-1 — Engine foundation and native bridge (complete)

Scope: neutral contracts; content verification; SQLite migration; Compose →
generated UniFFI → Rust; finite WASI fd_write probe. Prerequisite: none.
Risks: backend support differs by host; standalone Rust MSI can shadow rustup.
Acceptance: Rust checks; Android APK build; targeted native bridge/probe tests;
record exact revision, ABI, commands and limitations in the verification report.
Evidence: [2026-11-16 foundation verification](verification/2026-11-16-stage0-foundation.md),
code revision `55641f8d81d9085322df4653e0a9e09b355e603b`: 15 Rust regressions,
3 actual Android x86_64 integration tests, dual-ABI build, and lint (0 errors,
9 warnings). This does not complete S0-2 or the full Stage 0 exit criterion.

## S0-2 — Registry → verified WebC → Android execution (in progress)

Depends on S0-1. Inspect the current official registry GraphQL schema; resolve
immutable public package versions with hashes; bounded download into the content
store; execute Python and a simple web server from actual WebC commands, with
per-instance filesystem/network isolation. Do not expose unrestricted host
networking as a production shortcut. Runtime import boundary stays enforced.

Acceptance: on Android **arm64**, download a public package from the registry,
verify hash, execute `python/python`, start the selected HTTP server, receive a
real response through `127.0.0.1` from the device browser/test client, and stop it
cleanly. Record device/OS/ABI/package version/digest and exact build revision.
Implemented: live GraphQL search/resolve, WebC v3 digest/size validation, bounded
transitive graph installation, offline dependency loading, and an actual Python
3.13.20 execution/sandbox test on Android x86_64. The first output-capture attempt
timed out; deterministic draining fixed it and the unchanged guest checks passed.
A physical Android 15/API 35 arm64 phone passed the live Python suite using the
optimized Rust runtime (55.882 seconds for the whole test, including registry
work) and all three bridge regressions. The physical arm64 HTTP harness now returns
real responses, isolates simultaneous instances, and confirms forced-stop cleanup.
Cranelift cold/warm/process-memory measurements and clean release sizes are now
recorded; Singlepass only supports the tested core workload and rejects Python.
LLVM build/license prerequisites and further compatibility checks remain visible;
S0-2 is still in progress.
Evidence: [registry/Python verification](verification/2026-11-16-registry-python.md)
and [loopback HTTP verification](verification/2026-11-16-loopback-http.md).

Current measurements: [physical runtime report](verification/2026-11-16-runtime-measurements.md).
The default Cranelift compiler is MIT; optional Singlepass/LLVM 7.4.2 use separate
BUSL terms and require an owner/expert production licensing decision. No permissive
fallback or distribution approval is implied by a development experiment.

Compare supported compiler backends and measure release APK bytes, first-start
compile milliseconds, warm-start milliseconds and peak memory bytes. Measurements
must distinguish emulator from physical phone and debug from release builds.

## S0-3 — Curated web package inventory and Play policy validation (pending)

Depends on S0-2. Inventory web UI packages; accept recipes only with Android
evidence and reviewed licenses. Public catalog hosting and error report destination
remain owner decisions. Internal Play upload requires owner account, application
ID decision, signing setup, privacy/distribution review and explicit authorization.
The current `dev.wasmdroid.app` ID is provisional. No policy approval is implied.

## MVP engine prerequisites (in progress)

Tracked in [issue #3](https://github.com/gatgserv/wasmdroid/issues/3).
Implemented prerequisites: durable lifecycle metadata/events and stale-revision protection;
exclusive engine ownership/recovery; named volume data operations, read-only
leases, traversal/symlink confinement and journaled removal recovery. General
RuntimeDriver, actual guest mounts, Android service ownership and end-user
lifecycle remain pending. Experimental host interruption passes focused checks
but failed the combined phone suite (HTTP cleanup and CPU-loop exit). It is not
accepted as reliable. The owner clarified a common supervisor with one service
controller/execution group per package; stopping one service must stop all its
descendants and preserve other services. See the service supervision design.
Storage metadata is not evidence of guest execution.

See [the current storage and cancellation evidence](verification/2026-11-16-lifecycle-storage.md)
and [the service supervision design](design/service-supervision.md).

## Remaining MVP

Lifecycle driver/supervisor, long-lived engine Foreground Service, safe network
manager, volumes, registry install/uninstall progress, compiled module cache,
Store/My Apps/details/settings, embedded web launch, opt-in advanced mode,
rotated logs and terminal. Define each as an acceptance-tested story. Full live
E2E runs at acceptance completion; targeted story checks stay separate.
Do not start Docker/iOS implementation in this phase.
