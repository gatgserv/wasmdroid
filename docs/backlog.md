# Delivery backlog

Combined-app notification consistency is now implemented locally: a separate
logical-app/phase/background summary deduplicates actual native group members.
Worker/pending reservation counts and actual idle-retirement gates are preserved.
Pure JVM checks and Android compilation are separate from the pending actual
notification/FGS/device acceptance. See [notification summary](design/notifications.md).

The owner selected Apache-2.0 and the public repository `gatgserv/wasmdroid`.
The complete application and future screen proposals use English only.
`origin` is configured. The repository was verified empty (no branches, issues,
PRs or CI runs). GitHub CLI authentication as `gatgserv` and repository write
permissions are now verified in the executing context.
The baseline is on `main` through the explicitly approved initialization; new work
is in [PR #2](https://github.com/gatgserv/wasmdroid/pull/2). The remaining Stage 0
acceptance is tracked by [issue #1](https://github.com/gatgserv/wasmdroid/issues/1).
See `implementation-status.md` for the full scope.

## S0-1 — Engine foundation and native bridge (complete)

Scope: neutral contracts; content verification; SQLite migration; Compose →
generated UniFFI → Rust; finite WASI fd_write probe. Prerequisite: none.
Risks: backend support differs by host; standalone Rust MSI can shadow rustup.
Acceptance: Rust checks; Android APK build; targeted native bridge/probe tests;
record exact revision, ABI, commands and limitations in the verification report.
Evidence: [2026-11-16 foundation verification](verification/2026-11-16-stage0-foundation.md),
code revision `55641f8d81d9085322df4653e0a9e09b355e603b`: 15 Rust regressions,
3 actual Android x86_64 integration tests, dual-ABI build, and lint (0 errors,
9 warnings). This does not complete S0-2 or the full Stage 0 exit criterion.

## S0-2 — Registry → verified WebC → Android execution (in progress)

Depends on S0-1. Inspect the current official registry GraphQL schema; resolve
immutable public package versions with hashes; bounded download into the content
store; execute Python and a simple web server from actual WebC commands, with
per-instance filesystem/network isolation. Do not expose unrestricted host
networking as a production shortcut. Runtime import boundary stays enforced.

Acceptance: on Android **arm64**, download a public package from the registry,
verify hash, execute `python/python`, start the selected HTTP server, receive a
real response through `127.0.0.1` from the device browser/test client, and stop it
cleanly. Record device/OS/ABI/package version/digest and exact build revision.
Implemented: live GraphQL search/resolve, WebC v3 digest/size validation, bounded
transitive graph installation, offline dependency loading, and an actual Python
3.13.20 execution/sandbox test on Android x86_64. The first output-capture attempt
timed out; deterministic draining fixed it and the unchanged guest checks passed.
A physical Android 15/API 35 arm64 phone passed the live Python suite using the
optimized Rust runtime (55.882 seconds for the whole test, including registry
work) and all three bridge regressions. The physical arm64 HTTP harness now returns
real responses, isolates simultaneous instances, and confirms forced-stop cleanup.
Cranelift cold/warm/process-memory measurements and clean release sizes are now
recorded; Singlepass only supports the tested core workload and rejects Python.
LLVM build/license prerequisites and further compatibility checks remain visible;
S0-2 is still in progress.
Evidence: [registry/Python verification](verification/2026-11-16-registry-python.md)
and [loopback HTTP verification](verification/2026-11-16-loopback-http.md).

Current measurements: [physical runtime report](verification/2026-11-16-runtime-measurements.md).
The default Cranelift compiler is MIT; optional Singlepass/LLVM 7.4.2 use separate
BUSL terms and require an owner/expert production licensing decision. No permissive
fallback or distribution approval is implied by a development experiment.

Compare supported compiler backends and measure release APK bytes, first-start
compile milliseconds, warm-start milliseconds and peak memory bytes. Measurements
must distinguish emulator from physical phone and debug from release builds.

## S0-3 — Curated web package inventory and Play policy validation (pending)

Depends on S0-2. Inventory web UI packages; accept recipes only with Android
evidence and reviewed licenses. Wasmer Registry is the v1 Store source.
The owner selected report preview followed by the fixed GitHub issue form; local
implementation is complete, with actual browser/UI acceptance pending. See
[diagnostic reports](design/diagnostic-reports.md). Internal Play upload requires owner account, application
ID decision, signing setup, privacy/distribution review and explicit authorization.
The current `dev.wasmdroid.app` ID is provisional. No policy approval is implied.

## MVP engine prerequisites (in progress)

Tracked in [issue #3](https://github.com/gatgserv/wasmdroid/issues/3).
Implemented prerequisites: durable lifecycle metadata/events and stale-revision protection;
exclusive engine ownership/recovery; named volume data operations, read-only
leases, traversal/symlink confinement and journaled removal recovery. General
RuntimeDriver is implemented by the worker-resident ServiceDriver and used by
the native service facade. Its private callback terminates only its own worker;
the coordinator still awaits Binder death. Process RSS/CPU and localized resource
summaries are local source implementations, with acceptance pending.
Actual guest mounts, a common
Android foreground coordinator, per-service workers and product lifecycle screens
are now local implementations awaiting integrated acceptance. Experimental host interruption passes focused checks
but failed the combined phone suite (HTTP cleanup and CPU-loop exit). It is not
accepted as reliable. The owner clarified a common supervisor with one service
controller/execution group per package; stopping one service must stop all its
descendants and preserve other services. See the service supervision design.
Storage metadata is not evidence of guest execution.

See [the current storage and cancellation evidence](verification/2026-11-16-lifecycle-storage.md)
and [the service supervision design](design/service-supervision.md).

## Android v1 internal APK

Use Wasmer Registry as the sole active Store source. On the physical Android
phone, search in normal mode, download a public version, create an application
from the installed package, and start and stop the resulting service. Check
compatibility and registry failure messaging with the existing workflow.
After verification, both the download completion card and the saved release
in search results should open service configuration for that exact release.

Verify service-process RAM/CPU observation. Check that a configured positive
timeout stops the service and that an empty timeout lets it continue beyond
the same observation window. Verify embedded WebView profile separation between
apps on a provider that supports it. Hard resource caps and complete WebView
traffic control are deferred. Record the tested revision, APK, ABI, commands,
results and skipped checks in `docs/verification`.

The [Android interface acceptance](verification/2026-11-18-android-ui-acceptance.md)
records physical Store search, standalone service creation, RAM/CPU, both timeout
settings, WebView persistence, and SAF backup/restore. The same source passed
Android 9 registry transfer interruption and retry, plus equal-origin WebView
profile isolation and selected-profile erasure on Android 16 with 16 KiB pages.

The [integrated suite](verification/2026-11-18-integrated-android-suite.md)
passed all 270 distinct Android instrumentation cases across the applicable
phone and emulator targets, all 292 host engine/FFI checks, and all 82 native
driver tests on Sony after correcting the synthetic WebC signature fixture.

The [Android lint cleanup](verification/2026-11-18-android-lint-cleanup.md)
records the API 36 checkpoint. The separate
[API 37 migration](verification/2026-11-18-android-17-api-37.md) passed
Android 17 native runtime, independent guest Internet and LAN access, and
Wi-Fi sharing with the Android local network permission. The same APK
passed compatibility checks on Android 9.

The [Android dependency refresh](verification/2026-11-18-android-dependency-refresh.md)
updates the six versions flagged during the migration and reports an
issue-free Android lint run on the refreshed APK.

The [AGP and service form verification](verification/2026-11-18-agp-and-service-form.md)
records AGP 9.5.1, the removal of Gradle 10 deprecation diagnostics, and
Android 9 compatibility. It also records the aligned Create service
controls, live measurements for a running HTTP service, and the Sony
comparison of the failing `baran` package with a working service made from
`wasmer/static-web-server@2.44.0` in the same registry.

Store marks the exact Python and Static Web Server releases with recorded
successful Android runs. Those releases lead the loaded Wasmer search results,
including direct Wasmer lookups for exact matches omitted by its text index.
The **Newest release** order uses the version publication date within each
tested or untested group. It retains the provider's pagination.
See [Store label verification](verification/2026-11-18-store-android-tested.md).
Store hides known WebC v1 and v2 releases by default and can show them with the
exact format and a disabled **Download** action. The
[WordPress compatibility verification](verification/2026-11-18-registry-webc-filter.md)
records the registry response and separates container format from the WCGI runner.

## Remaining MVP

The owner approved Internet and guest outbound-LAN access with separate
permissions, both disabled by default. The standalone first-start/restart path
now has actual selected-Network callbacks, native pre-spawn proof, destination
policy and reviewed normal controls. Old no-capability startup stays Unsupported;
running handover and managed grants have local implementations. On the Sony,
Internet-only public DNS and TCP and LAN-only private DNS and TCP passed.
The opposite permission was denied in each mode. The same worker recovered LAN
traffic after Wi-Fi loss without reproducing the ART abort from
[issue #7](https://github.com/gatgserv/wasmdroid/issues/7). The LAN-only Python
socket now keeps its timeout through the full HTTP body read. Repeated writable
polls, partial writes, and half-close passed the final Android networking suite
for [issue #8](https://github.com/gatgserv/wasmdroid/issues/8).
See [DNS and TCP acceptance](verification/2026-11-18-android-egress-jni-arm64.md),
[timed LAN TCP acceptance](verification/2026-11-18-android-timed-lan-tcp-arm64.md),
and the [outbound mechanism](design/guest-egress-mechanism.md). Inbound Share and
NetworkMode::Lan are separate contracts. Package metadata does not grant guest
networking.

Schema3 media and signed catalog work is historical for this release. See
[catalog media](design/catalog-media.md) for its contract.

Advanced configuration now has a local version-checked next-run editing API,
atomic spec/volume-reference/event updates, common-owner worker/data fencing and
a private typed form codec and native Compose create/edit form under issue #3,
implemented after its English conversation scheme. Data, history and unsupported-feature
gates are preserved; save starts no guest. See
[application configuration](design/application-configuration.md). Native/independent/
rendered/next-run acceptance remains pending with the integrated checks.

Complete and accept the local lifecycle/supervisor, Foreground Service, guest
mounts, registry installation/removal, Store/My Apps/details/settings, opt-in
advanced mode and data/SAF flows. The compiled disk cache and separate embedded
browser profiles have local implementations with acceptance pending. Private
per-service TCP/self-connect/internal ports, bounded host forwarding,
generation-scoped stack routes and explicit guest Internet/LAN grants have local
implementations. Integrated device acceptance remains pending. Standalone
web applications now attempt Wi-Fi ingress by default when a single eligible
selection is available and Android allows local access. The owning worker
can relay the published web port through both a private IPv4 and an on-link
IPv6 listener. Stop sharing and loss of the selected capture still revoke the
entire generation's share. See
[LAN ingress](design/lan-ingress.md) and the private networking design.
Failed standalone and managed service cards expose the stored exit information
and bounded guest stderr preview, with direct access to private output.
The [current verification](verification/2026-11-18-store-release-dual-stack.md)
records Sony HTTP via IPv4 and IPv6, the Android 17 permission flow and the
Android 9 release-date compatibility regression.
Live rotated logs, the native terminal and resource displays are locally
implemented with integrated acceptance pending. Full live
E2E runs at acceptance completion; targeted story checks stay separate.
Do not start Docker/iOS implementation in this phase.

## Stage 2 stacks (in progress)

Tracked in [issue #4](https://github.com/gatgserv/wasmdroid/issues/4), depending on
the general lifecycle/private networking foundation in #3. Bounded Compose
YAML/JSON planning, installed digest/entrypoint selection, scoped/external named
volumes and atomic definition/member persistence now have local implementations.
Creation starts no guests. The common controller now supplies membership-derived
private grants, peer names, dependency/capacity/start/reverse-stop/rollback and
versioned managed retry. Datasets survive removal and failed preparation; no
parsed or compiled definition is accepted as a working web/DB stack.

Native group list/import/preview/details, lifecycle controls, stopped-definition
editing and explicit missing-package download/review are implemented locally
after the conversation schemes. Creation pins the reviewed normalized plan and
rejects changed installed artifacts. Remaining: physical GUI/SAF/accessibility and
independent metadata/transport/controller/actual two-service arm64 acceptance.
Include wrong/stale grants, third-peer flow preservation, capacity and partial
startup rollback/stop/retry races. No acceptance boxes
are closed from source compilation. See [the stack contract](design/stacks.md).


## Controlled package installation (in progress)

Tracked in [issue #5](https://github.com/gatgserv/wasmdroid/issues/5). Local source
now adds single-operation cancellation/publication ownership, cancellable HTTPS
request/body waits, bounded resolver admission, shared CAS writer admission and
explicit new-byte/optional total-CAS budgets. Existing Android pull/catalog calls
use controlled APIs and retain generation-scoped handles until actual completion.
No total-cache product default or automatic eviction is invented. Native progress/
cancel/retry/dismiss rows are now locally implemented after the scheme, with
supervisor-owned progress and receipts. Persistent cache policy/settings and
physical/visual/independent acceptance remain pending. Earlier signing and
catalog update work is outside the v1 Store path. Local package import stays
separate.
See [installation contract and limits](design/package-installation.md).

## Local WebC/Wasm import (in progress)

Tracked in [issue #6](https://github.com/gatgserv/wasmdroid/issues/6), depending
on lifecycle #3 and installation controls #5. Local source supplies bounded
private streaming, format inspection, sealed preview, installed-only transitive
dependency pins, guarded CAS publication and the same service runtime for WebC
and raw Wasm. Import does not create/start applications. Regression definitions
cover neutral transactions, actual driver formats/loader and native bindings;
compilation alone closes no acceptance boxes. Native SAF/product flows and
common-owner retained preview/commit/cancel now follow the conversation scheme.
Provider/rotation/accessibility, actual guest/dependency/volume/HTTP execution and
selective descendant/worker stop remain pending. See
[the local import contract](design/local-packages.md).
Installed imported-digest group planning/update is now implemented locally with
dependency/metadata fences and a worker-side pin check. Actual imported-group
execution, selective stop and data acceptance still require the integrated device
suite; no runtime acceptance follows from metadata preparation or compilation.

## Restart, boot and resource policy (in progress)

Standalone no/on-failure/always restart and default-off boot opt-in now have local
generation/revision-fenced coordinator, common-owner and Android receiver/service
integration. Scheduling waits for observed worker death and durable terminal
commit; Dead uses a versioned replacement identity. Manual stop/edit/remove/data
admission cancels queued automation. Automatic-start settings now follow the
presented English scheme, with private drafts and actual group-ownership gates.
The v1 resource contract is process RAM/CPU observation and an optional
execution timeout. A blank timeout represents unlimited execution time in
WasmDroid. Hard RAM/CPU enforcement is deferred. The physical round verified
visible worker RAM/CPU, finite and unlimited timeout, Home/reopen, manual
Stop/Start, and automatic startup after a real reboot and first unlock. The
automatic failure-restart policy has separate acceptance. See
[restart and boot](design/restart-and-boot.md).
The backend adds schema-5 durable run/stop authority, same-boot bounded
cold recovery and generation-fenced worker acknowledgements. Browser sessions now
acquire actual common-owner generation/port leases and profile-data erasure uses
explicit profile-token/revision/lease/provider acknowledgement. The v1 browser
contract uses separate WebView profiles per app. The Sony retained WebView data
after reboot, and the 16 KiB AVD verified isolation and clearing on its compatible
provider. Complete WebView traffic control is deferred.
