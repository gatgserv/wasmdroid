# Delivery backlog

Local backlog while no remote repository/issue tracker is connected. The owner
selected Apache-2.0 on 2026-11-16. The public repository account is undecided and
the GitHub CLI is unauthenticated; no public repository or PR has been created.

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

## S0-2 — Registry → verified WebC → Android execution (pending)

Depends on S0-1. Inspect the current official registry GraphQL schema; resolve
immutable public package versions with hashes; bounded download into the content
store; execute Python and a simple web server from actual WebC commands, with
per-instance filesystem/network isolation. Do not expose unrestricted host
networking as a production shortcut. Runtime import boundary stays enforced.

Acceptance: on Android **arm64**, download a public package from the registry,
verify hash, execute `python/python`, start the selected HTTP server, receive a
real response through `127.0.0.1` from the device browser/test client, and stop it
cleanly. Record device/OS/ABI/package version/digest and exact build revision.
Compare supported compiler backends and measure release APK bytes, first-start
compile milliseconds, warm-start milliseconds and peak memory bytes. Measurements
must distinguish emulator from physical phone and debug from release builds.

## S0-3 — Curated web package inventory and Play policy validation (pending)

Depends on S0-2. Inventory web UI packages; accept recipes only with Android
evidence and reviewed licenses. Public catalog hosting and error report destination
remain owner decisions. Internal Play upload requires owner account, application
ID decision, signing setup, privacy/distribution review and explicit authorization.
The current `dev.wasmdroid.app` ID is provisional. No policy approval is implied.

## MVP (after Stage 0 exit)

Lifecycle driver/supervisor, long-lived engine Foreground Service, safe network
manager, volumes, registry install/uninstall progress, compiled module cache,
Store/My Apps/details/settings, embedded web launch, opt-in advanced mode,
rotated logs and terminal. Define each as an acceptance-tested story. Full live
E2E runs at acceptance completion; targeted story checks stay separate.
Do not start Docker/iOS implementation in this phase.
