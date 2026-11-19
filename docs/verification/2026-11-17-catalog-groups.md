# Pinned multi-service catalog preparation

Source revision: `1469b6c84b1ecd859ad6f06df308ab28614823ee`.
Executed on the matching source tree before the local source commit; following
status/evidence changes are documentation only. No new production GUI was added.

## Scope and review

Catalog schema1 retains the single InstanceSpec wire shape. Schema2 additionally
supports strict Compose group recipes with an exact public/version/digest
inventory, primary service and matching signed per-artifact review attestations.
Pure validation rejects unsupported host/external-data/dependency/port authority
before resolution/download. This does not prove actual package compatibility or
license permission; the production catalog remains empty.

The installer resolves/checks all roots before downloads, then uses a shared
maintenance lease, writer, union dependency graph and cumulative budgets. One
publication transaction commits all image metadata. Accepted cancellation before
publication or SQL failure leaves no partial image metadata; verified CAS bytes
are preserved. Legacy mutable-reference single pulls keep their existing shape.

The common owner dispatches single versus group preparation. Native preparation
re-reads actual installed roots, compares the normalized plan to catalog pins,
performs runtime preflight before datasets and uses existing atomic managed
membership/private-volume authority. It ignores caller graph JSON. Worker loading
rechecks the actual root against its restrictive catalog digest pin. Descriptive
UI/web fields survive explicit group editing; they grant no membership, ports or
catalog-verification authority. An explicit advanced review refreshes the pin;
the signed Store selection does not authorize a changed recipe.

Independent source review found and corrected exact-reference substitution,
the planning-to-publication pin gap, the worker loaded-root gap, presentation
metadata breaking lossless edit-source roundtrip, and raw comma-separated package
objects being accepted by a one-entry wrapper. No test or gate was disabled.

## Executed local commands

```powershell
cargo test --locked -p wasmdroid-engine --test multi_root_install --test catalog_stacks --test catalog_updates --test catalog_preparation --test graph_transactions --test operation_control --test stack_updates --test local_stacks
cargo test --locked -p wasmdroid-ffi --lib
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

| Local suite | Executed result |
| --- | --- |
| Multi-root installer |13/13 PASS |
| Pure catalog stacks |12/12 PASS |
| Signed catalog updates |20/20 PASS |
| Catalog preparation |4/4 PASS |
| Existing graph transactions |12/12 PASS |
| Operation control |3/3 PASS |
| Existing stack updates |6/6 PASS |
| Imported local groups |6/6 PASS |
| FFI one-entry boundary |1/1 PASS |

Total **77/77 local hermetic regressions PASS** across engine76 and FFI1.
Clippy PASS8.67s with no warnings; formatting/diff checks PASS. Android native
arm64 release-profile driver/engine/FFI built in19.17s, matching bindings generated,
and main/instrumentation Kotlin compile PASS25s (33 tasks/4 executed). Existing
JDK21/SDK/NDK toolchain was used. This is Android debug compilation, not APK
execution, release signing, a full suite or remote CI.

The initial preparation run passed2/4; two new fixtures incorrectly attempted an
immutable digest overwrite through StateDb, which correctly rejected it. The
final tests retain that rejection assertion and inject corrupted metadata only
through SQL in uniquely owned disposable databases. Runtime refusal, unexpected
metadata drift, worker digest substitution and publication retry are checked
without weakening production immutability. Final selected run passed all76 engine
cases; the separate FFI case passed too.

## Remaining evidence

No device/guest HTTP, actual multi-service web/database application, network
download/cancellation, provider/visual/accessibility, reboot/crash, full regression
suite/lint, CI, push, merge or deployment was run in this batch. No acceptance box
was closed. Actual application assets/seeding, reviewed licenses and combined
current-device evidence remain required before any curated entry is shipped.

Normal combined-application cards and scoped widget/Quick Settings controls still
require the next conversation scheme before production GUI. The accompanying
prototype interaction checks cover only example-state transitions, scoped target
selection, pending-start cancellation, removal confirmation and saved public
presentation fields. They are not rendered Android or native-controller evidence.
Full browser transport/origin guarantees, hard resource enforcement, LAN/QR and
final independent/device acceptance remain part of the full active goal.
