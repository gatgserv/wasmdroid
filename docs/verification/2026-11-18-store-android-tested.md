# Store Android test labels

The runtime-enabled APK was built from
`028ec371f06483a3640a5e94ae84382268c6c296` with ARM64 and x86_64 libraries,
API 28 minimum and API 37 target, and the release-profile Cranelift runtime.

- App APK SHA-256: `02f61fc21ee0bf4db5b1d9f425c469487195ba1d5d1b54f21274d74aec35f37b`.
- Test APK SHA-256: `ee0bfeae8d1872ac16229414aa420ec310f98bdbb55b44f4ed668b4330ed22cf`.
- Android 17 (API 37, x86_64): **23/23** Store search state and presentation
  tests. The ranking regression covers an exact tested release arriving on a
  later page, a newer untested release of the same package, and stable
  Wasmer ordering within each group.
- The Android 17 Store searched the live registry for `python` and
  `static-web-server`. Both queries rendered the corresponding exact tested
  release first, with **Tested on Android** and **Download** on the same card.
  The downloaded cards also showed the label in the visual smoke check.
- Android lint found **no issues**. The existing AGP Gradle 11 visibility
  deprecation still appears in the build. Host formatting, 292 engine and
  FFI tests, and Clippy with warnings denied all passed.

## Label evidence and ranking

| Exact Wasmer Registry release | Successful Android run |
| --- | --- |
| `wasmer.io/python/python@3.13.20` | [Sony standalone HTTP UI acceptance](2026-11-18-android-ui-acceptance.md) |
| `wasmer.io/wasmer/static-web-server@2.44.0` | [Sony standalone Static Web Server in WebView](2026-11-18-agp-and-service-form.md#sony-arm64-catalog-package-investigation) |

Store applies the local test evidence to matching release references returned by
Wasmer Registry and to downloaded cards. The result list puts the labeled
releases first across all loaded pages. Downloaded cards retain lexical
reference order.

The public text search emitted six rows each for `python` and
`static-web-server`, omitting both tested versions. The exact-version query
found both releases public and available. For a matching tested package
name or reference, the first page can request that exact release directly
from Wasmer. The reply must match the exact reference and be public and
unyanked. The optional lookup uses one row of the existing page limit and
keeps the provider's original search cursor. If the lookup is unavailable,
the already returned text-search rows are kept. The label records a successful
run with the tested setup. The installer's integrity and permission gates are
independent.

## Reproduction

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial emulator-5596 -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.RegistrySearchPresentationTest,dev.wasmdroid.app.RegistrySearchStateTest' -TimeoutSeconds 240
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The new APK's device verification targets the isolated Android 17 emulator.
Physical Android 15 and Android 9 execution of this APK and remote CI were
outside this verification. Prior evidence above covers the actual guest runs.
