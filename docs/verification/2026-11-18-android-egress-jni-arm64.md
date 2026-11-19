# Android guest DNS and TCP after the JNI stack fix

Tested `6f66ae14876d60f18ba596d09b2ea1de025e37b8` on the selected Sony G8142,
Android 15/API 35, `arm64-v8a`, with 4,096-byte pages. The test APK has the
Cranelift runtime, the Rust release profile, and debug signing. Installed and
local APK SHA-256 both matched
`5cb143156472210bd3315b6277870672c66466dc49702b67425199a56770e34f`.

The previous physical DNS and emulator raw TCP failures were in ART's thread
attachment path. Guest-side capture state now reads a native revocation signal.
The actual Android Network calls still execute as retained Tokio blocking work
with provider checks before and after DNS and bind. The FFI wrapper transfers
foreign callback destruction to a native OS thread. This covers the case where
an old capture's last reference is dropped by a guest fiber.

See [issue #7](https://github.com/gatgserv/wasmdroid/issues/7) and the
[previous reproduction](2026-11-18-android-ui-acceptance.md).

## Physical UI test setup

The stopped service was configured in **My apps → Network access**. Each run
used an existing verified `wasmer.io/python/python@3.13.20` package, a Python
fixture in a test-owned volume imported through the Android document picker,
and test buttons in the app's embedded WebView. The public test resolved
`example.com` and opened a separate raw TCP connection to a public IPv4 address.
The private DNS test looked up a test name with a fixed RFC1918 answer. The
private TCP test used a numeric workstation address and an ephemeral HTTP
listener. A direct Sony shell probe received the marker before the guest tests.
The server also identifies requests made by the guest fixture.

| Saved permission | Public DNS | Private DNS | Public numeric TCP | Private numeric TCP |
| --- | --- | --- | --- | --- |
| Internet on, LAN off | Resolved public addresses | No authorized answer | Connected | Denied |
| LAN on, Internet off | No authorized answer | Resolved the expected RFC1918 address | Denied | Connected and received the controlled peer's HTTP marker |

The private transfer used a blocking guest socket with a six-second
`SO_RCVTIMEO` after connect. After the phone's Wi-Fi was disabled, a new LAN
connection failed normally. After Wi-Fi returned, the guest again received the
peer marker in the same worker process. The service stayed alive in both
permission modes. The Android crash buffer recorded zero new WasmDroid worker
crashes during the test interval.

Two initial private HTTP attempts using Python `socket.settimeout(6)` throughout
timed out after numeric connect with an active worker. That fixture reported
send and receive together. The blocking retest recorded the peer's accepted
connection, sent marker, and guest readback. [Issue #8](https://github.com/gatgserv/wasmdroid/issues/8)
tracks the separate timed-socket behavior.

The test fixture was stopped, its Internet and LAN flags were returned to off,
and the phone's original Wi-Fi, mobile data, and display rotation were restored.

## Build and automated checks

- Gradle assembled the matching app and test APKs in 35 s. The Rust release
  build reported four pre-existing warnings in the vendored WASIX crate.
- The serial-targeted Android runner passed the three `EngineBridgeTest` cases
  in 0.297 s, including execution through the exact packaged runtime.
- The ARM64 Rust test executable ran directly on the same Sony in an isolated
  temporary test directory. All six Android adapter tests and all 13 regrant
  tests passed. They cover the native guest-side check, provider loss, late
  success refusal, descriptor and owner retention, and replacement and Stop
  fencing. These tests use controlled provider fixtures.
- The Windows host engine and FFI suites passed all 292 tests. Rust formatting,
  Clippy with warnings denied for those host crates, and the Android ARM64 driver
  test-target check passed.

```powershell
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.EngineBridgeTest
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo check --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --tests
cargo test --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --lib --no-run
adb -s <selected-phone> shell 'TMPDIR=<test-directory> <test-binary> --test-threads=1 service_egress_android::tests'
adb -s <selected-phone> shell 'TMPDIR=<test-directory> <test-binary> --test-threads=1 service_network::egress_regrant_tests'
```

Remote CI on this revision remains pending.
