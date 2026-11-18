# Android v1 registry Store on physical arm64

The two network regressions recorded below were subsequently fixed and retested. See the [network verification report](2026-11-18-network-regressions-arm64.md).

Verified on 2026-11-18. The application APK was built from `428e346accf2c84d9499f6b4c2a6891205190250` on `codex/wasmer-registry-store`. The test device was a physical Sony G8142 running Android 15, API 35, with the `arm64-v8a` ABI and Android System WebView `131.0.6778.200`.

The debug-signed runtime APK uses the Rust release profile with Cranelift. Its size is 101,102,089 bytes. SHA-256 is `95c522caadc20d452dabbc9088f1ad1972ae91b285a04697ccb826c50f2c318c`. `apksigner verify` passed. Package inspection verified that the arm64 FFI library was present and the bundled catalog asset absent. The installation used `adb install -r -t` through the selected-device test runner. The two connected emulators and existing application data were preserved.

## Commands and outcomes

```powershell
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\android\gradlew.bat -p android --offline :app:lintDebug -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
.\tools\test-android.ps1 -Serial <selected-physical-device> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <comma-separated-class-list> -TimeoutSeconds 600
```

- Gradle APK assembly and Android Lint passed on the listed source. Lint reported 0 errors and 93 warnings. Rust format, 292 engine/FFI tests and Clippy with warnings denied passed at `ceee0a4ff3bef12c46b665401b1d34df75cb7406`. The Rust source and lockfile are identical on the APK revision.
- The first instrumented group passed 28 tests. It selected `EngineBridgeTest`, `RegistrySearchPresentationTest`, `RegistrySearchStateTest`, `WebProfileIsolationTest`, `BrowserProfileCapabilityTest` and `WorkerResourcesTest` in `dev.wasmdroid.app`. The actual WebView provider kept cookies separate for profiles using the same loopback address and allowed the original profile to be reopened.
- The second group passed three tests. `ServiceTimeoutAcceptanceTest` showed that a three-second timeout stopped its Python worker, the worker with a blank timeout continued, and generation-bound RAM and CPU measurements remained available. `SupervisedHttpAcceptanceTest` ran two isolated Python workers on guest TCP port 8080, received distinct HTTP 200 responses on separate Android host ports, and verified that each stop closed only its own listener. `RegistryProbeTest` verified and executed public Python WebC from the registry.
- The Store smoke test ran in normal mode. It searched the real Wasmer Registry for `static-web-server` and displayed `wasmer.io/baran/static-web-server@1.0.5`. The Store reported a verified installation of that package on the `88cc0cf` build. The APK revision then opened the exact installed reference with `Create service`, saved a new application and displayed it in My apps. The test-owned application was removed through the app. The downloaded package was retained in the store.

## Open verification findings

- On baseline `d861eba`, real registry calls reached Reqwest without a Tokio reactor. `61b192d` moved eager request construction inside the bounded executor and added a regression. The real Android registry and production-owner tests above passed after the change. See [issue #5](https://github.com/gatgserv/wasmdroid/issues/5).
- On `ceee0a4`, the single-server `HttpProbeTest` case passed. Its legacy two-server case reached both servers and confirmed the second survived the first stop, then the instrumentation process crashed with `SIGSEGV` while stopping the second. Symbolication placed the fault in the Wasmer VM signal-trap backtrace on a coroutine stack. The triggering signal was undetermined. The final product-path test exercised the same two-service requirement through separate Android worker processes and passed. The legacy test remains intact.
- `SupervisedLoopbackTest` on `ceee0a4` received the initial marker and then EOF after the 31-second idle read. The pinned WASIX 0.704.2 receive paths give an unset socket read timeout a 30-second fallback. The positive finite/unlimited service tests use separate runtime supervision state. The long-idle stream regression remains open.
- The phone's WebView provider supports multiple profiles and lacks `DELETE_BROWSING_DATA`. The two full profile-erasure cases on `d861eba` failed at that capability boundary. The final `BrowserProfileCapabilityTest` exercised the explicit rejection without changing profile state. Full erasure still requires a capable provider.
- This was a targeted physical Android run. Release signing, Play distribution, x86_64, API 28 device execution, visible resource sampling in a rendered running card, and full-product browser/rotation/accessibility and boot acceptance were outside this run. The approved first-version policy defers hard RAM/CPU caps and full WebView traffic mediation. Remote CI evidence belongs to the submitted commit.

## Follow-up on the remaining device checks

On 2026-11-18, the physical Android 15 arm64 phone reran the three classes below using the original `428e346accf2c84d9499f6b4c2a6891205190250` APK and its matching test APK. The application APK SHA-256 was unchanged.

```powershell
.\tools\test-android.ps1 -Serial <selected-physical-device> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.HttpProbeTest -TimeoutSeconds 300
.\tools\test-android.ps1 -Serial <selected-physical-device> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.SupervisedLoopbackTest -TimeoutSeconds 300
.\tools\test-android.ps1 -Serial <selected-physical-device> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.BrowserProfileCapabilityTest -TimeoutSeconds 300
```

- `HttpProbeTest` ran two cases in 33.986 seconds. The single-server case passed. The legacy same-process dual-server case failed during shutdown with `guest status finished but 1 execution tasks are still active`. This run completed instrumentation. The earlier native crash remains a separate observation.
- `SupervisedLoopbackTest` reproduced `EOFException` on the post-idle read at line 148 in 41.892 seconds. The existing long-idle requirement remains open.
- `BrowserProfileCapabilityTest` passed on the phone with WebView `131.0.6778.200`, exercising rejection without a profile mutation or erase receipt.

A separate x86_64 bridge-only build from an archive of the same source revision ran on a dedicated Android 16 API 36 emulator with `com.google.android.webview` version `133.0.6943.137`. Its app APK SHA-256 is `587f8816eef8b132a4b3df2f812c8fb907f9e6627076d21b1d97aeeb04ed3863`. This run exercises the WebView provider and profile state without the guest runtime.

```powershell
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PwasmdroidAbis=x86_64 -PwasmdroidRuntime=false -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <dedicated-emulator> -Abis x86_64 -RuntimeEnabled false -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.BrowserProfileErasureTest,dev.wasmdroid.app.WebProfileIsolationTest,dev.wasmdroid.app.BrowserProfileCapabilityTest' -TimeoutSeconds 180
```

All four emulator tests passed in 1.286 seconds. The provider acknowledged erasing the selected fixture profile, its cookie was cleared, and the other profile's cookie survived. An active view fenced erasure and cookies survived replacement of the same application generation before the authorized clear. This is evidence for the compatible-provider path. The physical phone exercised the unsupported path.
