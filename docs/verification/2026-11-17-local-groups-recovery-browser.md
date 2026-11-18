# Local groups, durable recovery and private browser identity

Source revision: `e54857f0b61217b17e23aeea10db9ae3fa9ce042`.
Checks ran on the matching source tree before the local source commit. Only the
live status/evidence record changes after this revision. This is local integration
evidence, separate from final integrated acceptance and remote CI.

## Scope

- Imported exact-digest WebC/core-Wasm group planning, dependency/metadata pins,
  stopped updates and worker-side artifact verification.
- Schema-5 durable standalone run/stop authority, same-boot cold-owner recovery,
  exact-generation acknowledgements and fail-closed legacy defaults.
- Private native browser identity, unchanged legacy profile hashes and atomic
  identity transfer during same-spec Dead replacement. Fresh applications and
  package replacements receive fresh profile identities.
- Common-owner browser session leases, old-generation view fences and explicit
  provider-acknowledged per-profile website-data clearing. No new GUI controls
  were added before their English screen proposals.

## Executed local checks

Windows host, rustup Rust 1.95.0; existing JDK 21/SDK/NDK toolchain for Android.

| Command | Result |
| --- | --- |
| `cargo test --locked -p wasmdroid-engine --test automation --test browser_identity --test local_stacks --test stack_updates --test configuration --test catalog_updates` | Automation10, browser identity4, catalog updates16, configuration6 and local stacks6 passed. Stack updates initially5/6; see correction below. |
| `cargo test --locked -p wasmdroid-engine --test stack_updates` | Final6/6 PASS; test execution1.07s, compilation3.12s. Total selected regressions48/48 PASS across the two runs. |
| `cargo fmt --all --check` | PASS. |
| `cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings` | PASS9.22s, no warnings. |
| `.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain` | PASS38s,33 tasks/4 executed. Actual arm64 release-profile native driver/engine/FFI27s rounded, matching generated bindings, main and instrumentation Kotlin compiled. Android debug compilation; no APK execution or release signing. |
| `git diff --check` | PASS before source commit. |

The native automation/browser-identity suites execute SQLite authority/migration/
replacement checks. They do not execute Android Binder death, WebView or reboot.
Browser, coordinator and imported-group Android definitions compiled but remain
unexecuted on the device.

## Failures and corrections

The first selected run stopped after14 passing tests: Windows returned740 before
launching the catalog executable. Test binaries had no execution-level manifest.
A Windows-MSVC test-only `asInvoker` manifest now explicitly selects the caller's
ordinary token. No UAC/admin launch or machine-policy change occurred. Relinking
allowed all16 catalog assertions to execute and pass. Android receives no Windows
linker flags. Reference contracts: [Microsoft execution levels](https://learn.microsoft.com/en-us/windows/win32/secbp/running-with-administrator-privileges),
[linker manifest input](https://learn.microsoft.com/en-us/cpp/build/reference/manifestinput-specify-manifest-input?view=msvc-170),
[Cargo test-only linker arguments](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rustc-link-arg-tests).

The subsequent run exposed a rollback-test expectation mismatch: total event
count increased6 versus5 because independently committed preparation correctly
retained a labelled dataset and its `volume_created` audit event for retry. The
corrected regression requires all prior events to remain byte-equivalent through
serde, exactly one expected volume event with no instance owner, unchanged stack/
initial definition and three prepared datasets. Successful retry must reuse that
dataset without a duplicate creation event. Production audit behavior was not
changed and no assertion was disabled. Final stack suite6/6 passed.

Initial Clippy findings were corrected in source without allowances, including
conditional structure, redundant closures/borrows, sorting, integer predicates
and the lifecycle helper argument shape. Prior compile failures remain historical;
the final integrated gate above passed.

## Remaining evidence and product scope

No physical-device run, actual guest HTTP/group execution, cold-owner crash,
reboot, forced-stop behavior, provider clearing, screenshots, accessibility,
lint/full suite, push or CI was executed for this batch. No issue acceptance box
was closed. Draft PR2 remains at its previously published head.

Stable profile bytes and cookie scope do not provide stable browser origins when
an automatically assigned host port changes. LocalStorage/IndexedDB/service-worker
access continuity, complete browser egress and atomic port-reuse isolation remain
open. Hard memory/CPU enforcement remains explicitly Unsupported. Real curated
web/database recipes, assets, licenses and catalog hosting/trust configuration
remain separate prerequisites. The four English screen proposals cover follow-on
group editing, automatic-start, catalog-update and browser-data controls; visual
prototype checks do not establish Android UI acceptance.
