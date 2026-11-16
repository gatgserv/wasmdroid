# WasmDroid

An Android-first, native app for running public WebAssembly packages locally.
Jetpack Compose talks to a runtime-neutral Rust engine through UniFFI.
The product plan is [plan-aplicatie-wasmer-mobil.md](plan-aplicatie-wasmer-mobil.md).

## Current state

Stage 0 is in progress. This checkout contains the engine contracts, catalog
validation, a verified SHA-256 content store, an initial SQLite migration, and an
Android laboratory screen with a generated native bridge and an optional WASI probe.
The engine now searches/resolves the public registry, verifies WebC v3 downloads
and transitive dependency graphs, and publishes installed metadata atomically.
An explicit native harness executes the real Python package offline on Android;
it is not yet an arbitrary-command user runner or a full package-manager UI.
Background execution, HTTP and complete Stage 0 acceptance remain pending. The curated catalog is
empty until packages have Android evidence. No Play compatibility claim is made.

## Layout

| Path | Purpose |
| --- | --- |
| `engine/` | Serializable API, catalog validation, storage, driver contract |
| `engine/drivers/wasmer-local/` | Wasmer-only dependencies and finite WASI probe |
| `engine/ffi/` | UniFFI Android boundary |
| `android/` | Native Compose laboratory app |
| `catalog/` | Versioned curated catalog |
| `docs/` | Contracts, backlog and verification evidence |

## Rust checks

Use Rust **1.95.0 through rustup**, not a separate Rust MSI installation.
Dependencies are committed in `Cargo.lock`. Wasmer 7.4.2 / WASIX 0.704.2 are pinned
because Wasmer 7.5 requires Rust 1.96. Wasmer imports must stay inside its driver crate.

```sh
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo fmt --all --check
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

On Linux, the runtime probe can also be checked directly:

```sh
cargo test --locked -p wasmdroid-wasmer-local
cargo run --locked -p wasmdroid-wasmer-local --example probe
```

The selected Cranelift backend does not support native Windows. Run its probe
on Android, or Linux; the runtime-neutral engine checks also run on Windows.

## Android development (Windows)

Install JDK 21, Android SDK platform 36, NDK **28.2.13676358**, and rustup.
Set `ANDROID_HOME` (or use the standard `%LOCALAPPDATA%\Android\Sdk` location).
Gradle invokes `tools/build-native.ps1`, which cross-compiles the engine and
generates matching Kotlin bindings. No manually copied bindings are committed.

```powershell
$env:JAVA_HOME = 'C:\path\to\jdk-21'
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
cd android
# arm64 phone, includes the actual runtime probe:
.\gradlew.bat :app:assembleDebug -PwasmdroidRuntime=true
# x86_64 emulator:
.\gradlew.bat :app:assembleDebug -PwasmdroidAbis=x86_64 -PwasmdroidRuntime=true
```

The default bridge-only build omits Wasmer. The app explicitly shows that the
execution probe is unavailable. To include both architectures use
`'-PwasmdroidAbis=arm64-v8a,x86_64'` (quote the argument in PowerShell). Native libraries currently use the Rust debug
profile: their size and speed are not release performance measurements.

Target a specific existing emulator/device for the focused integration tests from
the repository root (set `JAVA_HOME`/`ANDROID_HOME` as above):

```powershell
.\tools\test-android.ps1 -Serial emulator-5586 -Abis x86_64
```

These tests check the packaged native bridge and built-in `fd_write` module.
The runner uses `adb -s` throughout, has a bounded instrumentation timeout,
and stores its output under `.local/test-results`. It installs/replaces only
WasmDroid and its test APK; it preserves app data and does not reset devices.
They do not satisfy the registry/Python/HTTP Stage 0 acceptance criterion.
The Windows build script is the first supported Android build host; a portable
Linux/macOS packaging script remains backlog work.

The live registry/Python suite is separate and explicitly selected. A connected
arm64 phone uses `-Abis arm64-v8a`; an emulator uses `x86_64`:

```powershell
.\tools\test-android.ps1 -Serial <device-serial> -Abis arm64-v8a -RustProfile release -TestClass dev.wasmdroid.app.RegistryProbeTest -TimeoutSeconds 600
```

`-RustProfile release` includes optimized Rust inside a debug-signed Android APK.
For packaging directly through Gradle, use `-PwasmdroidRustProfile=release`.
Keep Android debug/release signing separate from the Rust compilation profile
when reporting measurements. The default hermetic suite selects only `EngineBridgeTest`.

For a host-side registry check without a native Windows Wasmer backend:

```sh
cargo run --locked -p wasmdroid-engine --example registry -- search python
cargo run --locked -p wasmdroid-engine --example registry -- pull wasmer.io/python/python .local/registry-spike
```

Product screens are specified in [docs/design/ui-flow.md](docs/design/ui-flow.md)
and sketched before their Compose implementation.

## License

WasmDroid is licensed under [Apache-2.0](LICENSE). Dependencies retain their own
licenses. Wasmer is MIT; its copyright and license must accompany distributions.
Registry packages are separate works and require individual license review.
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
