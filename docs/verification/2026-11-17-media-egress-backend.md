# Catalog media and scoped outbound dependency checkpoint

Date: 2026-11-17. Exact local source revision:
`ef2d67c6f4e8a418c701387e14b030a1f432364e` on
`codex/stage0-foundation`. Author and committer are `gatgserv`.
This is a source/selected-check checkpoint, not full product acceptance.
No push, remote CI, merge, release or physical-device execution was performed.
The published draft PR #2 remains on its earlier submitted revision.

## Implemented scope

- Catalog schema3 bounded inline PNG/JPEG media, exact payload digest/size and
  metadata validation, compatible absent fields in schemas1/2 and signed review
  binding. The one-entry FFI selects the feature schema while preserving its
  existing strict root and128KiB limit. Production catalog remains empty.
- Android pure payload model, bounded strict media JSON adapter and one-active/
  eight-admitted native decoder with8MiB retained cache. Actual MIME/dimensions
  precede pixel decoding; cache eviction never recycles published bitmaps.
  Media GUI is not wired before its English conversation proposal.
- A Rust-only scoped outbound TCP dependency: mandatory destination policy and
  selected-Network provider, actual retained work/descriptor/engine ownership,
  generation/revision/grant/network fencing, private-alias reservation and actual
  drain before terminal success. Current product preflight remains Unsupported.
  No Android resolver/binder or positive permission path is supplied here.
- Owner-approved next direction: separate default-off Internet and outbound-LAN
  permissions. Inbound Share and NetworkMode::Lan retain their existing meanings;
  signed recipe requests must not silently become operator consent on Install.

## Executed native checks

Windows host, locked dependencies and existing Rust toolchain:

```text
cargo fmt -p wasmdroid-engine -p wasmdroid-ffi
cargo test --locked -p wasmdroid-engine --test catalog_media --test catalog_updates --test catalog_stacks --test catalog_assets --test catalog_preparation
cargo test --locked -p wasmdroid-ffi --lib
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

All passed. Engine test compilation11.91s:

| Suite | Passed | Reported execution |
| --- | ---: | ---: |
| catalog_assets |12|0.11s|
| catalog_media |16|0.07s|
| catalog_preparation |5|0.14s|
| catalog_stacks |13|0.01s|
| catalog_updates |20|0.19s|
| FFI library |4|0.00s|

Engine total66/66 and FFI4/4; FFI compilation5.67s. Selected-crate Clippy passed
in10.57s. These runs are unchanged by the later driver-only corrections and the
Android decoder/test additions. Clippy here does not include the Wasmer driver.
The schema rejection fixture now uses4 because3 is the implemented media schema.

Media regressions cover legacy serialization, feature/schema/null/unknown-field
rejection, cumulative bounds, canonical base64/digest/dimensions, actual PNG chunk
CRC/type framing, JPEG envelope restrictions and exact signed media review.
They do not certify decompressed pixels, screenshot truth or source/license
approval. Generated first-party pixel fixtures are contract data only.

## Executed pure JVM checks

Fresh Kotlin2.2.21 compilation of `CatalogMediaModel.kt` and
`CatalogMediaModelTest.kt`, JDK21, JVM target17, cached JUnit4.13.2/Hamcrest1.3;
then `org.junit.runner.JUnitCore dev.wasmdroid.app.CatalogMediaModelTest`.
The local runner is `.local/test-catalog-media-models.ps1` and invokes the actual
compiler/test sources with the cached jars. **5/5 PASS,0.045s**. No Android JSON,
BitmapFactory or platform stub methods ran in this host suite.

Executed selected cases total **75 =66 engine +4 FFI +5 pure JVM**. This count
includes retained regressions and must not be reported as75 new media tests.

## Final matching Android compilation

Existing Android SDK/NDK28.2.13676358 and JDK21; arm64-v8a release native runtime,
default Cranelift and generated matching UniFFI Kotlin bindings:

```text
cargo fmt --all
cargo fmt --all --check
cargo check --locked -p wasmdroid-wasmer-local --tests --target aarch64-linux-android --release --no-default-features --features cranelift
android/gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

All passed. Driver check5.15s. Android **BUILD SUCCESSFUL in23s**,33 actionable
tasks:4 executed,29 up-to-date; native rebuild18.98s. Both main Kotlin and
AndroidTest Kotlin compiled. Builder session59325 terminated with exit0.

**21 new driver definitions (17 transport/provider/namespace +4 alias) were
cross-compiled, not executed.** Three actual Android decoder/JSON/cache definitions
also compiled, not executed. They include generated PNG/JPEG decode, validation,
cache identity/eviction without recycling a held consumer, and strict JSON frames.
These are not actual device/provider results.

Earlier driver checks6.15s/2.90s and Android builds36s/19s preceded the final
corrections; they are superseded by the final matching compilation above.
Root reviewed integration/lifecycle ordering; the independent catalog lane read
back the final narrow corrections with no remaining concrete scoped finding:

- Reject malformed/private DNS spellings before any external provider call.
- Full guest client drop cancels late connect and established forwarding, while
  pending provider ownership persists until actual return; write half-close is
  distinct and continues to permit its response.
- Recheck actual current namespace/private aliases/manager after DNS completion.
- Interactive Ctrl-C revokes current work without permanently fencing future
  explicit captures. Runtime stop, including StopSignal::Interrupt, kill and Drop
  permanently fence admission.
- Replacement permits forward durable lifecycle revisions of the same exact
  worker/specification, rejects rollback, requires a strictly newer grant and
  actual prior drain. Factory checks actual current record/spec remain exact.

Read-review and compiled regression definitions do not prove actual Android
Network binding, resolver cancellation, provider completion or socket cleanup.

## Limits and remaining evidence

No actual GUI media rendering, phone install, image/provider execution, visual/
accessibility/rotation acceptance, current recipe install or positive guest
Internet/LAN flow has run. Cache bytes are not a process-RSS/UI-reference bound;
native framing is not full image decoding. No browser complete isolation,
exclusive-generation gateway or hard worker memory/CPU enforcement is claimed.
The stable-origin document is a read-only proposal; historical deleted origins
cannot be reconstructed. Recipe/media/legal/hosting decisions remain open.

Local logs retain the detailed outputs in `.local/catalog-media-native.log`,
`catalog-media-ffi.log`, `catalog-media-clippy.log`, `catalog-media-host-models.log`,
`egress-definitions-android-final.log` and `media-egress-android-final.log`.
The full implementation goal and related acceptance issues remain open.
Progress was recorded on existing [issue #3](https://github.com/gatgserv/wasmdroid/issues/3#issuecomment-6959573991)
without closing acceptance criteria or changing the submitted PR revision.
