# Integrated Android and Rust verification

The app and instrumentation APKs were built from `e553dd945141d14cd946e150badc62767ee394de`
on `codex/wasmer-registry-store` with the Rust release profile, the Cranelift
runtime, and Android debug signing. The installed APK hashes matched the
respective local outputs on all three targets.

| Target | Runtime build | SHA-256 |
| --- | --- | --- |
| Physical Sony G8142, Android 15/API 35 | ARM64, 4 KiB pages | `462e69c961e37340040652e3e515401de2f3ac832ab1ee7c9c0a5997c6a97044` |
| Android 9/API 28 AVD | x86_64 | `2a3fe6f14514f66d50db0c859493d57173397703f2c94588bed48a07f11ef498` |
| Android 16/API 36 AVD | x86_64, 16 KiB confirmed by `getconf PAGE_SIZE`, WebView 133.0.6943.137 | Same x86_64 APK |

## Automated suites

| Group | Result |
| --- | --- |
| Windows Rust engine and FFI | 292/292, with formatting, Clippy `-D warnings`, and Wasmer manifest isolation passing |
| Android Gradle ARM64 and x86_64 builds | App and instrumentation APKs assembled on both ABIs |
| Android ARM64 lint | 0 errors, 93 warnings |
| Sony Android quick group | 255/255 across 50 classes, 13.477 s |
| Sony Android benchmark, cancellation, HTTP, and registry group | 6/6 across five classes, 106.857 s |
| Sony Android supervised services group | 6/6 across six classes, 127.272 s |
| Sony live Notes acceptance | 1/1 through the explicit `-NotesLive` runner, 27.554 s |
| Android 9 AVD runtime | 5/5, including verified registry Python and private HTTP, 21.980 s |
| Android 16 16 KiB AVD runtime and WebView | 9/9, including two compatible-provider erasure cases, 29.591 s |
| Full ARM64 native driver on Sony | 82/82 on the corrected fixture revision, 2.94 s, using the debug application's isolated cache and `run-as` |

The instrumentation suite covered all 270 distinct Android cases in 63
classes without skips on their applicable targets. The two provider-dependent
erasure cases ran on the Android 16 AVD. Counting the repeated runtime and
profile checks on both emulators, Android instrumentation passed 282 times.
The final host, native, and Android runs passed all 644 distinct automated cases.

## Initial native failure and rerun

The first complete driver run on `e553dd9` passed 81/82. The synthetic positive
WebC import used the engine's CAS hex encoding for an atom signature. WebC's
metadata parser expects the SHA-256 digest in its base64 signature encoding.
`95d0c1fe963f3d7153efc11d9732485988fbda12` corrects the test fixture's positive
and deliberately incorrect signatures using WebC's typed encoder. The
production validator and guest runtime were unchanged. The full Android native
test binary rebuilt from that revision passed 82/82, including raw Wasm and
WebC import and offline execution through the shared loader.

## Interface checks

- Sony Store found matching public Wasmer Registry results. With Wi-Fi disabled
  and mobile data already off, the Store displayed the connection error. The
  original network state was restored and **Retry search** returned live results.
- The Sony's LAN-only Python service received HTTP 200 and the complete 27-byte
  response in 45 ms with `socket.settimeout(6.0)` still set. Both consecutive
  writable `select` calls passed. Public TCP was denied. After Wi-Fi was
  restored, the same worker received the complete timed response again in 45 ms.
  The fixture ended stopped with both network permissions off.
- The Android 9 AVD rendered the Wasmer Registry Store interface.
- On the Android 16 AVD, the WebView 133 renderer exited with `SIGSEGV` in
  Trichrome/ART's background metrics path during an initial interface attempt.
  WasmDroid displayed the page error and kept the service running. Reopening
  the same page worked. The UI then saved a browser marker, acknowledged
  **Manage data → Clear web data**, and showed the marker missing after the next
  start. The counter in the named volume advanced from 12 to 13.

## Commands and coverage

```powershell
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo test --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --lib --no-run
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-device> -Abis <abi> -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <class-group> -TimeoutSeconds 600
.\tools\test-android.ps1 -Serial <selected-phone> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.NotesRecipeAcceptanceTest -NotesLive -TimeoutSeconds 600
adb -s <selected-phone> shell "run-as dev.wasmdroid.app sh -c '<isolated-native-test-command>'"
```

The x86_64 build used a clean archive and the same Gradle assembly flags with
`-PwasmdroidAbis=x86_64`. The native runner executed all test names with
`--test-threads=1`. This round used the existing Windows host and Android
targets. WSL failed to start because the host compute service was unavailable.
Release signing, a full TalkBack pass, physical ARM64 16 KiB execution, and
remote CI on the tested source remain separate coverage. The existing remote
checks for [PR #2](https://github.com/gatgserv/wasmdroid/pull/2) target
`23455dce2a8b2eef5fc22cf06426c81115f2f485`.
