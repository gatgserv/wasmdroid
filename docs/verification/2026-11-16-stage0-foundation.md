# Stage 0 foundation verification — 2026-11-16

**Code revision:** `55641f8d81d9085322df4653e0a9e09b355e603b`
(`feat: bootstrap Android engine and WASI probe`), branch `codex/stage0-foundation`.
The evidence/backlog commit following this revision changes documentation only.
S0-1 is complete; the product's full Stage 0 exit criterion is still pending.

## Environment

Windows 11; Rust 1.95.0 via rustup; Microsoft JDK 21.0.10; Android SDK 36;
NDK 28.2.13676358; Gradle 8.13; AGP 8.13.1; Kotlin 2.2.21;
UniFFI 0.32.2; Wasmer 7.4.2; WASIX 0.704.2; Cranelift backend.
Native library builds: `aarch64-linux-android` and `x86_64-linux-android`.
Device execution: dedicated `WasmDroid_Stage0` emulator, serial `emulator-5586`,
Android 16 / API 36, **x86_64**, 2 CPU cores and 2048 MiB RAM.
No physical Android arm64 execution is claimed.

## Exact checks

Commands ran from the repository root, with these environment settings:

```powershell
$env:JAVA_HOME = 'C:\Program Files\Microsoft\jdk-21.0.10.7-hotspot'
$env:ANDROID_HOME = Join-Path $env:LOCALAPPDATA 'Android\Sdk'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
& $cargo fmt --all --check
& $cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
& $cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
& powershell -NoProfile -ExecutionPolicy Bypass -File tools/test-android.ps1 -Serial emulator-5586 -Abis 'arm64-v8a,x86_64' -RuntimeEnabled true
& ./android/gradlew.bat -p android :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true
```

| Check | Result |
| --- | --- |
| rustfmt | PASS |
| Rust regression tests | PASS — 15 tests, 0 failures, 0 ignored |
| Clippy, all targets of engine/FFI, warnings denied | PASS |
| Dependency boundary check from CI workflow | PASS — no Wasmer dependency outside driver |
| Android application/test APK build, both ABIs, runtime enabled | PASS |
| Android native integration | PASS — 3 tests, 0 failures, 0 skipped; ADB process exit 0 |
| Android lint | PASS — 0 errors, **9 warnings**; no suppressions/baseline added |
| Compose activity cold launch | PASS — `am start -W`, status `ok`; engine v0.1.0 / API 1 visible |

Regression coverage includes SHA-256 known vector and digest parsing; corrupt
download isolation; partial-file cleanup and independent size limits; cache
corruption detection/repair; registry-qualified references; recipe defaults,
mount/port validation, unknown-field rejection; catalog version/ID/URL/web-launch
validation and evidence requirement; lifecycle terminal states; persisted SQLite
metadata and rejection of a newer database schema without downgrade.

Device tests load the actual packaged `.so` using generated Kotlin bindings,
validate engine identity and the requested runtime build flag, validate an empty
catalog, propagate a typed Rust error to Kotlin, and compile/execute the built-in
WASI `fd_write` fixture. It returns exactly `Hello from WasmDroid!\n`.
See [instrumentation output](2026-11-16-native-tests.txt) and
[activity screenshot](2026-11-16-android-screen.png).
The suite's reported 0.148 seconds is test duration, not a package performance benchmark.

## Debug artifact measurement

Artifact: `android/app/build/outputs/apk/debug/app-debug.apk`.
Size: **180,680,136 bytes**, measured from the built file; it contains both ABIs,
the debug-profile Rust runtime and a debug-signed Android application.
SHA-256: `89ce48796d2ad34dd181bcdce3fc0fcb2651e1e0d419d01b2b1e7a4968789ded`.
This is not the release APK/AAB size. No first-start, warm-start or peak-memory
performance claim is made. The in-app screen is a technical laboratory, not MVP UI.

## Remaining lint warnings

- `OldTargetApi`: target SDK 36 is not the newest available SDK.
- `AndroidGradlePluginVersion`: a newer Gradle version is available.
- Two `GradleDependency`: newer Compose BOM and Activity versions are available.
- Two `NewerVersionAvailable`: newer coroutines and JNA versions are available.
- `ChromeOsAbiSupport`: lint reports missing x86_64; the tested APK is built with
  both arm64-v8a and x86_64. This static warning remains unresolved.
- `DataExtractionRules`: explicit Android 12+ backup/transfer rules are pending.
- `MissingApplicationIcon`: launcher artwork is pending.

Gradle also reported that it could not strip `libjnidispatch.so` and packaged it
unchanged. No quality gates were disabled to obtain this result.

## Bootstrap failures resolved and test-host limitation

The initial reference validator accepted `../python`; the regression test now
rejects it. UniFFI 0.32 requires global `--config` sections, and an FFI error field
named `message` collided with Kotlin `Throwable.message`; both were corrected.
The Windows standalone Rust MSI shadowed rustup's installed Android targets;
the build script now prefers rustup proxies. Native Windows Cranelift execution
is unsupported upstream; Android cross-compilation and actual Android execution
were used instead. The initial PowerShell process wrapper lost `ExitCode` after
process termination; retaining the handle fixed it without dropping exit checks.

An initial Gradle `connectedDebugAndroidTest` attempt inventoried an unresponsive
unrelated emulator, then produced no test results on the selected existing
device. That attempt was canceled and is **not** counted as a passing run.
The passing runner uses direct `adb -s` instrumentation on a separate project-owned
AVD; existing AVDs were not restarted or wiped. Only the dedicated emulator is
stopped after verification; its project-local data is retained for reuse.

## Pending acceptance and external prerequisites

- Registry GraphQL resolution/download and real WebC execution; Python package;
  server sockets and real loopback HTTP response; lifecycle/background service.
- Physical arm64 execution; release size/startup/memory measurements; compiler comparison.
- Real WASIX socket/thread/signal behavior; filesystem/network policy enforcement
  for arbitrary packages; production resource limits. The finite fixture does not verify these.
- Web-package inventory and reviewed licenses; curated catalog remains empty.
- Full dependency license/notice bundle and Play policy/signing/distribution work.
- Remote CI, PR and public repository: no remote is configured and GitHub CLI is
  unauthenticated. The repository owner/account still needs to be selected.
- Native Linux probe and CI jobs are prepared but were not executed on this Windows host.

No full browser/real-stack E2E, production deployment, merge or Play upload occurred.
