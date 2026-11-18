# Transient generation-scoped LAN transport primitive

Tested source: `f74950adb087752f50b0087953d7f21ac3a6d1fd` (gatgserv).
This is a neutral transport building block, not an enabled Android sharing flow.
Actual worker ingress, Android selected-network binding/permission lifecycle,
per-app user opt-in, QR and physical LAN acceptance remain unfinished.

## Source and independent review

`network::lan` adds an immutable instance/generation/guest-port/interface-epoch/
private-IPv4 scope, mandatory unconnected-socket network binder and retained
listener. The connector owns an actual generation capability; no reusable host
port lookup is provided. Wrong connector or returned-stream scope cannot forward
client bytes. Client/connect bounds, duplex half-close, revocation and awaited
terminal child/listener cleanup have explicit contracts. Dropping an observer
does not manufacture a cleanup acknowledgement. Default loopback reservations
and the current unsupported product LAN capability are preserved.

Independent source review found the first source released its listener
before abort/join of child tasks, violating retained-port-through-cleanup scope.
The final source retains the listener through all child destructors. A new
deterministic current-thread regression polls actual shutdown to Pending, cancels
the observer, probes port rebinding during pending-future destruction, awaits the
real closure receipt, and checks peer EOF plus final rebind. The review read-back
confirmed the correction; no second concrete blocker was reported.

No new dependency was introduced: Cargo explicitly enables the already used Tokio
`io-util` feature; Cargo.lock is unchanged. Wasmer imports stay in its driver.

## Executed checks

```powershell
cargo test --locked -p wasmdroid-engine --lib network::lan
cargo test --locked -p wasmdroid-engine --lib
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all --check
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

- Final LAN socket cases: **12/12 PASS**, compile3.28 s, tests2.09 s. The initial
  eleven-case run passed before the independent lease-order finding; its results
  are superseded for the final primitive by these12 cases.
- Full engine library: **19/19 PASS**, compile0.38 s, tests2.05 s. This includes
  the12 LAN cases, default loopback exclusion/unsupported policy, registry parsing/
  private URL rejection, process-stat parsing and pending TLS cancellation.
  It is a library suite, not every integration test or the complete product suite.
- Clippy: **PASS9.69 s**, warnings denied across the selected crates/all targets.
  Rust formatting and whitespace checks passed.
- Android: **PASS23 s**, release arm64 native20.89 s plus matching bindings,
  main/instrumentation Kotlin task graph;33 tasks1 executed. Main/test Kotlin
  stayed up to date because the prior notification/Notes source was unchanged.
  The new Rust module compiled for Android; no Android test was executed.

Logs are ignored local files: `.local/lan-native-final.log`,
`.local/lan-library-final.log`, `.local/lan-clippy-final.log` and
`.local/lan-android-final.log`. No build/test process remains live after these
reported results.

The portable core tests retain real loopback listeners/streams. Production
`LanLease::bind` still requires strict RFC1918 plus the mandatory platform binder;
the private test-core entry grants no production loopback/wildcard fallback.
The fixture binder refusal checks real pre-listen ordering and socket release,
not actual Android selected-interface binding.

No physical Wi-Fi ingress, network switch/permission denial/revocation, real
guest adapter, browser/TLS/authentication, QR, UI/accessibility/rotation, full
suite/lint, remote CI, push, merge or release was performed. Read
[the integration contract](../design/lan-ingress.md) for required work. The earlier
notification/Notes evidence retains its separate exact source revisions in
[its report](2026-11-17-notification-notes.md). Full application acceptance remains
open.
