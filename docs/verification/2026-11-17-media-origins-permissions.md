# Media GUI, stable origins and outbound permission contract

Date:2026-11-17. Exact source revision:
`b4476cf886d47ab307181d56b60a61183725a6db`, branch
`codex/stage0-foundation`. Author and committer: `gatgserv`.
This is a local source/integration checkpoint. The full product goal, issues and
physical/provider/visual acceptance remain open. No push, remote CI, merge,
release, production catalog promotion or phone execution occurred.

## Source and independent review

- Native Store icons, details/gallery/full preview, unavailable/retry/legacy
  fallback and image credits follow the English proposal shown before GUI edits.
  One retained AppModel owns the decoder. Exact source incarnation plus raw entry
  travels through parse/decode/publication/install. Synchronous guards withhold
  stale images/credits before effect cleanup; late results and install callbacks
  recheck current authority. Native exact remote selection remains authoritative.
- Independent review found an actual owner seam: a retained old engine's finally
  could capture the new ready epoch inside catalog refresh. Actual connected-
  engine identity is now checked before reads and after awaits; disconnect clears
  the token and next-read throttle. General refresh final state/focus publication
  also checks its captured epoch/engine. Narrow readback was clear. This race was
  source-reviewed, not executed through actual Android ServiceConnection.
- Migration006 and opaque actual identity/spec/revision/run-authority capture
  retain real bound TCP listeners and durable per-profile/guest-port history.
  Saved busy ports fail without fallback; races/SQL failures cannot partially
  claim a listener set. History survives stop/retry/removal and known active
  assignments migrate before recovery. Legacy deleted history is unrecoverable;
  explicit saved-origin changes and ambiguous guest mappings remain scoped
  Unsupported pending their review flow. Complete/exclusive browser flags stay
  false. This does not prove actual localStorage persistence yet.
- Separate additive `lan_access` defaults false and omits false serialization,
  preserving legacy bytes. Normal single catalog preparation uses a validated
  effective clone with both outbound grants false; original signed requests are
  unchanged. Stack tampering rejects either positive flag. Positive runtime
  Internet/LAN still remains Unsupported until platform/policy/consent wiring.
- A real schema3 media-stack regression found typed preparation still selected
  schema2. It now chooses3 for present media and2 for legacy absence; a new test
  exercises actual managed preparation rather than parse alone.
- Owner approved LAN-only DNS through the same captured network, with resolved
  TCP destinations independently checked. The next app-default/VPN adapter and
  destination policy remain proposals, not enabled product behavior.

The other two lanes independently read media source/selection handling and
permission/origin boundaries. Root and catalog lane read the final portable-data
correction. No concrete residual scoped finding remained after correction.
Read-review is not device/network/Compose proof.

## Executed native regression suite

Windows host, existing Rust toolchain and locked dependencies:

```text
cargo fmt --all
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi --no-fail-fast
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all --check
```

**247/247 PASS =241 engine +6 FFI**,0 failed/ignored in the host-selected suites.
Test compilation22.54s; Clippy9.37s. Zero-case documentation suites completed.
This is the complete selected Windows crate suite, including existing independent
regressions; it is not247 new tests or execution of Unix-only/Android cases.

Notable scope:18 real socket/metadata origin cases0.36s;8 outbound Boolean/
configuration/effective-grant cases0.08s;6 catalog preparation cases0.34s;
6 data cases3.60s; FFI6 cases0.00s. All other selected lifecycle, automation,
catalog, install, local import, stack, storage and terminal regressions passed.

Initial failures are preserved in separate logs:

1. New origin fixtures assumed initial revision1; actual creation is revision0.
   Fixtures now read actual instance/authority revisions, including negative
   publication tests, so expected origin refusal cannot pass merely from stale CAS.
   No production revision contract or acceptance assertion was weakened.
2. An older unknown-catalog-schema fixture used2, now a supported contract. It now
   uses4 while retaining the unknown-schema rejection assertion.
3. Full suite reproduced an existing real Windows nested-backup failure:
   `backup_roundtrip...` failed in streamed `reader.read(65536)` with permission
   denied. Internal PathBuf join/push produced backslashes rejected by the volume
   confinement boundary. Internal joins now append literal slash using OsString,
   validating both operands/result; ensure_parent validates its whole input before
   creating anything. External backslash/traversal rejection and permissions are
   unchanged. The strengthened roundtrip verifies128KiB binary content, long
   Unicode nested/deeper names, nested empty folders and independent tar raw slash
   names; a new depth3 import verifies binary bytes and no malicious side effects.
   Crafted backslash archive rejection remains. Final six data cases all pass.

## Executed pure JVM selection checks

Fresh Kotlin2.2.21/JDK21 compilation, JVM target17, cached
JUnit4.13.2/Hamcrest1.3. Sources `CatalogPresentation.kt` and
`CatalogPresentationTest.kt`, then:

```text
org.junit.runner.JUnitCore dev.wasmdroid.app.CatalogPresentationTest
```

**6/6 PASS,0.009s**. Local exact runner:
`.local/test-catalog-presentation.ps1`. It covers unavailable/absent source,
changed source with identical pixels, changed same-ID credits/entry, source return
with a new incarnation and cache content without authority. No Android or Compose
methods ran. Total executed current checks **253 =247 native +6 pure JVM**.

## Matching Android compilation

JDK21, existing Android SDK/NDK28.2.13676358, arm64-v8a release native Cranelift:

```text
cargo check --locked -p wasmdroid-wasmer-local --tests --target aarch64-linux-android --release --no-default-features --features cranelift
android/gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

All passed. Final driver cfg(test)5.18s; Android **BUILD SUCCESSFUL in26s**,
native23.58s,33 tasks:1 executed,32 up-to-date. Main/test Kotlin were unchanged
from the preceding successful42s build (11 executed tasks), which compiled the
new GUI, strings and generated effective-spec binding. The final build regenerated
matching native/bindings for the portable-data correction. Final session29136
terminated with exit0; no other builder remained live.

New LAN-preflight definition compiled, not executed, alongside21 prior outbound
transport definitions. Three actual Android decoder/JSON/cache definitions remain
compile-only. No actual BitmapFactory, Compose layout/gesture, WebView/Notes,
Android DNS/bind/VPN, SAF, worker stop/death or positive permission acceptance was
run here. Earlier superseded native/Android outputs are not final-source proof.

Detailed outputs remain in ignored `.local/media-gui-origins-native-integrated.log`,
`media-gui-origins-clippy-integrated.log`, `media-gui-origins-driver-integrated.log`,
`media-gui-origins-android-integrated.log`, plus prior failure/compile logs and
`catalog-presentation-host.log`. Final integrated device and CI phases remain
deferred by the owner until implementation completion.
Progress is recorded on existing [issue #3](https://github.com/gatgserv/wasmdroid/issues/3#issuecomment-6960512129),
without closing acceptance criteria or changing the submitted PR revision.
