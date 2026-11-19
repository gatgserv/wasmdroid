# Logical notification summary and unverified Notes recipe

Final source: `f6a44fb0a3a42055545703f6ede84a6de89c8866`.
Main source: `b4ea540bc9a1f1ce0637bd44e6ffad3c597aefe3`.
The final delta adds an explicit notification-permission prerequisite to the live
acceptance definition and its documentation. It changes no production code or
Rust source. All commits are authored and committed by `gatgserv`.

## Implemented behavior

The foreground display now counts actual native groups once, separately from
unchanged worker/boot/restart ownership. English plurals distinguish active,
starting and stopping applications; retained background work is a separate fact.
READY, Stopping and configuration admission publish snapshots even when worker
counts are equal. Existing idle permits, slot limits and data/IO retention remain
authoritative. Background-only reads cannot establish a new foreground service.
Independent review found and corrected those missing phase/admission publications.

The pinned Notes schema2 example combines the existing exact first-party HTML
with the reviewed candidate StaticWebServer command and scoped read-only mount.
It remains `android_verified=false`, evidence null, outside the shipped catalog.
Structural checks assert that curated preparation refuses it before runtime or
volume effects. The current-owner Android harness deliberately uses advanced
pull/create/import/start APIs; it does not bypass that curated gate or certify
the catalog seed receipt path. Package-source/SBOM correspondence, complete
third-party review and current physical recipe acceptance remain open.

The explicit live harness requires `-NotesLive`, runtime arm64, an unlocked
device, permitted notifications and an idle owner. Its test-only assets are not
packaged into the application. It defines exact seed import/no-overwrite, real
HTTP from two private roots, separate host ports, selective stop/restart, peer
generation retention, browser lease revocation, notification counts and scoped
cleanup. Cleanup requires captured native group birth/state before stop/removal;
uncertain acknowledgements are preserved for inspection. No shared image/CAS or
unrelated application cleanup is permitted. ABI checks do not prove physical
hardware: later acceptance must record selected-device inventory separately.

## Executed local checks

```powershell
cargo test --locked -p wasmdroid-engine --test notes_draft --test catalog_preparation --test catalog_stacks
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all --check
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

| Check | Observed result |
| --- | --- |
| Actual Notes draft/schema/pins/asset bytes/unverified gate |3/3 PASS |
| Catalog preparation and seed/publication recovery |5/5 PASS |
| Catalog structural contracts and encoded limits |13/13 PASS |
| Pure notification JVM cases |12/12 PASS; JUnit0.023 s |
| Clippy, all selected crates/targets, warnings denied |PASS1.50 s |
| Rust formatting and staged/unstaged whitespace |PASS |
| Main Android integration build |PASS16 s; native release1.73 s;33 tasks11 executed |
| Final permission-prerequisite Android build |PASS4 s; native1.00 s;33 tasks2 executed |
| Android runner PowerShell AST and English resource XML |PASS |

The pure JVM run compiled only `NotificationSummary.kt` and
`NotificationSummaryTest.kt` together with the already cached Kotlin2.2.21
compiler, JDK21 and JUnit4.13.2/Hamcrest1.3. No dependency downloads or Android
processes were involved. Exact compiler/run arguments were:

```powershell
& $java -cp $compilerCp org.jetbrains.kotlin.cli.jvm.K2JVMCompiler -no-stdlib -no-reflect -jvm-target 17 -classpath $testCp -d $hostOutput android/app/src/main/java/dev/wasmdroid/app/engine/NotificationSummary.kt android/app/src/androidTest/java/dev/wasmdroid/app/NotificationSummaryTest.kt
& $java -cp "$hostOutput;$testCp" org.junit.runner.JUnitCore dev.wasmdroid.app.NotificationSummaryTest
```

`compilerCp` comprised cached compiler/stdlib/script-runtime/daemon2.2.21,
reflect1.6.10, coroutines-core-jvm1.8.0 and annotations13.0; `testCp` was
stdlib2.2.21/JUnit4.13.2/Hamcrest1.3. Each cached artifact/version was required to
resolve exactly one JAR. An initial internal data-class constructor emitted a
copy-visibility warning; making the presentation value constructor public fixed
it, and the final compilation and12 tests passed without warnings.

The initial Android invocation failed before source compilation because its
process lacked `ANDROID_HOME`. Setting the existing SDK through the process
environment resolved this; both subsequent builds passed. The initial Notes
test formatting check failed; rustfmt corrected only formatting before the final
21-case native run, Clippy and format check. No gate or assertion was weakened.

Local logs: `.local/notification-summary-host.log`,
`.local/notes-draft-native-final.log`, `.local/notification-notes-clippy.log`,
`.local/notification-notes-android.log`, and
`.local/notification-notes-permission-final.log`. These ignored outputs are not
public artifacts. The final source has the same production/main and Rust trees
as the main source, so the earlier matching checks retain that scope.

## Not executed or certified

The new live Android definition and changed native-owner data-observer definition
were **compiled, not executed**. Pure JVM execution covers only the12 projection
cases, never actual foreground permission, notification posting, Binder, guest
behavior, HTTP, provider/browser storage or cleanup.

No selected-device install, physical acceptance, full suite/lint, rendered
Compose/TalkBack/rotation, push, new remote CI, merge or release occurred. The
production catalog stays empty. Old physical HTTP evidence retains its historical
revision and does not certify this recipe/current owner. Full browser egress,
atomic port reuse and stable origin remain open; profile retention does not
prove localStorage continuity when automatic host ports change.

LAN transport development is a separate ongoing source batch. English LAN/QR
and error/report schemes are prepared for presentation before GUI implementation;
their local script syntax and static selector references passed, but their
rendering was not inspected. They do not establish actual sharing/report behavior.
The report destination question remains a product preference; no report was sent.
