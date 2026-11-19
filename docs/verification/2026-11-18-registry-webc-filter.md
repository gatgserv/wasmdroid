# Store compatibility for legacy WebC releases

The previous APK's Store displayed `This configuration is not supported yet`
after a package download attempt in the WordPress results on the Sony G8142.
The public Wasmer Registry returned WebC v2 for both displayed versions
`wasmer/wcgi-wordpress-demo@0.1.7` and
`randomduck/wcgi-wordpress-demo@0.1.2`, even when requesting distribution v3.
The official demo's command manifest declares the WCGI runner. The current
installer requires WebC v3 and stops at metadata resolution for these
distributions. WCGI execution is a separate runtime requirement.
[Wasmer WCGI runner](https://docs.wasmer.io/runtime/runners/wcgi/)

The Store search requests the actual WebC format using the existing Wasmer
Registry and cursor. Known v1 and v2 releases are hidden by default.
**Show incompatible releases** reveals them with the format explanation
and a disabled **Download** action. Missing metadata keeps the normal
installation checks available. The same data accompanies an exact release
lookup. A WebC v3 resolution failure from any installation entry point shows
a specific message about the package or its dependencies.

## Checked artifacts and tests

The runtime-enabled debug APKs were built from
`cb599784f74aa1915042be834f51bac858f91ad0`, with ARM64 and x86_64 libraries,
API 28 minimum and API 37 target.

- App APK SHA-256 `b6aad253b6ec3472301e925a837cc3c3341290828dd895409f647f3f03705058`.
- Test APK SHA-256 `8a06ed5c4f847468d49071e213c66eddc64a62bca465f85ab6202faadcda95e1`.
- Android 17, API 37, x86_64. **36/36** targeted registry search and
  diagnostic report tests.
- The real Store hid the WordPress releases by default. Revealing them
  displayed the WebC v2 explanation and a disabled **Download** action.
  A separate live search kept **Download** enabled and **Tested on Android**
  visible for `wasmer/static-web-server@2.44.0`, a WebC v3 release.
- Android lint found no issues. Formatting, **296/296** host engine and FFI
  tests, and Clippy with warnings denied passed. The app APK passed its
  signature and 16 KiB ZIP alignment checks.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial emulator-5596 -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.RegistrySearchPresentationTest,dev.wasmdroid.app.RegistrySearchStateTest,dev.wasmdroid.app.DiagnosticReportTest' -TimeoutSeconds 240
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The filter build was tested locally on Android 17. The Sony retained its
previous APK during this investigation. WebC v2 installation and WCGI execution
remain unsupported.

## Store search recovery on Sony

With the previous Store APK installed, submitting `flask` showed
`No packages matched` on the Sony, while `python` returned the tested release.
Wasmer's text search with the command filter returned zero package versions
for `flask` and `bash`. Its package-name index returned releases whose WebC v3
distribution manifests declare WASI commands. The Store uses that index when
the first text-search page is empty or contains only known legacy releases
and the alternative can improve it. The search form stays above the scrolling
Store results, including in landscape.

The runtime-enabled debug build used source
`8172b7378ca379e1228811dd34e99be69dadad50`, ARM64 and x86_64 libraries,
API 28 minimum, and API 37 target.

- App APK SHA-256 `826ad19e76cc882cfeb9ec9ff7023b2f964dd1aab4e6db91aac5c735a51c1438`.
  The installed base APK on the Sony matched this digest. Signature and
  16 KiB ZIP alignment checks passed.
- Test APK SHA-256 `8a06ed5c4f847468d49071e213c66eddc64a62bca465f85ab6202faadcda95e1`.
- Sony G8142, Android 15 / API 35, ARM64. The three targeted suites passed
  **36/36**. In landscape, the input and Search button were both visible.
  Live UI searches showed enabled downloads for
  `wasmer.io/wasmer/bash@1.0.25` and
  `wasmer.io/mayrelsuarezgomez/python-flask-server@0.5.3`.
- Two requests using the checked-in Wasmer name query for `flask` returned
  distinct pages with a provider continuation cursor. Host regression coverage
  verifies filtering by the manifest and use of the correct search cursor.
- Host formatting, **297/297** engine and FFI tests, Clippy and Android lint
  passed. The local Gradle build and lint completed in 1m54s.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial CB512E6RTY -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.RegistrySearchPresentationTest,dev.wasmdroid.app.RegistrySearchStateTest,dev.wasmdroid.app.DiagnosticReportTest' -TimeoutSeconds 300
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The device checks cover discovery and download availability in the Store.
Package installation and launch for Bash and Flask, a second UI page, and
Android 9 were outside this retest. Remote CI was not run on this revision.
