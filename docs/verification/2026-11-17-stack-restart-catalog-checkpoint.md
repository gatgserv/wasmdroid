# Stack, restart and catalog checkpoint

Feature source: `35c3188995ad5255c50d298fee452bcede645194`.
Final reviewed source: `99df3470ba4adbc016441e6aa597916b78b0c6bf`.

## Implemented source

- Stopped-stack update source/preview/apply, exact stack/member/artifact fences,
  atomic membership/spec/dependency/data/event changes, private initial definition
  retention and protected declared/restored recovery datasets. Same-package
  logical services preserve actual identities/history; replacement/reintroduction
  uses fresh identities. No update starts a guest or downloads an artifact.
- Standalone restart policies and explicit default-off boot opt-in, versioned Dead
  replacement, manual cancellation and pending-work foreground lifetime. Automatic
  scheduling requires observed worker death and durable terminal acknowledgement.
  Android boot receiver/service, actor reservations and bound/unbound idle
  retirement are wired to the actual common owner.
- Signed remote catalog/index verification, bounded controlled HTTPS, persistent
  last-good snapshot, rollback/equivocation/clock/freshness fences and exact recipe
  pins. Native/common-owner/AppModel paths are integrated. Optional public APK trust
  configuration is deliberately absent; no host/key or verified recipe is invented.

Native stack editing, restart settings and catalog controls still need their next
English schemes before new GUI implementation. Hard worker memory/CPU limits
remain explicitly Unsupported with false capabilities. Metric/scope/defaults and
an authorized enforceable backend are unresolved; measured RSS/CPU, guest linear
memory, virtual-address rlimits or scheduling hints are not substituted for them.

## Commands and results

On final source `99df3470ba4adbc016441e6aa597916b78b0c6bf`:

```powershell
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
cargo fmt --all -- --check
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
git diff --check
```

Gradle passed in 2 seconds (33 tasks, 2 executed) using JDK 21 and the existing
Android arm64 runtime toolchain. Android app/instrumentation Kotlin compiled;
the matching release-profile native driver/engine/FFI and bindings were reused
from successful 38/24-second builds in this batch. The 24-second build was on
feature source `35c3188995ad5255c50d298fee452bcede645194`; the final source adds
only the Kotlin idle-liveness correction. This is an Android debug compilation
with release-profile Rust, not a release/AAB/signing or physical-run result.
Neutral all-target compilation passed in 1.06 seconds; formatting and diff checks
passed. Initial formatting failures in a driver import and a serde assertion were
fixed before the feature source; no gate was weakened.

Compiled but did not execute 24 new neutral regression definitions (six stack
updates, sixteen catalog updates, two boot/restart serde) and fifteen new Android
coordinator/boot/lifetime definitions. Existing preflight and actual-public-package
stack definitions were expanded for restart/boot and update/data-owner/stale-replay
contracts. Compilation does not establish any assertion or native transaction ran.

## Independent source review and remaining evidence

Independent source review found and corrected stale-zero foreground teardown,
same-zero retirement-token invalidation, automatic timeout misclassification,
post-terminal-acknowledgement cleanup, executor-admission cleanup and final
package/catalog/admin idle-liveness. Final idle shutdown uses an actor/monitor
permit and lifecycle/start-ID fences, rather than a count snapshot. These are
source-review findings and compilation results, not Android scheduler evidence.

No tests/device interaction, actual reboot, screenshots/visual/TalkBack acceptance,
Clippy/lint, intermediate push, CI polling/run/restart, merge or deployment occurred.
The published draft PR remains at its earlier head. Issues remain open; no
acceptance boxes were closed. Exact guest behavior, whole-descendant termination,
stack concurrency/network/data/rollback, boot/crash/OS restrictions, browser/provider
flows, real catalog recipe/license evidence and independent regression execution
remain part of the full implementation and final integrated acceptance.
