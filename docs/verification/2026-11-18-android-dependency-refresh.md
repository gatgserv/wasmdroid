# Android dependency refresh

The APKs were built from `47fbedc26ddcb4303de5b03c1737363d828d3043` on
`codex/android-api-37`. This update addresses the six version advisories
recorded during the [Android 17 migration](2026-11-18-android-17-api-37.md).

| Component | Previous | Verified |
| --- | --- | --- |
| Gradle wrapper | 9.4.1 | 9.9.1 |
| Jetpack Compose BOM | 2025.10.01 | 2026.09.00 |
| Activity Compose | 1.11.0 | 1.13.0 |
| AndroidX WebKit | 1.14.0 | 1.18.0 |
| Kotlin coroutines for Android | 1.10.2 | 1.11.0 |
| JNA Android AAR | 5.18.1 | 5.19.1 |

The Gradle distribution and generated wrapper JAR use the published
checksums. Gradle 9.9.1's tested AGP range covers AGP 9.2.1. The selected
AndroidX artifacts' minimum SDKs fit API 28, and Compose 1.12.1 fits the
project's compile SDK 37. [Gradle checksums](https://gradle.org/release-checksums/)
· [Gradle compatibility](https://docs.gradle.org/9.9.1/userguide/compatibility.html#android)
· [Compose BOM](https://developer.android.com/develop/ui/compose/bom)
· [JNA 5.19.1 changelog](https://github.com/java-native-access/jna/blob/5.19.1/CHANGES.md#release-5191)

## Build and packaging

`assembleDebug`, `assembleDebugAndroidTest`, and `lintDebug` passed.
Android lint reported **no issues**. Gradle separately reported
`Configuration.setVisible(boolean)` deprecations from the Android Gradle
plugin for the future Gradle 10 transition. The Android packager retained
JNA's native library as supplied when symbol stripping was unavailable.
The native cross-build also emitted four existing warnings from the
vendored `wasmer-wasix` crate.
Both ABI variants of JNA and the engine passed 16 KiB ELF segment
alignment checks, and both APKs passed 16 KiB ZIP alignment and v2
signature verification. The app package reports minimum API 28, target
API 37, and ARM64 and x86_64 native libraries.

| Artifact | SHA-256 |
| --- | --- |
| App APK | `ee681e12950d41e8c3e6571ecfdff6e7bc75533f3710d9010e6ffa9f861fa076` |
| Instrumentation APK | `2ed7fb65222db3ce23a78b5d5f5b4029af82fefdd3026d0e29e81be07f84a455` |

Windows Rust engine and FFI passed 292/292, along with formatting,
Clippy with warnings denied, and the Wasmer manifest isolation check.
On the Android 9 x86_64 AVD, the app APK's
hash matched and **24/24** focused tests passed in 9.837 s. The Store
opened and displayed the existing Wasmer Registry interface.

On the x86_64 Android 16 AVD with a measured 16,384-byte page size, the
same APK passed **9/9** runtime and WebView cases in 19.621 s. The run used
WebView 133.0.6943.137 and included actual JNA/UniFFI loading, registry
Python, supervised HTTP, and profile isolation and erasure.

## Android 17 network regression

The full instrumentation rerun passed **270/270** distinct tests on the
Android 17 x86_64 AVD with 4 KiB pages and WebView 145.0.7632.218. Runs were
quick 255/255 (8.528 s), benchmark/cancellation/HTTP/registry 6/6 (33.132 s),
supervised 6/6 (90.927 s), profile erasure 2/2 (0.692 s), and live Notes
1/1 (18.035 s).

The Android 17 x86_64 AVD ran the existing Wasmer Registry Python 3.13.20
fixture under the new APK. The Internet-only setting permitted public DNS
and TCP and blocked the private targets with the Android LAN permission
both denied and granted. In LAN-only mode the expected private name
resolved and Python read HTTP 200 and the entire 27-byte body while
`socket.settimeout(6.0)` remained set. The measured write-to-body interval
was 28 ms. Public DNS and TCP were blocked. Rejecting the Android LAN
permission left the guest LAN Save button disabled.

With both guest permissions off, public and private DNS/TCP were blocked.
The Wi-Fi sharing dialog rendered an accessible QR code. The second AVD
on the shared virtual Wi-Fi fetched HTTP 200 and the expected fixture.
Wi-Fi loss, explicit Stop, and Android LAN permission revocation each
made the old address inaccessible. Reconnecting Wi-Fi left sharing off
until the user started it again.

## Commands

```powershell
.\android\gradlew.bat -p android :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-device> -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <class-group> -TimeoutSeconds 600
.\tools\test-android.ps1 -Serial <selected-api-37-avd> -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.NotesRecipeAcceptanceTest -NotesLive -TimeoutSeconds 600
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The Android 17 interface procedure used the same isolated app and peer
AVDs as the [original API 37 verification](2026-11-18-android-17-api-37.md).
The serial-targeted UI path exercised the real Android permission
dialog, the running Wasm guest, and the Wi-Fi peer. Physical API 37 and
remote CI remain pending.
