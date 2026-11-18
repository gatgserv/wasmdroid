# Selected Wi-Fi sharing and reviewed reports: local verification

Tested source: `07e5797194aecee42b190e0f570881f35311d402` (author and committer
gatgserv). The checks below ran on the matching source immediately before that
local commit; the recorded JVM rerun also passed after the commit. Documentation
and the live status checkpoint are separate. No push or remote CI was performed.

## Scope and independent review

This source connects the retained LAN transport to the actual generation-owned
ServiceNetwork, private FFI descriptor binder, ordered Android Wi-Fi selection,
common-owner/worker controls and the English default-off address/QR dialog.
Reports follow the owner's preview-then-GitHub-form selection. The English
sharing/report scheme preceded GUI implementation. Guest internet/LAN egress,
hard worker memory/CPU limits and complete browser egress isolation remain
Unsupported; no useful catalog recipe was promoted by these checks.

Independent source review found and the final source corrected:

- Native closed receipt and active-slot release must occur in one ordered
  critical section, after actual forwarder cleanup.
- Off must hide the address/QR immediately. QR output must match its source URI;
  delayed Copy must revalidate the exact current visible/unlocked frame/grant.
- Stop during the initial pending On interval needs its own lifetime-owned
  cancellation, even before a status exposes an epoch. Owner actual-send
  admission and worker Main-queue reservation/dispatch CAS now fence it.
- Control-capacity rejection must return an exact canonical no-native receipt
  only for a verified unowned port; malformed or already-owned requests cannot
  fabricate closure or mutate another grant.

Independent source review found no further issues in these corrections.

## Native and Android checks

Commands ran sequentially with Rustup, JDK21, SDK36 and NDK28.2; local installation
paths and generated build products are not committed.

```powershell
cargo fmt --all
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi --lib
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo check --locked -p wasmdroid-wasmer-local --tests --target aarch64-linux-android --release --no-default-features --features cranelift
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

| Check | Actual result | Scope limit |
| --- | --- | --- |
| Engine library |19/19 PASS,2.09s; combined native test compilation14.23s | Includes12 retained LAN real-socket cases; not all integration suites |
| FFI library |3/3 PASS,0s | Two strict LAN wire cases and existing catalog input case |
| Selected-crate Clippy | PASS2.28s, warnings denied | Engine/FFI all targets; not a separate driver lint certification |
| Android driver cfg(test) | PASS3.04s | Nine new driver definitions compile; not executed |
| Final Android compilation | PASS19s;33 tasks,4 executed; native release8.87s | Matching native library, generated UniFFI, main and Android-test Kotlin; no test execution or install |
| Formatting / whitespace | PASS | CRLF normalization notices are not whitespace failures |

Earlier Android passes at36s and31s preceded the final cancellation/capacity
corrections and do not certify final source. The first had an unused native
import, the second nullable-selection Kotlin warnings; both were corrected.
The final19s build had no reported Kotlin warnings. No build remains running.

## Selected pure JVM definitions

The ignored local `RunPureChecks.java` uses JUnit Requests with explicit platform
method exclusions, asserts the expected count and zero ignored/failing cases,
and runs only the six classes below. The actual source models/tests were compiled
using cached Kotlin2.2.21 tooling and the app's generated BuildConfig. SDK36's
official android.jar resolves unused Android signatures; it does not emulate
Android. No fake ConnectivityManager, Network, FD or Intent result was supplied.

The recorded rerun invocation was:

```powershell
& $javaTool -cp "$modelClasses;$junitJar;$hamcrestJar;$stdlibJar;$androidJar;$zxingJar" RunPureChecks
```

Here `modelClasses` was `.local/models-host-20261117`, `javaTool` the JDK21 Java
launcher, and classpath artifacts were JUnit4.13.2, Hamcrest-core1.3,
Kotlin-stdlib2.2.21, Android SDK36 stubs and ZXing-core3.5.4. Compiler setup also
used Kotlin compiler/script/daemon2.2.21, reflect1.6.10, coroutines-core-jvm1.8.0
and annotations13.0. This ad hoc local runner is not a new Gradle/CI test gate.

| Class | Executed / result | Recorded elapsed |
| --- | --- | --- |
| DiagnosticReportTest |8/8 PASS |50ms |
| LanNetworkSelectionTest |12/12 PASS |15ms |
| DiagnosticSharingTest |1/1 PASS |1ms |
| LanQrEncoderTest |2/2 PASS |61ms |
| LanStartIntentTest |2/2 PASS |14ms |
| LanEnableDispatchTest |2/2 PASS |2ms |

Total **27/27 PASS**. The prior final host invocation also passed27/27; small
timings are measurements of this runner, not performance claims. QR tests decode
the actual generated matrix to the exact URI and reject foreign authorities or
oversize input. Cancellation tests include real competing host threads.

Four explicitly excluded methods require actual Android:
`androidAdapterReadsOnlyCurrentSdkAndAllowlistedSupportedAbis`,
`actualAndroidDuplicateIsClosedWhileNativeOwnerDescriptorRemainsOpen`,
`actualAndroidBindFailureStillClosesOnlyTheDuplicate` and
`issueFormContainsExactlyTheReviewedTextAndFixedOwnerDestination`.
Seven LanSharingCoordinator actor definitions also only compile in this batch;
they are not part of the host runner. Thus38 new Android definitions comprise27
host-executed and11 compiled-only, plus nine separate driver definitions that
only cross-compile. Compilation is never counted as a passed test.

## Static inputs and limitations

Parsed all12 main resource XML files and the manifest; no duplicate keys in
`res/values`. Pinned local QR encoding uses ZXing core3.5.4. Its tagged upstream
LICENSE and NOTICE are bundled by prepareAppAssets; source hashes:

```text
LICENSE 3F62881F0566227A24B12E5A754CC79F39AAA94883038E95C94812E1F50AF42F
NOTICE  4DB4B60AACE29EC6C5C4F13CA9CAC1930D3C0C54BCC67DCEEAD0C10771120E66
```

This verifies packaging inputs, not a completed transitive release/legal audit.
Ignored logs: `.local/lan-reports-native-final.log`,
`.local/lan-reports-clippy-final.log`, `.local/lan-reports-driver-tests-final.log`,
`.local/lan-reports-android-final.log`, `.local/lan-reports-host-final.log` and
`.local/lan-reports-host-recorded.log`.

No physical device/test APK installation, actual Wi-Fi Network/FD binding,
peer-device HTTP, QR scan, blocked/interface/permission loss, stop/death/drain,
guest/owner restart, browser-form launch, rotation/accessibility/visual review,
full product acceptance, full lint, remote CI, push, merge or release occurred.
These remain explicit acceptance prerequisites. Actual device work is deferred
until implementation completion as requested. Existing issues stay open.
