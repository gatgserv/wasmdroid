# Android lint cleanup

The local debug APK and instrumentation APK were assembled from
`ac94f129479c4e225bf7479acde0c470b77166c0` on
`codex/wasmer-registry-store`. Gradle packaged the release-profile Cranelift
runtime for ARM64 and x86_64 in one debug-signed APK. The app APK's SHA-256
was `cae4d64fd2a8636916cf572723cc44c55da04fa63f66f24e8047e9873364c306` and
the instrumentation APK's was
`bfb921fe035a5ec14b2e6dc453bd200e5df110df27f71faa218b52ba628a4d6e`.

`lintDebug` reported **0 errors and 1 warning**, down from 93 warnings in the
[integrated suite](2026-11-18-integrated-android-suite.md). APK inspection
confirmed both runtime ABIs, minimum API 28, target API 36, the adaptive icon,
and the system backup settings. The app APK passed 16 KiB zip alignment and
debug signature verification.

The cleanup removed 56 unused strings and corrected the group count labels,
WebView feature guards, clipboard compatibility, and Android UI and packaging
findings. Transfers retain the 16 MiB reserve and use space available to the
app without requesting cache eviction. Android system cloud backup and device
transfer exclude private app data. The user-directed document export remains
available.

Lint exceptions are local to behavior the app needs. The manifest declares its
single English locale for the system locale picker on Android 13+. The launcher
hosts the widget background, so the app window theme does not draw behind it.
Guest sites use JavaScript inside their per-app WebView profile without an
Android JavaScript bridge. Browser data erasure checks the result of writing
its synchronous receipt. The terminal retains the legacy composed-text event
path for Android 9.

## Remaining Android SDK warning

`OldTargetApi` remains active. Moving to API 37 requires a separate Android 17
compatibility pass. On API 37 the host must handle Android's runtime LAN
permission alongside WasmDroid's independent guest permissions. Acceptance
needs grant, deny, and revoke scenarios for guest outbound access and inbound
Wi-Fi sharing. Review the target-dependent native library loading behavior and
verify it with the runtime APK. API 37 compilation requires AGP 9.1.1 or later.
[Android 17 target behavior](https://developer.android.com/about/versions/17/behavior-changes-17)
· [AGP platform compatibility](https://developer.android.com/studio/releases)

## Test commands

```powershell
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-device> -Abis <abi> -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <class-group> -TimeoutSeconds 600
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.NotesRecipeAcceptanceTest -NotesLive -TimeoutSeconds 600
```

Windows engine and FFI tests passed 292/292. Formatting, Clippy with warnings
denied, and the Wasmer manifest isolation check passed. Android 9 (API 28,
x86_64) passed seven instrumentation cases and opened the Wasmer Registry Store
from the installed APK. Android 16 (API 36, x86_64, 16 KiB pages, WebView
133.0.6943.137) passed all nine selected runtime, profile isolation, and
per-profile erasure cases in 13.088 seconds. Android terminated the first
instrumentation process for low memory while the AVD was starting. The
complete rerun passed after the AVD settled, using the same APKs.

The Sony G8142 (Android 15/API 35, ARM64, 4 KiB pages) passed 268/268 cases
across the quick (255), benchmark/cancellation/HTTP/registry (6), supervised
services (6), and live Notes (1) groups. The Sony and Android 16 AVD hashes
matched the local APK. The three targets covered all 270 distinct Android
instrumentation cases with no skips in the final runs.

The [integrated report](2026-11-18-integrated-android-suite.md) records the
standalone ARM64 native driver run. Remote CI for this revision, release
signing, and Android 17 device acceptance remain pending.
