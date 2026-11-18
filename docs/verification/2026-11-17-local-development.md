# Local integration during the development batch

This record contains compilation evidence only.
No new test execution, device install, visual acceptance, CI submission or
release is claimed. The full implementation goal and issues remain open.

## Source revisions

- `6850d343a9e26fc1ccbb49b52598d99d69ff43dc`: application data/SAF, portable per-volume
  backup/restore and dependency-aware image/cache maintenance.
- `daed0641b254fe11f1c6c6f6159f215a86a46d1a`: authenticated persistent compiled
  modules, actual WebC atom-hash verification and separate embedded web profiles;
  includes the preceding data implementation.

Both are local feature-branch commits attributed to `gatgserv`. They have not
been pushed during this development phase. The commands below ran on the source
tree captured by the second revision, before its local commit was created.

## Integration commands

Windows, rustup Rust 1.95.0, JDK 21, Android NDK 28.2.13676358, arm64-v8a,
Cranelift, optimized Rust release profile with Android debug variant:

```powershell
cargo fmt --all
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
git diff --check
cd android
.\gradlew.bat :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Results: Rust engine/FFI and their host test sources compile; Android native
runtime, generated UniFFI bindings, main Kotlin and instrumentation Kotlin sources
compile. The final Gradle invocation returned `BUILD SUCCESSFUL in 14s`.
`git diff --check` found no whitespace errors. Formatting is preparation, not a
passing `cargo fmt --check` acceptance result.

An initial direct Android `cargo check` lacked the NDK CC/AR/linker environment
and failed in ring's C build. The supported Gradle/native build path supplied the
toolchain and compiled successfully. A Windows canonical-path mismatch in the
new fingerprint build script and an incorrect Wasmer features accessor were
corrected before the final successful build. No failed check was bypassed or
weakened.

## Still pending

The new host data/archive/cache regressions and `WebProfileIsolationTest` are
defined and compile, but have not run. Rust tests, Clippy, Android lint, physical
service-group stop/persistence/SAF/provider/cache-hit regressions, browser network
security evaluation and visual/accessibility acceptance remain pending. The
earlier combined shared-process HTTP/CPU failures retain their historical report;
these compilation results do not supersede them.

Development still includes the remaining lifecycle trait integration, network
capabilities, live logs/terminal, curated recipes, resource/restart/stacks and
other Stage 2 scope. Public catalog hosting, report destination, Play account,
signing and distribution prerequisites remain separate owner/expert gates.

## Live output and native stdin/TTY integration

Source revision `272defea878cad582c74b0eb236cfc628ded94f9`, local feature branch,
author and committer `gatgserv`; no push/CI submission. The same Rust check and
Gradle commands above were run on this exact revision. Engine/FFI and neutral
host regression sources compiled. The final arm64 Gradle invocation completed
with `BUILD SUCCESSFUL in 11s`; native Wasmer, matching generated bindings, main
Kotlin and instrumentation sources compiled. A prior Kotlin String.length call
was corrected before the successful integrated source build.

The source now includes raw rotated live log frames, sequence/generation/since/
tail queries, a native advanced Logs screen, bounded virtual stdin, WASIX TTY
state/resize/interrupt/EOF and generation-scoped acknowledged control IPC. The
terminal emulator/keyboard/attach GUI remains pending. Plain logs are not treated
as a terminal implementation.

`engine/tests/log_streams.rs` and `LiveOutputTest` define independent byte,
pagination, rotation, crash-tail, cursor and UTF-8 regressions. The explicitly
selected `SupervisedTerminalTest` defines live Python TTY/long Unicode input,
virtual resize, pre-exit live output and stale-input rejection. These tests have
not run. New driver-local input queue unit tests are defined but were not built
by the Windows neutral host check or the production Android library build.
The Kotlin instrumentation sources did compile. Actual guest behavior is still
unproven by these compilation checks.

No new tests, Clippy, lint, device execution, visual acceptance, old CI polling or
new CI run occurred for this batch. All previous full-goal and combined stop
limitations remain open.

## Native terminal display and keyboard integration

Source revision `ddcd5cc42a0629f7920a6a78bd6bda053fad4278`, local feature branch,
author/committer `gatgserv`, no intermediate push or CI submission. The working
tree captured by this revision passed the same engine/FFI/all-neutral-target
compilation command and arm64 `compileDebugAndroidTestKotlin` command above.
The final Gradle result was `BUILD SUCCESSFUL in 3s`, incrementally reusing the
native runtime compiled in the preceding successful 20-second invocation.
Main and instrumentation Kotlin compilation completed. One compiler deprecation
warning remains for the legacy IME `KeyEvent.characters` fallback; no zero-warning
lint result is claimed.

The source includes the worker-owned bounded VT100 grid, immutable display pages,
small idle acknowledgements, device replies, optional final display persistence,
native Canvas/IME attachment, hardware/special keys, manual clipboard actions and
generation fencing. A virtual resize now updates dimensions without copying stale
echo/canonical flags over a guest's concurrent terminal-state change.

The dependency was inspected locally after `cargo info vt100`: version 0.18.1,
MIT, MSRV 1.85. Its cells bound text to 22 bytes; the no-std vte parser bounds OSC
storage to 1024 bytes. Both relevant feature graphs showed no vte std feature:

```powershell
cargo tree --locked -p wasmdroid-engine -e features -i vte@0.15.0
cargo tree --locked -p wasmdroid-ffi --features wasmer --target aarch64-linux-android -e features -i vte@0.15.0
```

The upstream MIT license was copied into `third_party/vt100/LICENSE` and the
generated Android license assets; this does not complete the full release notice
audit. `engine/tests/terminal_display.rs`, `TerminalKeyboardTest` and stronger
`SupervisedTerminalTest` assertions define parser/snapshot/IME/live/final-frame
regressions and compile, but were not executed. They are not physical terminal,
password echo, Python REPL/Bash/SQLite, rotation, accessibility, clipboard or
visual acceptance evidence. Tests/Clippy/lint/device/CI remain deferred to the
integrated phase, and the full Android implementation goal remains unfinished.

## Worker driver and process resource summaries

Source revision `d47cbdf778915102c2470bbd06c0eb2c54e77d7f`, local feature branch,
author and committer `gatgserv`. This source tree passed local integration
compilation with:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
# In android/, with the existing configured JDK/SDK and rustup PATH:
.\gradlew.bat :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

The final Rust engine/FFI/all-neutral-target check completed successfully in
1.58 seconds. The final Gradle invocation completed `BUILD SUCCESSFUL in 20s`,
including the optimized arm64 native runtime, generated matching UniFFI Kotlin,
main Kotlin and instrumentation source compilation. These commands compile;
they do not execute tests or establish real process behavior. Source was formatted
with `cargo fmt --all`; `git diff --check` passed before the source commit.

The service facade now uses the worker-resident RuntimeDriver. Termination goes
through the private host callback, outside native execution locks, and the common
controller still requires Binder death before resource release. A native stop
also schedules a bounded worker deadline before attempting an execution lock.
Live stats query only their owned generation and expose current whole-process
RSS, cumulative process CPU and a monotonic sample timestamp. Resource cards use
same-generation interval CPU percentages, actual MiB conversion and nullable
unavailable states; JVM/JIT overhead is explicitly included. Memory and CPU
limits remain unsupported, not silently converted to a sampling watchdog.

The resident-page parser regression and neutral target sources compiled on the
host. WorkerResourcesTest and the extended live Python capability/stats/stale-run
assertions compiled as instrumentation sources. Driver-local callback/lock unit
tests are defined but not compiled by the Windows neutral check or the production
Android library build. None of these new tests were run.

No physical-device counter measurements, resource UI visual/background/rotation
checks, native wait/remove acceptance, worker deadline/group-stop acceptance,
Clippy, lint, test execution, intermediate push, old CI polling or new CI run
occurred. The historical shared-process interruption failures remain visible.
See [the driver contract and pending acceptance](../design/driver-and-resources.md).

## Private service TCP namespace and forwarding lifetime

Source revision `c40f54d69c7ffa851f10f78d2dc026159b5931b2`, local feature branch,
author and committer `gatgserv`. The source captured by this revision passed:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
# In android/, using the existing configured JDK/SDK and rustup PATH:
.\gradlew.bat :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Final neutral host check: success in 0.46 seconds. Final Gradle check: `BUILD
SUCCESSFUL in 1s`, compiling the final Kotlin receipt change and reusing the
matching native runtime/bindings from the preceding successful 13-second build
(optimized arm64 native compilation 10.54 seconds). The preceding build compiled
the new SupervisedLoopbackTest instrumentation sources; the final invocation
confirmed them up to date. Formatting preparation used `cargo fmt --all`;
`git diff --check` passed before the source commit.

This adds the production worker's private TCP self-connect/internal/ephemeral
ports, exact listener lookup, static localhost resolution, bounded admission,
source/listener lifetime ownership, and separate network fence/revoke operations.
Only explicitly declared ports publish host ingress. The shared forwarder bounds
listener startup waiting, with established streams no longer subject to a blanket
30-second lifetime. Normal guest completion allows bounded forwarding drain and
records forwarding_drained; the flag is not remote application acknowledgement.
Explicit stop still revokes that namespace and the final Android/Binder death
gate remains required. Bounded recent completion receipts remove the immediate
fast-exit wait envelope race; eviction/owner recreation still yields unknown
result fields instead of attaching stale output.

The existing pinned MIT virtual-mio dependency is now a direct driver dependency
with default features disabled, to use interest notification types without
enabling host-net. Cargo.lock changed only that dependency edge; no crate version
was changed. No Wasmer import moved outside its driver.

New driver-local unit definitions cover exact lookup, isolation, bound connect,
wildcard/ephemeral bind, rebind, source lifetime, buffer preservation, fence/revoke
and backlog/port/connection bounds. They were not compiled by these targets and
have not run. The new instrumentation definition compiled but was not executed:
two private Python services, two exact self listeners, no implicit host ports,
no cross-service leakage, late completion receipt, and a third explicitly
published service exchanging bytes after a 31-second interval, then reporting
bounded normal drain. Its phone/runtime behavior is unproven.

No new test execution, Clippy/lint, device/network measurement, visual acceptance,
intermediate push, old CI polling or new CI run occurred. Historical combined
shared-process failures are preserved. Named stack grants, cross-worker routing,
internet/LAN permissions, actual network/long-stream/stop/IPv6/quota acceptance and
the remaining full Android plan stay open. See [private networking](../design/private-network.md).

## Compose planning and durable stack definitions

Source revision `506b89c7a0c84de8eca20778759afd259e29d586`, local feature branch,
author and committer `gatgserv`. The captured source passed local compilation:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Final host result: success in 1.84 seconds, including the new neutral stack
regression sources. Final Android result: `BUILD SUCCESSFUL in 19s`, including
optimized arm64 native compilation (16.63 seconds), matching generated bindings
and the new StackDefinitionTest instrumentation sources. Main Kotlin reused its
successful compilation from the preceding 24-second integration invocation.
`cargo fmt --all` prepared formatting and `git diff --check` passed before commit.
An earlier host integration error identified the missing direct serde dependency
for the bounded FFI encoder; adding the existing workspace serde dependency
resolved it. No quality gate was relaxed.

The code adds bounded YAML/JSON subset parsing, explicit command/env/port/volume
normalization, installed digest/entrypoint selection, dependency validation,
schema-3 stack membership and declared dataset references. Creation starts no
guests. Definition/member/event publication is atomic in source; the creation
event sequence distinguishes removal/recreation incarnations. Existing and
prepared data are preserved, and small membership/list APIs avoid private env
copies. Managed individual startup/retry is gated until the actual stack
controller/routing exists; this layer is not a functioning web/DB stack.

The pinned new packages are saphyr-parser 0.3.4, shell-words 1.1.1 and arraydeque
0.5.1. Their primary package metadata/source and license texts were inspected.
Both saphyr upstream copyright sets' MIT notices, shell-words MIT and arraydeque
MIT notices were copied into third_party and generated Android license assets.
No other crate version was changed; FFI now directly lists the existing serde
workspace dependency. This is not a completed distribution notice audit.

New host definitions cover parsing/normalization, unsupported authority,
duplicates/aliases/tags/multiple documents/cycles, nesting, no host interpolation,
SQL-trigger publication failure, retained prepared bytes, declared unused volume
references, external data, entrypoints/ranges, membership reopen and stale
recreation versions. They compiled but did not run. StackDefinitionTest compiled
but did not execute its public Python metadata/native definition/volume flow;
it does not execute guests or prove stack networking/dependency startup.

No new tests, device execution, migration drill, runtime stack communication,
visual acceptance, Clippy/lint, intermediate push, old CI polling or CI run
occurred. Issue #4 tracks the remaining scoped cross-worker transport, peer
resolution, dependency/capacity/start/stop/retry/rollback controller and GUI.
All full-plan and historical shared-process failure limitations remain open.
See [stack contracts and remaining work](../design/stacks.md).

## Scoped worker transport and common stack lifecycle

Source revision `574ab56f35c5d29f6804153a2ed392a2352e3dfc`, local feature branch,
author and committer `gatgserv`. Final integration compilation:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Host: success in 1.64 seconds, including neutral grant/retry regression sources.
Android: BUILD SUCCESSFUL in 18s, optimized arm64 native build 15.87 seconds,
matching generated bindings and main Kotlin compilation; instrumentation source
compilation was up to date from the preceding successful 29-second integration.
`cargo fmt --all` and `git diff --check` completed before the source commit.
No test execution is implied by compilation.

New source implements membership-derived per-generation directed grants, static
peer names/virtual IPv4 addresses, private Unix transport with HMAC-SHA256
handshake, positive-connect ACK/Opening state, exact guest ingress, bounded
connections and selective revocation. Unchanged edge leases survive other peer
changes. Endpoint cleanup after forced worker death is deferred to the next
exclusive owner recovery; inert socket files can remain during that owner run.
No ambient host egress, host DNS, internet/LAN, IPv6 mesh or remote endpoint was
added. Keys stay in private host IPC, outside specs/events/logs/backup/debug output.

The common controller reserves capacity before bind, starts dependencies in
order, updates live topology, rolls back only new reservations/workers, stops
members in awaited reverse order and retries dead managed members using a new
identity and current volume specs. Explicit stop of an already running dependency
during partial startup still reaches that worker. Group failure does not replace
a verified guest result. READY means service_started, not application health.
Current membership is distinct from immutable creation-plan identity history.

New neutral test definitions cover HMAC direction/header denial, grant reuse and
rotation, membership forgery, debug redaction and managed retry/version history.
They compiled, but did not run. Driver cfg(test) definitions cover actual Unix I/O
ACK/authentication/exact delivery/source identity/stale revision/revocation; they
were not compiled by these production targets and have not executed.
SupervisedStackTest instrumentation compiled but did not execute its two Python
workers, peer-name TCP exchange, named-volume counter persistence, independent
database-style worker stop/restart, unchanged client generation and group stop.
This is a counter fixture, not a curated database/web application recipe.

No device tests, independent transport/controller suite, Clippy/lint, visual
acceptance, push, old CI polling or new CI occurred. Cross-stack/stale grant denial,
third-peer live streams, capacity and startup rollback/stop/retry/death races,
whole-group hot descendants and the remaining full Android plan stay pending.
Existing historical shared-process failure evidence remains unchanged.

## Native group management and reviewed stack import

Source revision `9e9118b058985282a857d0dc2ffc03e63ee8c701`, local feature branch,
author and committer `gatgserv`. The conversation schemes were shown before GUI
implementation. This revision adds native My Apps Apps/Groups navigation, paged
metadata summaries, actual current membership, group/member details and lifecycle
controls, and preserved-data definition removal confirmation. Managed members are
excluded from standalone removal using actual membership rather than UI labels.

The advanced import draft remains in ViewModel memory across rotation, not saved
state or disk; process death intentionally discards it. The document editor has
no saveable state registry. SAF reads are limited to 128 KiB plus one admission
byte and reject malformed UTF-8 instead of replacing it. Leaving the importer
cancels pending reads/preview and closes provider resources; a committed creation
is not reversed when UI closes. Edits invalidate previews and late results cannot
authorize a changed draft. Creation submits the exact reviewed normalized plan,
including installed digests, to a bounded native API; validation rejects artifact
replacement before dataset preparation instead of silently replanning. Creation
still starts no guests. Missing packages fail explicitly and require installation.

Group start/stop use separate operation keys, allowing cancellation during startup.
Known explicit startup cancellation is a notice. Other partial failures trigger
state refresh; serialized UI refreshes prevent older refresh operations from
replacing newer results. Controls distinguish active operations, native member
states and unavailable metadata. Small summary/member APIs exclude private specs.
There are 46 matching English/Romanian group strings; XML parsing, duplicate-name
and localization-key/Unicode checks passed. These static checks are not visual QA.

Compilation commands:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Final neutral host compilation passed in 1.20 seconds, including added stack
summary/privacy and changed-installed-artifact regression sources. Final Android
compilation passed in 4 seconds, including main and instrumentation Kotlin. It
reused the successful preceding 13-second native/bindings integration (optimized
arm64 native 5.79 seconds); the earlier 26-second invocation first integrated the
new group screens and importer sources. `cargo fmt --all` prepared Rust formatting
and `git diff --check` passed before the source commit.

StackImportTest compiled but did not execute its strict UTF-8/byte-boundary/BOM,
late preview, changed draft, pinned input and pending-create editing regressions.
It uses a fake import backend for controller behavior, not live metadata/runtime
or Android document-provider evidence. The new neutral artifact replacement test
also compiled without execution. No device/provider/Compose visual/TalkBack/large
text/rotation acceptance, test execution, Clippy/lint, intermediate push, old CI
polling or new CI run occurred. All previous failure evidence remains intact.

Remaining group scope includes missing-package download UX, definition editing/
reconciliation and actual storage/transport/controller/phone acceptance. Generic
TCP publications do not become HTTP browser targets automatically; the existing
web flow still requires explicit recipe metadata. A future advanced web/port
selection requires its own pre-implementation scheme and port-reuse acceptance.
The remaining full Android implementation scope stays open.

## Controlled package installation and CAS write budgets

Source revision `a6c5a0265590ae685936a9a83f4308be976a17bd`, local feature branch,
author and committer `gatgserv`. Tracked in issue #5, created after inspecting
existing issue scopes. This source adds single-operation cancellation/publication
claims, cancellable reqwest async request/body waits behind the registry facade,
bounded OS resolver admission, shared CAS writer admission and caller-supplied
new-byte/optional total-CAS budgets. Existing supervisor pull/catalog calls now
use controlled native APIs; cancellation requires the matching operation generation,
and owner close waits for native completion. No cancel/settings GUI was added.

Cancellation accepted before publication prevents partial graph metadata. Once
publication wins, cancellation returns false and the actual transaction outcome
remains authoritative. A second use of a token cannot finalize another owner's
state. Verified CAS blobs survive failure/cancel for retry/leased GC, while owned
staging is removed. Admission precedes blob requests and includes the temporary
peak for corrupt replacement, unknown regular files and staged bytes. Existing
runtime reads do not acquire the separate writer lock. Android uses current free
space minus the existing 16 MiB reserve for new-byte admission, with no invented
persistent total-cache default or automatic eviction. Logical CAS admission does
not reserve filesystem space or cap the whole app/process/compiled cache/volumes.

The HTTP runtime has two async/two blocking workers and four queued/running OS
resolver slots; cancellation cannot forcibly kill getaddrinfo. Residual resolver
jobs retain their slots but no engine/store ownership. TLS, HTTPS/redirect policy,
request timeouts, metadata/body bounds and temporary-URL redaction remain active.
See docs/design/package-installation.md for the exact scope and limitations.

Compilation commands:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Final host compilation passed in 1.79 seconds, including the new neutral and
transport-unit test definitions. Final Android compilation passed in 19 seconds,
optimized arm64 native 16.86 seconds, matching bindings and already compiled main/
instrumentation Kotlin from the preceding successful 23-second integration.
The prior 22-second integration caught Kotlin ConcurrentHashMap containment's
containsValue semantics; using explicit containsKey resolved the compile error.
An existing TerminalView KeyEvent.characters deprecation warning appeared in the
successful Kotlin compilation; no gate was disabled. cargo fmt --all and
`git diff --check` completed before the source commit.

Tokio was already pinned in Cargo.lock and used by the runtime; the neutral engine
now lists its existing version directly for async transport/cancellation. Only
that dependency edge changed, with no crate versions added/updated.

New source definitions cover pre-cancel, final verification cancellation, commit
refusal/token reuse/claim ownership and concurrent cancel-publication arbitration,
zero budget/cache reuse, exact staging peak/unknown-file preservation, competing
writers/runtime reads and staging-read cleanup. The transport unit defines an
actual hermetic TLS-handshake stall/cancel, with no certificate-policy bypass and
no workstation proxy dependency. All compiled without execution.
ControlledInstallTest compiled but did not run its fresh isolated-cache public
Python pull/body-progress cancel/no partial metadata/staging cleanup scenario.
It never removes shared installed applications or executes guests.

No tests, physical download/lifecycle/cancel/DNS/quota measurement, visual/provider/
background acceptance, Clippy/lint, intermediate push, old CI polling or new CI
run occurred. Historical failure evidence remains intact. The cancellation GUI
proposal is documented in docs/design/package-install-ui.md; the conversation
scheme is prepared before Compose changes. Its fragment/DOM and node syntax checks
passed, but browser rendering was not inspected. A validation script initially
mistook the legal header element for a head document tag; exact-tag validation
corrected that false positive, without changing the visualization.

GUI generation/rebind/progress/late-cancel/commit behavior, a persistent storage
policy, manifest/index/catalog trust and local package import remain open, along
with all remaining full-plan acceptance. This compilation is not runtime evidence.

## Per-operation package GUI and owner-side application preparation

Source revision `ec96910c2f2d2f24f68f6e3ebbc0f9cbe993ef91`, local feature branch,
author and committer `gatgserv`. Implemented after the conversation cancellation
scheme, with no new visual or quota settings invented. Native rows now show each
operation's own generation/progress, actual completion, accepted cancellation,
uncancellable finalization, retry and dismiss. Package progress is separate from
data-transfer progress; an unrelated operation no longer clears that channel.
Small owner snapshot refreshes serialize and reject a replaced connection. A live
cancelled control remains Cancelling until the native future actually completes;
image commit/progress Installed alone does not mark a catalog app ready.

The common supervisor retains filtered progress and 16 terminal receipts, excludes
recipe/env fields and private retry closures from snapshots, admits at most 16
package operations and bounds retained catalog input to 128 KiB. Retry/dismiss
require the retained generation. Typed registered-operation failures distinguish
an actual failed outcome from admission/stale requests; error cause traversal on
the actor is bounded. Closed actor submissions now fail rather than return a
future that can never run. Retained outcomes are private to the owner lifetime,
not durable across process recreation or an Android background guarantee.

Inspection identified that catalog recipe preparation previously ran in a
ViewModel continuation after native image commit. Clearing that ViewModel could
abandon app creation. The supervisor now owns this phase through completion. A new
pure validate_service_spec native preflight checks supported runtime fields before
dataset preparation; the rewritten final spec is checked again before creating
volumes. Preparation never starts guests. Failure preserves committed images and
all prepared datasets, with a distinct image-committed/setup-failed outcome.

Compilation commands:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Neutral host compilation passed in 1.23 seconds. Final Android compilation passed
in 2 seconds, including main and instrumentation Kotlin, reusing the successful
14-second native/bindings integration (optimized arm64 native 6.01 seconds).
The new regression sources also compiled in preceding one-second incremental
invocations. No new dependencies or crate versions were added. Rust formatting
and `git diff --check` passed. XML duplicate/key-parity/actual Romanian Unicode
checks passed for 18 new package strings; this is not rendered or accessibility QA.

PackagePresentationTest compiled but did not run its pending cancellation versus
terminal receipt, new-generation/old-receipt masking, independent progress,
image-commit-versus-app-readiness and private-field/counter filtering cases.
ServicePreflightTest compiled but did not execute native validation/no-data/no-
instance checks in a uniquely owned cache fixture. PackageGenerationTest compiled
but did not execute common-owner live public registry resolution, held callback,
accepted-cancel pending state, actual completion receipt, stale generation refusal
and exact-owned cleanup. It resolves metadata only and never removes shared
images/data, executes a guest or stops unrelated services.

No test execution, physical download/application/background measurement, visual/
TalkBack/large text/rotation acceptance, Clippy/lint, intermediate push, old CI
polling or new CI run occurred. Actual independent/device/GUI acceptance, catalog
recipe compatibility, persistent storage policy/settings, manifests/index and the
remaining full Android plan stay pending. Historical failures remain recorded.

## Reviewed-plan fixture correction

Source revision `1dedd49b11f3c202a4e4ea080525ad7d3ab65ba7`, author and committer
`gatgserv`. Read-only source inspection confirmed StateDb.put_artifacts already
rejects changing an installed immutable reference's digest. The newer replacement
regression incorrectly expected that overwrite to succeed; it was compiled but
not executed in its earlier report.

The corrected definition now asserts overwrite rejection and preservation of the
original digest, then explicitly removes the unreferenced artifact through its
guarded metadata API and reinstalls different content. It still requires the
old reviewed plan to fail before dataset/instance preparation. This strengthens
the immutable-version coverage and exercises a permitted cache change; production
immutability behavior and quality gates are unchanged.

`cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets` passed
in 0.74 seconds, including the corrected regression source. cargo fmt --all and
git diff --check passed. No test execution or CI occurred, so this is not a passing
assertion/runtime result. The package UI/runtime source remains ec96910 above.

## Local WebC and Wasm import backend

Source revision `d684d39a35bc67e75e1b4aa9888ddaf523480c7d`, author and committer
`gatgserv`. Tracked by [issue #6](https://github.com/gatgserv/wasmdroid/issues/6).
No CI run, test execution or physical-device interaction occurred for this checkpoint.

Implemented neutral private streaming/staging, exact length and digest identity,
sealed inspection preview, installed-only dependency closure/fingerprints,
review-token commit, shared CAS admission and cancellation/publication ownership.
Fresh exclusive-owner recovery removes only recognized abandoned regular staging
files. Import creates no service or dataset and starts no guest. A write error
closes the capability rather than resuming a potentially partial byte stream.

The Wasmer-only inspector accepts bounded WebC v3 and valid core wasm32 with
`_start: () -> ()`. It preflights framed spans/CBOR/atom directory before the
eager reader, verifies actual atom hashes and validates core modules without JIT
or execution. Both artifact kinds use the common offline package loader and
ServiceExecution through a raw-Wasm BinaryPackage adapter. Existing controller,
tracked tasks, private filesystem/network/stdio and observed-worker-death rules
are retained. This is source inspection and compilation, not successful runtime
or selective process-group-stop evidence.

Final committed-source compilation commands:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
cargo check --locked -p wasmdroid-wasmer-local --target aarch64-linux-android --release --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Results: neutral Rust compilation passed in 0.48 seconds; driver arm64
all-targets compilation passed in 3.88 seconds; Android integration passed in
15 seconds, including optimized native rebuild in 12.95 seconds and matching
UniFFI generation. Main/instrumentation Kotlin were up to date from the preceding
successful 19-second integration with the same binding/test sources. Six neutral,
four driver and two Android import regression definitions compile but did not
execute. They cover transaction review/budgets/ownership/recovery, actual raw
Wasm/WebC format and shared-loader execution definitions, malformed inputs,
wrong atom hashes, metadata limits and typed cancellation. The driver execution
definition uses a finite fixture; it does not certify Android worker death.

An initial attempt to check the Cranelift driver on the Windows host was rejected
by upstream's explicit unsupported-platform compile_error. Validation used the
actual supported Android target instead. An initial driver command-name type
error and a neutral test endpoint-argument error were corrected before the tested
commit; no check or quality gate was weakened. Three already-locked crates become
direct driver dependencies (shared-buffer, ciborium, wasmparser); no crate version
changed. Rust formatting and git diff --check passed.

The import screen proposal is presented before product UI implementation. The
conversation fragment local-package-import.html was read back and its JavaScript
syntax checked; it was not browser-rendered or visually accepted. Native SAF,
common-owner operation UI/lifetime handling, dependency navigation and application
creation UX remain pending. The current Compose subset still requires installed
public registry WebC images; local stack references need separate integration.
Other documented unsupported formats, explicit budgets versus whole-process
limits, real provider/rotation/accessibility, actual guest/HTTP/volume execution,
independent regressions and integrated acceptance remain pending. PR #2 stays
draft on its previously published revision; no acceptance boxes are closed.

## Native local package import workflow

Source revision `79a3563710fe95a55f42bf196a5605fe7b74fe00`, author and committer
`gatgserv`. The preceding conversation displayed local-package-import.html before
these product UI changes. At the start of this batch origin/PR #2/issue #6 were
refreshed; PR #2 stayed open/draft on published 23455dc. Issue #6 was updated
before implementation. No CI polling or intermediate push occurred.

Advanced Store now selects a content document through SAF, streams it on bounded
IO workers and opens the native preview/install flow. The common owner retains
input/control/capability and exact operation generation. Closing a dialog hides
it; details can be reopened. Native preview requires explicit commit. Missing
named dependencies have Store searches and a fresh-review action; waiting for
review does not block explicit dependency installation. Actual installed receipt
opens the existing application configuration with a selected verified command.
Import and application creation start no guest. EN/RO strings distinguish local
unverified source, read/inspect/review/install, cancellation and outcomes.

Provider cancellation/close runs away from the actor. Non-cooperative blocked
input stays pending/cancelling until the IO operation and native cleanup finish.
The owner bounds active operations/outcomes and filters URIs/provider messages
from snapshots and receipts. A pre-publication commit failure clears the visible
preview before another review. Terminal inputs require reselection. Generation
checks reject stale actions. This adds no Android background-service resurrection
or provider preemption guarantee. Current stack-reference restrictions remain.

Inspection also found implicit native capability drop did not explicitly order
staging/lease cleanup before releasing the engine ownership field. LocalImport
now uses its existing ordered release method on Drop. Failure cleanup drops the
capability, preserving Failed, instead of calling abort and turning a provider/
format failure into Cancelled. Cancellation remains tied to the native control
and actual cleanup. Dependency range bytes are bounded consistently with the UI.

Commands on the committed source:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Neutral Rust compilation passed in 2.00 seconds. Android integration passed in
22 seconds, including optimized native rebuild in 16.97 seconds, matching UniFFI
bindings, product Kotlin and instrumentation Kotlin. One new neutral drop/lifetime
definition and five Android definitions compile but did not execute. The latter
exercise actual native/common-owner import in unique cache roots, held input and
close/cancel completion, reopened preview/explicit commit/create-without-start,
provider failure/privacy, and pure presentation/command/dependency/counter
contracts. Injected streams are not real SAF/provider/rotation acceptance and do
not execute a guest or prove worker death. Existing format/runtime definitions
remain unexecuted in this phase.

Rust formatting, git diff --check and XML/key/placeholder/Romanian-Unicode checks
passed for 20 new string pairs. This is not rendered, TalkBack, large-text or
rotation evidence. No new dependency versions or generated build outputs were
committed. No tests, device execution, Clippy/lint, CI/push/restart or new visual
rendering occurred. Real provider and GUI acceptance, independent suites, actual
imported guest/dependency/HTTP/volume/descendant/other-worker acceptance, remaining
Android features and final CI remain pending. No acceptance boxes are closed.

## Versioned application configuration backend and form scheme

Source revision `23f0068b055ce15ddb5c9cd2034d7967cf7a8318`, author and committer
`gatgserv`. At batch start the current branch, origin, open draft PR #2 and
relevant issues/evidence were refreshed; issue #3 was updated before changes.
No remote CI was polled. The previous turn added native local package import;
this batch continues the approved advanced configuration/lifecycle requirement.

Implemented standalone next-run editing for Created/Exited/Dead records with
exact revisions, immutable artifact/endpoint/kind/driver identity, installed
command validation and guarded existing-volume selection. Spec, references,
revision and private-value-free event update atomically. No-op does not bump
revision; stale/active/managed/invalid/missing/pending/failed updates preserve the
previous configuration. Datasets and prior exit/output history are preserved;
no volume is deleted and no guest/listener is created or started by save.

The common actor fences actual worker and data ownership, reserves the instance
and dispatches native work to IO. The native facade retains ownership while
releasing its session mutex before separate SQLite connection admission. Thus a
DB wait is not deliberately performed on the controller actor. Existing runtime
preflight still rejects unsupported permissions/capabilities before mutation.
Malformed JSON configuration errors do not echo private input.

A private typed form codec preserves exact arguments/Unicode/env, validates row
identity/numbers/paths and retains unedited root/label/resource fields for native
validation. It refuses lossy/coercive/unsupported nested conversions and emits
field-only errors/redacted debug strings. This batch changes no product Compose
screen. The application-configuration.html scheme is presented before the new
form implementation; the existing guarded JSON creation dialog remains in use.

Final commands on the committed source:

```powershell
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Neutral compilation passed in 1.11 seconds. Android integration passed in
10 seconds, with optimized arm64 native rebuild in 6.48 seconds, matching UniFFI
generation and product/instrumentation Kotlin. The preceding full integration
also passed in 26 seconds (native 21.33 seconds) before the final actor/mutex/DB-
admission improvement. No dependency versions or generated outputs were committed.

Six new neutral regression definitions and six Android definitions compile but
did not execute. Neutral coverage defines retained actual dataset bytes/reopen/
history/private events, stale/no-op/active/identity/member guards, missing/pending
volumes and injected reference-write rollback. Android definitions cover exact
private draft round trips/errors/conversion and actual native preflight/metadata
plus common-owner data fencing and a deliberately locked SQLite admission case.
These are not actual guest/worker/GUI/provider or independent acceptance results.

Rust formatting and git diff --check passed. The screen fragment was read back
and its JavaScript syntax checked; it was not browser-rendered or visually
accepted. It persists only presentation choices, never typed form values. No
tests, device interaction, Clippy/lint, intermediate push, CI poll/run/restart,
merge or deployment occurred. Native form implementation, rendered/rotation/
accessibility/next-run guest behavior, independent/integrated acceptance and the
remaining Android plan remain pending. No acceptance boxes are closed.

## English-only application language

Owner request: the complete application must be in English. Source revision
`1cd4f8e9363ee3fb76b645ed4d01c8ce468807d6`, author and committer `gatgserv`.
The working agreement, README and active design/backlog contracts now record
English-only UI/notifications/accessibility/formatting and future screen proposals.
Historical bilingual evidence and Unicode/user-content fixtures retain their
tested-revision meaning.

Removed Romanian app resources. The manifest advertises only en and the existing
AGP 8.13.1 build uses androidResources.localeFilters to retain only English
dependency resources. Activity/common service resource contexts override only
locale/layout direction; explicit English formatting covers worker numbers.
No device-wide or guest locale is changed. The active configuration proposal is
translated throughout, including dynamic status and accessibility labels.

Command on the committed source:

```powershell
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
```

Passed in 1 second, 33 tasks up to date from successful 6/3-second integration of
the same Kotlin/resources. An initial deprecated resourceConfigurations setting
was replaced with localeFilters before the committed revision; no gate was
suppressed. Native libraries/bindings were unchanged and reused.

AAPT2 dump configurations on the actual linked debug product resource table
showed default, v30 and density configurations, with no language-qualified
translations. Static XML/name/source inspection found 219 English default string
keys, only en declared and no Romanian production resource/text literals. This
is resource/compilation evidence, not rendered device or TalkBack acceptance.

The English proposal was read back and JavaScript syntax checked; no browser
rendering occurred. No tests/device interaction, new CI run/poll, intermediate
push, merge or deployment occurred. New configuration form implementation and
the remaining Android goal continue; device locale/font-scale/theme/rotation/
accessibility and final integrated acceptance remain pending.

## Native English configuration form

Source revision `2554c0b9ae2178452b471bf417ffd19e03742f02`, author and committer
`gatgserv`. Implemented the native Compose form after its English conversation
scheme. Application/Execution/Data/JSON sections support private create/edit
drafts, masked environment values, installed command/volume selection, explicit
no-clobber volume creation, ports and supported next-run settings. Existing
worker/data ownership, version and unsupported-feature gates remain authoritative.
No save starts a guest. Connection epochs, lossless JSON conversion and private
fixed errors protect the editor; actual native acknowledgements own completion.

Local compilation commands:

```powershell
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
cargo fmt --all -- --check
cargo check --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets
git diff --check
```

Gradle passed on the committed source in 1 second (33 tasks, 2 executed).
Neutral Rust/FFI all-target compilation passed in 1.45 seconds; formatting and
diff checks passed. New deprecated Material tab usage was replaced with the
current PrimaryScrollableTabRow before this revision; no gate was suppressed.
Defined, compiled but did not execute eight controller regressions, one raw-JSON
budget regression and one actual-native no-clobber volume regression. These
cover source/revision/disconnection/in-flight guards, private draft preservation,
lossy conversion, actual acknowledgement versus refresh failure and retained data.

Static XML/source audit found 284 unique English default string keys, only en
declared, no translated string files and no Romanian production literals.
AAPT2 dump configurations on the linked debug product at
`android/app/build/intermediates/linked_resources_binary_format/debug/processDebugResources/linked-resources-binary-format-debug.ap_`
returned default/v30/mdpi/hdpi/xhdpi only. The first lookup used an obsolete
intermediate directory; inspecting the actual AGP output resolved it. No foreign
language configuration is packaged in that table.

No tests/device interaction, rendered/rotation/TalkBack acceptance, CI poll/run,
push, merge or deployment occurred. Exact next-run guest arguments/environment,
mounts/ports/TTY, independent regression execution and integrated device
acceptance remain pending with the rest of the Android plan. Issue #3 remains
open. The full implementation goal remains active.
