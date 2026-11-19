# Android arm64 network regression follow-up

Tested on 2026-11-18 from `2cbcd6d8e05c0fa4149409b485b57dd1338d6c66`, branch `codex/wasmer-registry-store`. The selected physical Sony G8142 ran Android 15, API 35, with `arm64-v8a` and WebView `131.0.6778.200`. The APK was debug-signed with the Cranelift runtime built using the Rust release profile. The signed APK is 101,045,083 bytes and its SHA-256 is `153a5507655ac91a92d081e8f97b246dda8982a36e62a0f7aa15d8c6d1294b3c`. Signature verification and inspection of the ARM64 library and bundled WASIX MIT license passed.

## Changes

- The driver uses a local patch of the pinned WASIX 0.704.2 crate. For established TCP streams, `sock_recv` and socket-backed `fd_read` preserve the guest's optional receive deadline. Explicit durations and other socket-kind defaults retain their previous behavior. The crate import was validated against the original registry checksum and recorded separately. The workspace lockfile changes only the WASIX source, and the driver cache fingerprint includes the vendored Rust source.
- Legacy in-process HTTP probes now revoke their own virtual listeners and streams to wake blocked I/O. Guest-directed shutdown waits for both guest exit and zero active execution callbacks. The probe path no longer sends native interrupt signals to its execution threads. One probe's close remains local to that probe.

## Physical device verification

Each group used the fresh app and test APK on the selected phone. The original legacy HTTP and long-idle loopback tests are unchanged.

| Group | Cases | Instrumentation time | Outcome |
| --- | ---: | ---: | --- |
| `HttpProbeTest` | 2 | 33.611 s | Pass, including the original same-process two-server stop case |
| `SupervisedLoopbackTest` | 1 | 80.454 s | Pass, including the reply after 31 seconds without data |
| `SupervisedReceiveCancellationTest` | 1 | 38.200 s | Pass. After 32 seconds without data, Stop returned the same generation's terminal receipt and closed the client and host listener |
| `SupervisedHttpAcceptanceTest`, `ServiceTimeoutAcceptanceTest`, `RegistryProbeTest`, `EngineBridgeTest` | 6 | 62.087 s | Pass, including independent worker HTTP and real registry Python |
| `HttpProbeTest`, confirmation run | 2 | 33.597 s | Pass again |
| `EngineBridgeTest`, `RegistrySearchPresentationTest`, `RegistrySearchStateTest`, `WebProfileIsolationTest`, `BrowserProfileCapabilityTest`, `WorkerResourcesTest` | 28 | 1.844 s | Pass, including the phone's capability-dependent WebView behavior |

Total 40 successful executions across 35 distinct cases. The legacy probe ran twice and both groups containing the bridge class are counted in the execution total. Device installs used the existing serial-targeted runner and preserved application data.

## Commands and other checks

```powershell
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\android\gradlew.bat -p android --offline :app:lintDebug -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <fully-qualified-group-classes> -TimeoutSeconds 420
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo check --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --tests
```

The confirmation groups used 360 seconds per instrumented invocation. Android Lint reported 0 errors and 93 warnings. All 292 host engine and FFI tests passed, as did Rust formatting and Clippy. The Android driver test targets cross-compiled, including the new probe network isolation regression. The native driver unit binary was outside this physical run. The four warnings from the vendored WASIX crate were in upstream code. Explicit guest socket timeouts retain their values in the patched selector. Neither of the physical TCP tests sets that option.

See the [earlier failing runs](2026-11-18-registry-store-arm64.md) and [issue #1](https://github.com/gatgserv/wasmdroid/issues/1). This is local evidence for the exact build and the named device cases.
