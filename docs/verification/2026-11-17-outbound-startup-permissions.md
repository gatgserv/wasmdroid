# Standalone outbound startup and reviewed permission controls

Date:2026-11-17. Exact local source:
`3155b469026ba253ffdb37aa83d14ff7180aba60`, branch
`codex/stage0-foundation`, author and committer `gatgserv`.
This checkpoint implements first startup/restart, not full network delivery.
Running handover/regrant, managed combined-app grants, unsupported address forms
and actual Android/provider/visual acceptance remain open. No push, remote CI,
phone install/execution, merge, release or catalog promotion occurred.

## Implemented and independently reviewed

- Normal My Apps Network access follows the English scheme presented before GUI.
  Two independent saved flags, actual native birth identity/version CAS, stopped/
  managed guards and actor job/data reservations preserve unrelated fields/data.
  Catalog requests do not grant access. Native source excludes args/environment.
- Scope issuance captures actual Starting desired-run authority/spec/version and
  engine ownership. Plain matching scope fields cannot create the Android adapter.
  A one-use capability attaches before guest spawn, with postcompile/native/stop
  checks; long startup and injected callbacks run outside the driver control lock.
- Actual Java default Network, including VPN, supplies bind and DNS. Native
  blocking callbacks retain actual lease/duplicate FD until return, independent
  of observer cancellation. Weak revoker installation precedes long startup and
  avoids callback ownership cycles. Original Java PFD remains borrowed until
  native entry duplicates it, including the no-egress production path.
- Bounded captured own-address/direct-route/known-Pref64 context supplies explicit
  destination classification. DNS allowed by either grant does not grant TCP to
  an otherwise forbidden answer. Full actual group-name inventory, including
  stopped/ungranted services, is reserved before external resolution.
- Fresh actual default/context checks surround readCapture/bind/DNS even if Main
  callbacks are queued. current() remains atomic-only, avoiding Binder IO under
  native locks. API29+ first capture awaits actual unblocked observation; API28
  cannot observe that callback status. OS observation/dispatch is not an atomic
  universal transition or hidden-topology guarantee.
- Field-specific draft setters prevent a fast LAN callback from restoring an old
  Internet grant before recomposition. Reload/dismiss/save use current epochs;
  late owner/source/identity results cannot retarget a new application.
- The dialog discloses background-service scope, separate web-page network
  limitations, current Android/VPN route and possible DNS provider visibility.
  Neither switch certifies complete WebView isolation.

Root and both lanes read the integration. Concrete accepted findings were fixed:
borrowed ownership-fd handoff, fresh default/context guards beyond queued callback
observations, and stale other-toggle values. Readback found no remaining concrete
scoped finding; this is not actual Android runtime proof.

## Executed native checks

Windows host, locked dependencies:

```text
cargo fmt --all
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi --no-fail-fast
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all --check
```

Corrected full selected suite: **270/270 PASS =263 engine +7 FFI**,0 failed/ignored;
test compilation2.13s. Included15 pure destination-policy cases0.00s,3 actual
native group-inventory cases0.12s and4 normal permission/data-preservation/identity
cases0.11s. All existing selected regressions also passed. Windows cfg does not
execute Unix-only driver tests.

Final selected Clippy PASS2.15s, no gate relaxation. Its initial8-argument FFI
finding was corrected with a typed worker identity record. The final FFI library
was then rerun after that signature change: **7/7 PASS0.00s**, compilation2.82s.
This repeat is included in270, not counted twice.

Initial full sweep had only3 new group-inventory fixture failures: unsupported
Compose labels. The fixture moved adversarial requested-name metadata into
supported environment/artifact labels while preserving real stopped/ungranted
membership and negative-alias assertions. The parser/production gates were not
weakened. Corrected full sweep passed all targets.

## Executed pure JVM checks

Fresh Kotlin2.2.21/JDK21 compilation, JVM target17, cached coroutines1.10.2,
JUnit4.13.2/Hamcrest1.3. Sources: NetworkPermissionController/EgressCaptureGate
plus their actual test files. Runner `.local/test-network-permissions.ps1`, then:

```text
org.junit.runner.JUnitCore dev.wasmdroid.app.NetworkPermissionControllerTest dev.wasmdroid.app.EgressCaptureGateTest
```

**15/15 PASS0.078s =10 controller +5 gate cases**. Covers identity/version changes,
late loads, save ownership/observer behavior, unsupported/managed/running guards,
withdrawal, rapid old-frame callbacks and concurrent loss/revoker installation
(20 controlled races, exact once). No Android/Compose/Network/JSON stubs executed.
Total selected executed checks **285 =270 native +15 pure JVM**; existing retained
regressions are included, not285 new tests.

## Android matching compilation

Existing JDK21/SDK/NDK28.2.13676358, arm64-v8a release native Cranelift:

```text
cargo check --locked -p wasmdroid-wasmer-local --tests --target aarch64-linux-android --release --no-default-features --features cranelift
android/gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

Driver cfg(test) PASS5.86s. Native Rust/bindings rebuilt23.76s; initial Android
compile then caught a toggleable overload call, corrected using named arguments.
Matching main/test Kotlin build **PASS15s**,33 tasks:3 executed,30 up-to-date.
One existing TerminalView.characters deprecation warning appeared; that file was
unchanged. Final disclosure copy/UI build **PASS8s**,33 tasks:10 executed,
23 up-to-date; native/bindings up-to-date. Sessions76174 and69181 are terminal;
the last final-copy command exited0. No builder remains live.

**13 new driver definitions (5 Unix provider/FD +6 startup-authority +2 private
inventory/ordering) compiled, NOT executed** alongside prior transport cases.
Actual Android capture/descriptor/DNS/bind/VPN/stop/death and rendered dialog
acceptance were not executed. Compilation does not establish those behaviors.

Read-only source/feature audit:

```text
cargo tree --locked --offline -p wasmdroid-ffi --features wasmer --target aarch64-linux-android -e features -i wasmer-wasix
```

Only sys-minimal/sys-thread/rusty_pool selected for wasmer-wasix0.704.2; neither
host-vnet nor host-reqwest/js selected. Cached exact SDK source makes default HTTP
client None for this feature set. Verified package loading installs offline
source/loader before guest execution. This narrow audit is not browser isolation
or proof about every future runtime feature combination.

Detailed failure/final logs remain in ignored `.local/outbound-product-*` and
`network-permissions-host-final.log`. First-start/restart source progress leaves
the full goal, related issue acceptance and final integrated phone/CI phases open.
Progress is recorded on existing [issue #3](https://github.com/gatgserv/wasmdroid/issues/3#issuecomment-6962154414)
without closing acceptance criteria or changing the submitted PR revision.
