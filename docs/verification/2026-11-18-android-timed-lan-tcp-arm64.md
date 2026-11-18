# Android timed LAN TCP readiness

[Issue #8](https://github.com/gatgserv/wasmdroid/issues/8) reproduced on the
physical Sony G8142, Android 15/API 35, `arm64-v8a`, with Cranelift and the Rust
release profile. Baseline `6f66ae14876d60f18ba596d09b2ea1de025e37b8` used the
debug APK with SHA-256 `5cb143156472210bd3315b6277870672c66466dc49702b67425199a56770e34f`.

## Failure on the baseline APK

The stopped worker was granted LAN only through **My apps → Network access**.
A Python 3.13.20 WASIX service opened an IPv4 socket to a controlled workstation
peer. The phone shell received the expected peer marker as a reachability
control. The guest called `socket.settimeout(6.0)` before `connect`, then kept
that mode through `sendall` and HTTP parsing and body readback.

- The peer accepted the TCP connection. The guest timed out in `sendall` at
  6,035 ms. A separate connection reported zero writable descriptors in each of
  two consecutive `select` calls.
- Public numeric TCP was denied. The blocking control received HTTP 200 and the
  complete 27-byte expected body. The same worker survived the probes.

## Correction and source provenance

The driver's bounded TCP pairs now report writable capacity on consecutive
polls while space remains. A full buffer still registers the writer to wake
when the peer reads. A separate defect in the same buffer affected short writes
at capacity. The accepted prefix now follows the shared readable-notification
path and wakes the waiting peer.

The readiness and partial-write fix is `9306e89ea768490bfbe20ef0550d06a86262dbe9`.
The broader suite exposed two existing failures in LAN ingress and TCP egress
half-close. The boxed `AsyncRead` implementation reset the total filled length
after a read into the end of a Tokio buffer. An EOF read could discard prior
bytes. The follow-up preserves the filled prefix and advances the read position.
The final source is `d80d484ca4286bb6dcb4ddaf7eb19515b4fce8a5`. The debug-signed
ARM64 APK SHA-256 is `462e69c961e37340040652e3e515401de2f3ac832ab1ee7c9c0a5997c6a97044`.

`virtual-net` remains at version `0.704.2`. The original crate archive matched
the registry SHA-256 `c2d137b0323f902e4339d361f8f2ba3df44aed1c6cb1641ea785fee9824781e4`.
Commit `8d3ecc0` imports the published archive. The package declares source
commit `f154bd28c5ca6a9182f7dfbb1a0afb26d8c63637`. Its MIT notice is packaged
with the APK. [Pinned upstream license](https://github.com/wasmerio/wasmer/blob/f154bd28c5ca6a9182f7dfbb1a0afb26d8c63637/LICENSE)

## Final Sony UI retest

The phone's installed APK matched the final SHA-256. The same test fixture and
controlled peer exercised the UI with LAN on and Internet off.

| Probe | Result |
| --- | --- |
| `socket.settimeout(6.0)` from connect through `HTTPResponse.read()` | HTTP 200, all 27 expected bytes, 38 ms. `gettimeout()` returned `6.0` after body verification. |
| Consecutive writable `select` calls | Both calls returned the connected socket as writable. |
| Numeric public TCP | Denied at connect. |
| Blocking control | HTTP 200, all 27 expected bytes, 43 ms. |
| Wi-Fi loss and recovery | Private TCP was unavailable while Wi-Fi was off. On reconnection, the same worker fetched HTTP 200 and all 27 bytes with the Python timeout still at 6.0 s, in 49 ms. |

The same worker remained alive throughout the probes. The fixture was stopped,
both permissions were restored to off, and the original Wi-Fi, mobile data,
and rotation settings were restored.

## Build and automated checks

- Gradle assembled the final ARM64 APK and test APK in 41 s. APK inspection found the
  packaged runtime library and a byte-identical copy of the upstream MIT text.
  The local WASIX crate emitted four pre-existing warnings.
- `EngineBridgeTest` passed 3/3 on Sony. The Windows engine and FFI suites passed
  292/292. Formatting and host Clippy with warnings denied also passed.
- The final ARM64 native driver executable passed all 60 selected networking
  regressions in 2.23 s on Sony. These cover repeated writable polls and
  backpressure, partial-write notification, both half-close paths, the private
  Unix broker, loopback, the Android adapter, and regrant.
- The normal Android shell refused filesystem Unix socket bind. A scoped direct
  bind probe succeeded in the debug app context. The full native suite then ran
  through `run-as` in an isolated app-cache directory and passed the unchanged
  broker test. The test executable and temporary test directory were removed.

```powershell
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.EngineBridgeTest
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo test --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --lib --no-run
adb -s <selected-phone> shell "run-as dev.wasmdroid.app sh -c '<isolated-test-command>'"
```

The native executable was selected with
`--test-threads=1 service_network:: service_egress_android:: service_broker:: service_lan:: loopback::`.

Remote CI on the tested revision remains pending.
