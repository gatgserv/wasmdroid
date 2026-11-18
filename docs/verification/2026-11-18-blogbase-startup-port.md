# Blogbase startup port and Wi-Fi

Source `54fd900a27c98203e66d580a0e1302afeb1cf751` adds **Set --port at
startup** to **Configure → Data**. Selecting it uses the chosen publication's
application port as the WASI command's `--port` value. The Wasmer driver
preserves the other command and user arguments. The automatic phone port and
the web-access selection can be configured independently.

An on-device edit also exposed a rejected save when the service already had
Internet access. Source `ecfefc67847e1413a83e6b6446781ce74dda109f` allows the
Android metadata edit with the existing Internet and LAN flags. It checks
both flags against the saved instance in the edit transaction. The separate
**Network access** flow remains responsible for permission changes.

## Package and build

- Sony Xperia G8142, Android 15 (API 35), `arm64-v8a`.
- Registry release `wasmer.io/carlosriojas/blogbase@0.3.6`, verified WebC v3
  digest `sha256:4e26e9b10e7909950d6fc6a0050034992fcad324dbe0b7d3aa96c06f6d1c71ec`.
- The package runs `script` with `--root /public --port 8080` from the manifest
  and uses the installed `wasmer/static-web-server@1.1.0` dependency. Before
  the change, appending `--port=8081` caused exit 2 due to a duplicate option.
  A temporary Static Web Server TOML file allowed a separate guest TCP 8082 run.
- Build uses runtime-enabled ARM64 and x86_64, release Rust, Cranelift.
- App APK SHA-256
  `5452b56b63127c7a13b4e887d022b3e8013fe41c55638ee62f0889a5d54266ce`.
- Instrumentation APK SHA-256
  `d884bcc64245a0c72150dfb10b7cc586e2a1bd1e8f47f9b056760f76ab50a5df`.

## Automated checks

- Android assemble and lint passed. Gradle reported the existing
  `Configuration.setVisible` deprecation. Bundled Wasmer emitted four warnings.
- APK signature and 16 KiB ZIP alignment passed.
- 48 Android form, configuration, permission, LAN sharing, and browser
  session tests passed on Sony. The configuration regression checks retaining
  existing Internet access and rejecting permission edits through this API.
- 298 host engine and FFI tests passed. Formatting and Clippy passed.
- Five Wasmer startup-port cases passed as an ARM64 native test process on Sony
  at `54fd900`. The final build uses the same driver source.
  The tests cover replacing annotations and additional arguments, preserving
  positional arguments, binding validation, and compatibility with existing
  configurations.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <sony> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.ServiceConfigurationDraftTest,dev.wasmdroid.app.ServiceConfigurationControllerTest,dev.wasmdroid.app.ServiceConfigurationTest,dev.wasmdroid.app.NetworkPermissionControllerTest,dev.wasmdroid.app.LanSharingCoordinatorTest,dev.wasmdroid.app.BrowserSessionAuthorityTest' -TimeoutSeconds 360
cargo fmt --all --check
cargo test --locked --offline -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked --offline -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo test --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --lib --no-run
adb -s <sony> shell <arm64-driver-test-binary> service_startup_port::tests
```

Device testing used the Sony. Android 9, Android 17, and remote CI were
outside this verification run.

## Live configuration and Wi-Fi

The registry fixture already had Internet access enabled. In **Configure**,
the operator removed the previous test argument, disconnected its test volume,
and selected application port `8083` with **Set --port at startup** in **Data**.
The saved service retained Internet access, web access on guest TCP 8083,
and automatic phone port selection. Its additional arguments and mounts were
empty. The test volume's contents were preserved.

The operator selected **Start**. The card showed **Running** and advertised a
Wi-Fi IPv4 address with a separately allocated relay port. On the PC, Chrome
connected directly to that address and issued `GET /`. The Blogbase home page
returned HTTP 200 and rendered **TGM News**, proving that the actual package
served its bundled site through the publication of guest TCP 8083. The
service was left running for review.

## Article feed

Chrome displays the **TGM News** section after an HTTP 200 for the home page.
The browser requests the articles from the Sanity project's API. With the
phone's Wi-Fi origin, Sanity returns HTTP 403 without
`Access-Control-Allow-Origin`. The same unauthenticated query without the
Origin header returned HTTP 200 with three results. Chrome reports a CORS
error, leaving the page without article links to exercise.

The guest Internet grant applies to the Wasm process. Browser requests have
their own origin and provider rules. The Sanity project owner controls the
[allowlist for browser origins](https://www.sanity.io/docs/content-lake/cors).
