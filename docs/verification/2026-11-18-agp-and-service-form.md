# AGP upgrade and service form verification

The runtime-enabled debug app and test APKs were assembled from
`9e17cafe05ab9d87d2620b7e50aa37fa645f06a5` on `codex/android-api-37`.
The baseline dependency refresh used AGP 9.2.1. The candidate uses
AGP 9.5.1 and Gradle 9.9.1, with minimum API 28, target API 37,
and the release-profile Cranelift runtime for ARM64 and x86_64.

## Gradle diagnostics and lint

| Gradle diagnostic | Scheduled impact | Baseline | Final |
| --- | --- | ---: | ---: |
| Kotlin task registration property delegates | Gradle 10 | 2 | 0 |
| Project used as dependency notation by AGP | Gradle 10 | 2 | 0 |
| `Configuration.setVisible(boolean)` | Gradle 11 | 15 | 15 |

Counts are occurrences in Gradle's problems report. The two native/assets
tasks now use explicit typed registration. AGP 9.5.1 removed the project
dependency notation diagnostics. Its remaining visibility diagnostic is
assigned to Gradle 11 by the current wrapper. The tested Gradle version
was 9.9.1. [AGP 9.5 release notes](https://developer.android.com/build/releases/agp-9-5-0-release-notes)
· [Gradle 9 upgrade guide](https://docs.gradle.org/9.9.1/userguide/upgrading_version_9.html)

AGP's newer lint rejected the transitive Lifecycle 2.9.4 check registry.
The app declares its used Lifecycle Compose library directly at 2.11.0,
which supports API 23. The two shortcut ViewModels keep their cleanup and
omit the empty superclass call flagged by the newer checks. The final
`lintDebug` report showed **no issues** with the checks enabled.

## Packaging and automated tests

Both APKs passed v2 signature verification and 16 KiB ZIP alignment.
APK inspection confirmed the declared SDK levels and ABIs.

| Artifact | SHA-256 |
| --- | --- |
| App APK | `915eeafffa8fe5bdddd0f29f17531623182f714ee8dafef77aab200888add905` |
| Instrumentation APK | `3bed4bfb67be4efac6e8a8a7b18d5730e62beda037f3956c4e3333e1a8e6b67e` |

- Android 9, API 28, x86_64. **52/52** selected runtime, configuration,
  network and shortcut cases in 8.430 s. The Wasmer Registry Store opened.
- Android 17, API 37, x86_64. **37/37** selected runtime, configuration,
  WebView profile, and shortcut cases in 10.734 s. The additional live
  Notes acceptance passed in 17.861 s, including HTTP from two instances
  of `wasmer.io/wasmer/static-web-server@2.44.0`.
- Windows engine and FFI passed 292/292. Formatting and host Clippy
  with warnings denied passed.

The app APK installed on both emulators matched the local hash.

## Create service form and CPU monitoring

On Android 17, the installed Registry package
`wasmer.io/wasmer/static-web-server@2.44.0` opened **Create service**.
The TTY and standard-input rows each exposed one checkbox action. Each
label's center was within 0.5 pixels of its row's vertical center on this
display. Tapping the TTY label toggled the checkbox. The optional
numeric timeout remained visible. The form omitted the static
CPU/memory limits hint.

The runtime still rejects explicit CPU shares and memory caps. CPU and RAM
observation use the running worker's process measurements. In the same
APK, the Python HTTP service displayed `389.8 MiB · CPU 2.4%` and opened
its fixture page in the embedded WebView.

A separate service created through the form used the package's default
`static-web-server` command and an existing test-owned read-only volume
at `/public`. It reached **Running** and exposed **Open** with the host,
port, and document root supplied as arguments. Its specification left out
resource limits and the service timeout.

## Sony ARM64 catalog package investigation

The Sony G8142 (Android 15, API 35) had the previous target-36 APK installed
with SHA-256 `cae4d64fd2a8636916cf572723cc44c55da04fa63f66f24e8047e9873364c306`.
Starting the existing `webserver` card ran
`wasmer.io/baran/static-web-server@1.0.5`. The original configuration left
arguments, volume mounts, port publication, CPU and memory caps, and timeout
empty. The run ended after 1.157 seconds with code 1. Its stderr reported
that binding to `[::]:80` failed with `Not supported (os error 58)`, consistent
with the documented private TCP network's unsupported dual-stack bind.

Setting `--host 127.0.0.1 --port 8080` exposed the next error, an inaccessible
default document root (`./public`). With a fresh read-only volume at `/public`
and `--root /public`, startup instead reached a panic in the package's Tokio
`runtime/thread_pool/park.rs`, again reporting OS error 58, this time with exit
code 129. The guest pinpoints the failing Tokio module. The exact guest syscall
behind this last error remains unverified. The original configuration was
restored after the unsuccessful probes. The CPU legend is a measurement scale:
one busy core is 100%, including the JVM and Wasm compiler in the worker.

On the same Sony APK, the installed Wasmer Registry package
`wasmer.io/wasmer/static-web-server@2.44.0` created a separate service named
**Static web server**. Its `static-web-server` command used
`--host 127.0.0.1 --port 8080 --root /public`, with TCP port 8080 published to
an automatically allocated phone-only port. The test volume `webserver-public`
was mounted read-only to the guest. The Android document picker imported
`index.html` into that volume. The standalone service remained running and
**Open** rendered the test page in the embedded WebView. The service was left
running. This test covered local HTTP with the official package on the existing
phone build. It left Wi-Fi sharing and outbound permissions out of scope.

## Commands

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-device> -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <class-group> -TimeoutSeconds 420
.\tools\test-android.ps1 -Serial <selected-api-37-avd> -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass dev.wasmdroid.app.NotesRecipeAcceptanceTest -NotesLive -TimeoutSeconds 420
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The form and running worker checks used serial-targeted UI inputs and the
Android accessibility hierarchy. The Sony still had the API 36 build
when its form was inspected. Device acceptance of this APK used the
isolated Android 9 and Android 17 emulators. Physical device execution,
an actual Gradle 10 build, and remote CI remain separate coverage.
