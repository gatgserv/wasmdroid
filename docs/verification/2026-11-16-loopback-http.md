# Registry HTTP and instance isolation — 2026-11-16

Tested code and generated-binding configuration:
`12d8e33268b7bb798e721dea8261eb6f863464da`.
HTTP implementation: `7f177cacb7a16c4243faa31b6b8eb3ebc163fed9`.
Tracking: [issue #1](https://github.com/gatgserv/wasmdroid/issues/1),
[PR #2](https://github.com/gatgserv/wasmdroid/pull/2).

## Actual device evidence

Physical Sony G8142, Android 15/API 35, arm64-v8a, 4096-byte pages.
Android debug signing, optimized Rust release profile, Cranelift.
The combined targeted suite passed **6 tests, 0 failures/skips**, in **67.694 s**;
ADB exited 0. See [instrumentation output](2026-11-16-phone-http-tests.txt).
This is targeted native/registry acceptance, not the final product/browser E2E.

- Downloaded/resolved public `wasmer.io/wasmer/static-web-server@2.44.0` through
  the existing verified installer. WebC SHA-256:
  `1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08`;
  container size 4,755,156 bytes. Execution sees only verified installed packages.
- The actual Wasm server served an in-memory `/public/index.html`. Android's
  `HttpURLConnection` received HTTP **200** twice, with the expected document
  and instance marker. No host directory was mounted.
- Two live guest servers used the same virtual `127.0.0.1:8080`, distinct
  engine-reserved host ports and private document roots. Each response contained
  only its own marker. After stopping the first, the second still served its page.
- Stop closed the host listener and all accepted forwarding tasks. A subsequent
  real host TCP connection failed. Tracked guest execution callbacks reached zero.
  Repeated stop returned the retained result without rerunning cleanup.
- This package required **SIGKILL escalation** after SIGINT, reported as
  `forced=true`, WASIX exit code **27**, `activeTasks=0`. This is a confirmed
  forced stop, not a graceful exit or a Unix-signal exit-code interpretation.
  The observed 7,191 ms includes preparation, execution and stop grace periods;
  it is not a compilation/startup benchmark.
- Three bridge regressions and the real Python sandbox probe passed again after
  sharing the offline graph loader and introducing an owned UniFFI object.

## Isolation and regression scope

`engine::network::PortLease` holds the original host listener across reservation
and forwarding, avoiding a free-port probe/rebind race. It supports TCP loopback
only; LAN, UDP, no-network forwarding and zero guest ports are explicitly rejected.

Each instance receives a fresh virtual loopback network with no host networking
backend. Only the exact declared guest address/port may bind. Guest DNS, outbound
connect, UDP and raw/administrative operations remain unsupported. Bound-socket
`connect()` is also denied: otherwise upstream could bypass the network-level
connect method and fall back to the first virtual listener. The host forwarder
never accepts an arbitrary guest target. Connections are capped at 32 and 30 s.

Host regressions cover reservation ownership/release and unsupported modes.
Native driver regressions cover wrong-address/port binds, both outbound-connect
paths, DNS/UDP rejection, independent virtual networks, and tracking callback
activity until it actually returns. These complement the physical two-server
test; neither replaces general resource-limit or malicious-guest testing.

Android cleartext is permitted only for numeric `127.0.0.1`. Device assertions
confirm cleartext remains denied for an external domain and non-loopback address.
Registry transport remains HTTPS. See Android's
[Network Security Configuration](https://developer.android.com/privacy-and-security/security-config)
and the server's [pinned CLI source](https://github.com/static-web-server/static-web-server/blob/v2.44.0/src/settings/cli.rs).

## Failures corrected without weakening gates

The first local stop attempt failed after 19.165 s: the main-task status finished
while executor work remained. Stop now escalates for remaining work even if the
main handle already finished, and requires callbacks to unwind. It never invokes
WASIX `terminate()` as proof of completion. The original HTTP request failure is
preserved if cleanup also fails.

That exposed Android's cleartext denial before any HTTP request. A scoped
loopback configuration fixed it; global cleartext was not enabled.

The first six-test pass on `7f177ca` took 65.263 s. Lint then found three NewApi
errors in generated JVM Cleaner calls against minSdk 28. Configuring UniFFI
`android=true` generates the API-34-guarded SystemCleaner/JNA fallback. The final
six-test suite above and lint passed on `12d8e33`. No minSdk increase, lint
baseline or suppression was used. API 28 execution has not yet been tested.

## Commands and remaining work

From the repository root with rustup Rust 1.95, JDK 21, SDK 36 and NDK 28.2.13676358:

```powershell
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo fmt --all --check
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
# Use the NDK CC/AR/linker environment from tools/build-native.ps1:
cargo clippy --locked -p wasmdroid-wasmer-local -p wasmdroid-ffi --features wasmdroid-ffi/wasmer --all-targets --target aarch64-linux-android -- -D warnings
.\tools\test-android.ps1 -Serial $env:WASMDROID_TEST_DEVICE -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -TestClass 'dev.wasmdroid.app.EngineBridgeTest,dev.wasmdroid.app.RegistryProbeTest,dev.wasmdroid.app.HttpProbeTest' -TimeoutSeconds 600
.\android\gradlew.bat -p android :app:lintDebug -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release
```

Host Rust: **26 tests passed**, core Clippy and arm64 Wasmer-enabled Clippy passed.
Android lint: **0 errors, 9 warnings**, retaining the earlier dependency/target,
ChromeOS, backup and icon warnings. The native runtime tests run in Linux CI;
cross-target Clippy alone does not claim their execution on Windows or Android.

Prior remote revision `fbcf75a` passed all four jobs in
[CI run 43940662046](https://github.com/gatgserv/wasmdroid/actions/runs/43940662046).
That run predates HTTP. Consult the PR's exact submitted head for new CI evidence.

Stage 0 remains open for the compiler/size/cold/warm/peak-memory matrix and curated
package inventory. This fixed acceptance harness is not a generalized lifecycle
driver, foreground service, product GUI, compiled-module disk cache, volume
manager, browser launch, resource enforcement or Play validation. The complete
Android goal remains tracked in `docs/implementation-status.md`.
