# Create a service from a downloaded Store release

Store offers **Create service** on a verified download's completion card and
on the matching saved release in search results. The configuration receives
the exact installed package reference and an available command from its local
record. Dismissing the completion card leaves the action on the search result.
The downloaded package list uses the same action.

The runtime-enabled debug APK was built from
`6422392df038cc5bec6650f33e67d8ed4ae12364` for ARM64 and x86_64, with API 28
minimum and API 37 target. The Sony G8142 ran Android 15, API 35, on ARM64.
The installed APK matched the local app APK SHA-256
`42d280509d8a9fc35473fc6603f4364c86984d054d89fc65e3f98b8cbf6dcf6c`.
The matching instrumentation APK SHA-256 was
`8677e76c2365b96184f59f6d4a46f95772c67778bf3f80c956664f9fb224547a`.

## Results

- The live Store showed **Create service** for an already saved Bash release.
  The failed Flask installation displayed its error and did not offer service
  creation.
- A fresh Store download of `wasmer.io/simplesue/wasix-hello@0.1.0` offered
  **Create service** on its completion card. The dialog showed the exact
  downloaded reference. After dismissing the completion card, the same search
  result showed **Downloaded** and opened the same creation dialog.
- Saving the configuration made the named service appear in **My apps**. The
  tester removed the test service through its card and retained the package.
- **41/41** selected Android tests passed, including the completion-to-root
  reference check, an unavailable or removed release, and service configuration.
- Rust formatting, **297/297** host engine and FFI tests, Clippy and Android lint
  passed. APK signature and 16 KiB ZIP alignment checks also passed.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial CB512E6RTY -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.RegistrySearchPresentationTest,dev.wasmdroid.app.RegistrySearchStateTest,dev.wasmdroid.app.PackagePresentationTest,dev.wasmdroid.app.ServiceConfigurationControllerTest' -TimeoutSeconds 300
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

Guest startup, Android 9 execution, and remote CI were outside this retest.

## Declared entry point and CPU sampling follow-up

On the same Sony, the saved `wordpress-woo@0.3.1` artifact listed `install`
and `run`, and its verified metadata declared `run` as the entry point.
The service created under the previous behavior used `install`. Two runs
exited with code 0, both with recorded CPU time and resident memory and no
standard error. The public Wasmer manifest confirms that `install` invokes a
PHP setup script. The `run` command starts PHP on guest loopback port 8080.

Source `8f26d72ccd8b2b78ea4e956e26238de74cb14e58` preselects the declared entry
point when it exists in the installed command list. A missing or unknown
entry point preserves the previous fallback. CPU reports **measuring…** while
a valid measurement interval begins and keeps **unavailable** for missing
resource data.

The runtime-enabled debug build targets API 37 with API 28 minimum and ARM64
and x86_64 libraries. App APK SHA-256
`afb7ec7784fa98909ef5a906113204543809aa4771365f82d495b5bf66a07990`.
Instrumentation APK SHA-256
`5e5cd864010bb7483a43ef500c05cb78d9afa77bf83168c9d4941e5c4da9bd07`.

- The Sony's installed base APK matched the app digest. The live Store
  opened the saved WordPress release with `run` selected. The tester closed
  the draft and confirmed the existing service still had its saved settings.
- **19/19** selected tests passed on the Sony (Android 15 / API 35, ARM64).
  The real Python timeout acceptance test sampled CPU and RAM from a running
  worker and calculated CPU usage across measurements in the same generation.
- Rust formatting, **297/297** host tests, Clippy, Android lint, APK signature
  and 16 KiB ZIP alignment passed.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial CB512E6RTY -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.RegistrySearchPresentationTest,dev.wasmdroid.app.PackagePresentationTest,dev.wasmdroid.app.WorkerResourcesTest,dev.wasmdroid.app.ServiceTimeoutAcceptanceTest' -TimeoutSeconds 360
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The retest did not start the WordPress HTTP server or perform Android 9 or
remote CI verification.
