# WordPress setup and in-app MariaDB feasibility

## In-app server prerequisite

On the physical Sony G8142, the existing WordPress service used
`wasmer.io/john5james6653/wordpress-woo@0.3.1`. It was running with no TCP
publications or WordPress database environment keys. Site acceptance remains
blocked on an in-app MySQL-compatible server. This check was limited to
running the database in WasmDroid at the owner's request.

The live Wasmer Registry returned no command-search results for `mysql`,
`mysqld`, `mariadb` or `mariadbd`. The package-name index returned the
Flask CRUD example `helloccc/flask-mysql-crud@0.1.1`, with a `script`
command, for `mysql`. The other three name searches returned no results.
These queries cover the discovery routes used by Store.

The public [Wasmtime MariaDB v0.1.11 release](https://github.com/adamziel/wasmtime-mariadb/releases/tag/v0.1.11)
archive asset matched its GitHub SHA-256
`2a0197878b5c2b370885674e5a9c569ff5cdd36f3c88bdbc44e53ba56fdb1b87`.
Its 24,557,237-byte module had SHA-256
`632e8703b94b89021e516f4c8d953de398462abd6c57ea9449d6818327a4b9e6`.
`wasmparser` 0.255.0 read 12 imports under `wasmtime_mariadb_files` and
15 under `wasmtime_mariadb_sockets`, including file locks, TCP listen/accept,
and poll. Neither namespace is supplied by the current WasmDroid WASIX
driver. The release needs its own host, so the module was inspected without
attempting to launch it. [Kandelo's MariaDB build](https://github.com/Automattic/kandelo/blob/main/docs/profiling.md#mariadb)
uses its own POSIX SDK and kernel as well.

No in-app database endpoint could be configured from these candidates.
The existing WordPress instance was left running and no new PC-to-WordPress
site check was possible.

## Configure required settings before start

The revision in this section is historical. The final APK uses the generic
mechanism recorded below.

Source `99e9de3f5219f26f7ea00842adf317a963c9de0b` attaches the verified
setup requirements to the existing Wasmer release identity and its exact
installed digest. **Set up to start** opens the versioned service editor on
**Execution** with named database host, port, name, username, and masked
password fields. The package's `install` command also requires explicit
initialization opt-in. The running service owner checks the current stored
settings before allocating a standalone or stack worker. Its error names
the missing settings, without values.

- Debug APK SHA-256 `6c806d7924a11bac07718806ef4d3c3ce9b40e48fa7ee5456df519ef9731bc04`.
- Instrumentation APK SHA-256 `9a6b3bd023f63f388ffe276bd94fd8fce733b72d9c4ed16f782fe50e268f12eb`.
- API 37 target and API 28 minimum. ARM64 and x86_64 native libraries use
  the release Cranelift runtime.
- The dedicated Android 17 x86_64 emulator passed **38/38** selected tests.
  Coverage includes the digest guard, valid and invalid port settings,
  explicit install opt-in and the versioned form entry point.
- The installed emulator APK matched the checked app digest. Starting setup
  for a test-owned service in normal mode opened all five database fields
  and masked its synthetic password. The saved instance stayed in the
  created state and its stored configuration was preserved during this
  unsaved form interaction.
- Rust formatting, **297/297** engine and FFI tests, Clippy, Android lint,
  APK signature and 16 KiB ZIP alignment checks passed.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial emulator-5596 -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.PackageRuntimeRequirementsTest,dev.wasmdroid.app.ServiceConfigurationControllerTest,dev.wasmdroid.app.ServiceConfigurationDraftTest,dev.wasmdroid.app.ServiceConfigurationTest,dev.wasmdroid.app.BrowserDataControllerTest,dev.wasmdroid.app.ServiceFailureTest' -TimeoutSeconds 240
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

## Physical Sony and the generic replacement

The Sony G8142 stopped the existing `wp3` through the normal **Stop** action.
The owner committed an exited state with guest exit 127 and its confirmed
stop reason. The initial APK from source `99e9de3` passed **38/38** selected
tests on Sony (Android 15 / API 35, ARM64). In the actual `wp3` card, **Set up
to start** opened all five database fields. No configuration was saved
during this observation.

The final source `c1c723208de60c24886a60f85cc282d5f2c3b2c0` removes the
release-specific rule. The same startup guard now uses declared per-command
registry requirements and service-level markers from **Configure**.
Both sources feed one standard editor. The owner checks the stored values
before allocating standalone or stack workers. See the
[manifest annotation contract](../design/package-installation.md#required-environment-settings).

- Debug APK SHA-256 `a9e5d327ef2a98d0c3d71c21d5a523cd8a10c042294dd1b7d70409aa4ab7d2db`.
- Instrumentation APK SHA-256 `efd2f0fe3f0d269486662e061217d3b5a72cd1f53693e4614a55c6492d984c9a`.
- The runtime-enabled API 37 target build includes ARM64 and x86_64,
  with API 28 minimum. Lint, APK signature and 16 KiB alignment passed.
- **32/32** selected Android tests passed on Sony. Rust formatting,
  **298/298** engine and FFI tests and Clippy passed. No remote CI or
  Android 9 execution was attempted on this revision.
- The final APK on Sony matched the recorded digest. The existing stopped
  `wp3` displayed the ordinary **Start** action. On a separate Bash
  service, normal-mode **Configure** saved a generic required marker.
  Two attempts to start with the field empty reopened the form and
  retained the created state. Saving a masked test value restored **Start**.
  Reopening the editor and clearing the value brought back the form.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial CB512E6RTY -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.PackageRuntimeRequirementsTest,dev.wasmdroid.app.ServiceConfigurationControllerTest,dev.wasmdroid.app.ServiceConfigurationDraftTest,dev.wasmdroid.app.ServiceConfigurationTest,dev.wasmdroid.app.RegistrySearchPresentationTest' -TimeoutSeconds 240
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```
