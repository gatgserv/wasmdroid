# Reviewed group, automation, catalog and browser controls

Source revision: `9b370dbdf2c00f1e13eece8e82a45179d7a29fae`.
Checks ran on the matching source tree before the local source commit. This
record is a documentation-only follow-up.

This checkpoint covers the shared AppModel, navigation, common owner and native
inventory changes for the English-language controls.

## Implemented scope

- Private bounded group editor with actual native source, version/member/artifact
  preview and exact reviewed apply. Source epochs reject callbacks from earlier
  groups/reloads. Pending switch blocks apply; admitted publication survives
  leaving the UI and a durable ACK clears the private draft.
- Read-only native public/imported package inventory, explicit public download
  through existing owner-retained operations and mandatory new review afterward.
  Imported digests require exact local import; no download/create/start is implicit.
- Independent default-off boot and Never/On failure/Always restart controls in the
  existing versioned configuration form. Real membership gates managed groups.
  Events update only the changed field of the current private draft. Save does
  not authorize a run. Hard CPU/memory enforcement remains Unsupported.
- Catalog configuration/freshness/version/accepted/expiry, refresh and exact
  generation cancellation. Remote install requires actual configured trust and
  a current snapshot; the native sequence/digest selection remains authoritative.
  Failure/cancel accurately describes the retained bundled or accepted source.
- Explicit browser-data controls, including apps without mounted datasets. The
  owner rechecks the exact private profile token/revision before provider work.
  Confirm binds the actual displayed opaque prompt object, rejecting stale
  dialogs even after same-ID instance recreation. Actual provider ACK and exact
  subsequent state govern success; active views and interruption remain visible.

## Commands and executed results

Windows, existing rustup/JDK21/Android SDK/NDK toolchain:

```powershell
cargo test --locked -p wasmdroid-engine --test stack_requirements --test local_stacks --test stack_updates
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all --check
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

- Selected local hermetic engine regressions: **15/15 PASS** (inventory3,
  imported groups6, stopped updates6). Inventory cases verify no metadata/event/
  dataset mutation, no private env leakage, deduplication, distinct installed/
  imported actions and rejection of unsupported references/input.
- Clippy PASS7.38s with `-D warnings`; final formatting/diff checks PASS.
- Final Android main/instrumentation Kotlin compile PASS10s,33 tasks/3 executed,
  using actual matching arm64 release-profile native/bindings from the preceding
 23.16s native build. This is debug Android compilation, not guest execution or
  a release/signing result.
- Default string XML parsed:391 unique string keys. The residual `values-ro`
  directory contains no files; production locale filtering remains English.

The first integrated Gradle attempt failed after native generation because root
omitted an AppModel argument for the import screen's new package pane. Adding
the explicit parameter/callsite fixed the error; the next compile passed. An
existing `TerminalView.kt`115 Java `characters` deprecation warning remains
visible. Android compilation is not claimed warning-free; no gate was weakened.

## Review findings and unexecuted checks

Independent source review corrected pending-switch/apply cancellation, stale
text callbacks after source reload, whole-draft automation event overwrite,
same-ID browser identity recreation, and old-dialog callbacks consuming newer
confirmations. Definitions cover the concrete orderings, rather than merely
checking presentation strings.

Compiled **32 new Android definitions/extensions**: group controller5, automation
configuration10, catalog presentation4, browser-data controller12 and one actual
common-owner/native identity-recreation case. These assertions have **not run**.
Existing import/configuration fixtures were adapted to required backend methods.
Neither compilation nor source review is device/provider/runtime evidence.

No phone installation, rendered GUI/TalkBack/rotation checks, real reboot/crash,
provider clearing, guest group HTTP/data/descendant acceptance, full regression
suite/lint, CI, push, merge or deployment was executed for this batch. Original
issues remain open and the published draft PR head is unchanged. Full browser
egress/atomic port reuse/stable origin, real curated multi-service recipes/assets/
licenses, hard resource enforcement, LAN/QR, widgets/tiles and final integrated
acceptance remain in the full Android scope.
