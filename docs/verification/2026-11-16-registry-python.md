# Registry installation and Android Python verification — 2026-11-16

**Tested application code:** `9864ac482ef3c30e8a032af9a5f41944dae9fe00`.
The evidence-only follow-up changes documentation, not application code.
Tracking: [issue #1](https://github.com/gatgserv/wasmdroid/issues/1),
[PR #2](https://github.com/gatgserv/wasmdroid/pull/2).
This is partial Stage 0 evidence: real loopback HTTP and the complete measurement
matrix are still pending. It does not complete the continuing Android product goal.

## Verified behavior

- Real public GraphQL search and resolution use the current registry schema.
  Resolution selects WebC v3, pins version/digest, rejects private/yanked versions,
  checks identity, and handles SemVer build metadata and dependency constraints.
- Downloads require HTTPS, have connect/request deadlines and a metadata size cap,
  and do not persist or print temporary download URLs. All execution blobs are
  SHA-256 verified. Metadata for the complete dependency graph is published in one
  SQLite transaction; partial/corrupt downloads do not publish installed images.
- Limits: 512 MiB per blob, 1 GiB per dependency graph, 32 packages, depth 16.
  Existing cache entries are reverified; corrupt entries are repaired by download.
  These are download bounds, not guest memory/CPU enforcement.
- The offline runtime resolver sees only the explicitly installed verified set.
  It cannot fetch a missing dependency or silently open an unrelated container.
- Python runs a fixed regression script: guest `/tmp` write/read succeeds;
  `/system/build.prop` and the test-owned host sentinel are not visible; a guest
  socket operation must fail with an unsupported/access-denied errno. Connection
  refusal alone is not accepted as proof of denied networking.
- stdout/stderr retain a bounded 64 KiB tail with truncation indicators. The first
  live attempt timed out waiting for retained descriptor clones; draining stops
  after execution and queued bytes are consumed, independently of reference counts.
  A native regression keeps a writer alive and requires bounded completion.

## Package evidence

| Package | Version | WebC SHA-256 | Bytes |
| --- | --- | --- | ---: |
| Python | 3.13.20 | `a9fa8202f1bf6a4eca8d31ee5f2b1970c8f45af5c5520cc954d28a133333d8e7` | 61,744,083 |
| Bash | 1.0.25 | `059606d132e2e6bc1afe3b432ee64dcb1b1b059815c8bb213cf3b24798ef21e1` | 1,870,786 |
| Coreutils | 1.0.27 | `35028c4e6b9fa8cccdffdcab2007d1df3af31b3f2b22fb890d022d7ae723bbd3` | 12,703,522 |
| Static web server (download only) | 2.44.0 | `1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08` | 4,755,156 |

Python depends on Bash, which depends on Coreutils. The public server was downloaded
and verified on the workstation; no server execution/HTTP success is claimed here.
These packages are not bundled into the APK or marked Android-verified in the catalog.
Registry-provided license metadata is retained; it is not a completed licensing audit.

## Checks and device results

| Check | Result |
| --- | --- |
| Host Rust tests | PASS: 3 registry + 15 contract + 6 graph/transaction regressions |
| Core Clippy, warnings denied | PASS |
| Android arm64 driver + Wasmer-enabled FFI Clippy, all targets | PASS |
| Formatting / diff whitespace | PASS |
| Native API 36 x86_64 Python suite, Rust debug profile | PASS: 1 test, 120.244 seconds |
| Physical Sony G8142, Android 15/API 35, arm64, 4096-byte pages | PASS: real Python suite, 55.882 seconds on first phone run |
| Combined phone bridge + Python checks on `9864ac4` | PASS: 4 tests, 0 failures, 0 skipped, 36.317 seconds; ADB exit 0 |
| Android lint, optimized arm64 library | PASS: 0 errors, 9 warnings, no baseline/suppressions |
| Proposed UI scheme interactions | PASS: DOM checks for navigation, install/start/open/stop, advanced-mode toggle |

The physical-device APK is Android debug-signed with **Rust release-profile** code.
Measured APK size: **57,111,854 bytes**; SHA-256:
`3676b5530fc1d79a9884b221f159646cff2cc2a128fc463dfde23d147a6b5205`.
It contains only arm64. Do not compare this directly to the earlier dual-ABI debug
runtime artifact as an optimization ratio. Suite duration includes metadata,
verification and test overhead; it is not a pure compilation/startup benchmark.
Repeat runs reused WebC downloads, not a persistent compiled-module cache.

The 9 lint warnings remain the dependency/target upgrades, static ChromeOS ABI warning,
backup extraction rules and missing launcher icon recorded in the foundation report.

## Commands

From the repository root, with JDK 21 and the standard Android SDK location:

```powershell
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
$env:JAVA_HOME = 'C:\Program Files\Microsoft\jdk-21.0.10.7-hotspot'
$env:ANDROID_HOME = Join-Path $env:LOCALAPPDATA 'Android\Sdk'
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo run --locked -p wasmdroid-engine --example registry -- search python
cargo run --locked -p wasmdroid-engine --example registry -- pull wasmer.io/python/python .local/registry-spike
cargo run --locked -p wasmdroid-engine --example registry -- pull wasmer.io/wasmer/static-web-server .local/registry-spike
```

The arm64 Clippy command uses the same NDK clang/ar/linker environment as
`tools/build-native.ps1`:

```powershell
cargo clippy --locked -p wasmdroid-wasmer-local -p wasmdroid-ffi --features wasmdroid-ffi/wasmer --all-targets --target aarch64-linux-android -- -D warnings
```

`WASMDROID_TEST_DEVICE` below identifies the connected phone; its hardware serial
is kept out of repository artifacts. The emulator suite used `emulator-5586`.

```powershell
.\tools\test-android.ps1 -Serial $env:WASMDROID_TEST_DEVICE -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -TestClass 'dev.wasmdroid.app.EngineBridgeTest,dev.wasmdroid.app.RegistryProbeTest' -TimeoutSeconds 600
.\android\gradlew.bat -p android :app:lintDebug -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release
```

See the [physical-device instrumentation output](2026-11-16-phone-native-tests.txt).

## Remote workflow and limitations

The owner approved initializing `main` **only** at baseline `63db389`; all new work
is on `codex/stage0-foundation` through PR #2. No merge/default-branch update beyond
that approved initialization was performed. The baseline CI's engine jobs and
Linux WASI probe passed; its Android setup failed because the action's default
obsolete SDK `tools` package was unavailable. The PR explicitly selects supported
`platform-tools`, retaining SDK/NDK installation and all build/lint checks. See the
[recorded finding](https://github.com/gatgserv/wasmdroid/issues/1#issuecomment-6926608026).
Live checks on the current PR head are the authoritative remote CI evidence.

No full real-stack/browser E2E, Play upload, server acceptance, runtime resource
limits, generalized lifecycle supervision, foreground service, or product GUI
implementation is claimed. The proposed GUI flow is shown before implementation.
