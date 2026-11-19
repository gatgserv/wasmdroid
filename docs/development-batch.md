# Active local implementation batch

This checkpoint summarizes implementation and local compilation.

Implemented at this checkpoint:

- common Android service coordinator, eight private same-UID worker processes,
  generation-aware IPC, death observation, stop escalation and orphan cleanup;
- duplicated engine-lock ownership across coordinator/worker lifetimes;
- generalized verified offline WebC service execution with args/env/cwd, explicit
  volume capabilities, TCP forwarding and unsupported-field rejection;
- actual WASIX volume filesystem adapter enforcing read-only and confinement;
- atomic running/port publication, stopped-with-unknown-exit reporting, and an
  explicit dead retry that preserves volumes under a new identity;
- three-run bounded output snapshots, registry search/pull and expected-digest
  catalog resolution before download/publication;
- localized Compose Store/My Apps/settings/details/configuration/output screens,
  opt-in advanced mode, notification permission and local-origin WebView.
- application data child screen with SAF import/export, explicit replacement,
  folder/file actions, progress/cancel and portable per-volume backup/restore;
- private atomic file staging, streaming gzip/tar backup, bounded no-clobber
  restore and atomic attachment while preserving the prior data volume;
- dependency-aware image removal and store maintenance leases protecting live
  installs from CAS garbage collection.
- authenticated persistent native-module cache bound to source/build/compiler/
  target/features, bounded eviction, explicit cache maintenance and hit counters;
- per-instance embedded WebView profiles with an explicit provider capability
  gate, denied device permissions and a same-origin cookie regression definition.
- raw live stdout/stderr with bounded disk segments, generation/sequence cursors,
  follow/since/tail queries, truncation reporting and an advanced Logs screen;
- bounded virtual stdin and WASIX terminal bridge, request acknowledgements,
  stale-generation fencing, virtual resize/interrupt/EOF control and a real
  Python TTY acceptance definition;
- bounded VT100 display state retained in the worker, immutable paged frames,
  optional final snapshots and a native Compose Canvas/IME terminal frontend.
- worker-resident RuntimeDriver used by the native service facade, trusted
  platform-only termination callbacks and generation-fenced resource queries;
- bounded process RSS/CPU measurements, nullable unavailable samples and a
  lifecycle-aware resource summary on running application cards.
- a private per-service TCP namespace with internal/ephemeral ports, exact
  self-connect, localhost resolution, bounded queues and stop revocation;
- established host forwarding without a blanket 30-second lifetime, bounded
  normal-exit draining and generation-scoped recent completion receipts.
- strict bounded Compose YAML/JSON planning with explicit installed-package
  commands/digests, dependency validation and scoped/external named volumes;
- schema-3 atomic stack/member metadata, preserved datasets and stale-incarnation
  removal protection;
- private authenticated Unix worker transport, scoped peer names and virtual IPv4
  routes, asynchronous connect acknowledgement and selective grant revocation;
- common stack capacity reservation, dependency startup, reverse stop, partial
  startup rollback and dead-member retry with preserved current datasets.
  Stack runtime acceptance remains pending;
- native paged group summaries, current membership, private in-memory SAF/editor
  draft, digest-pinned preview/create and individual/group lifecycle/detail flows,
  implemented after the conversation schemes. Visual/provider acceptance remains
  pending; creating a definition still starts no guest.

The installer now has single-operation cancellation/publication ownership,
cancellable HTTPS/body futures, bounded resolver jobs and cross-writer CAS space
admission. Android pull/catalog calls retain private generation-scoped controls.
Native per-operation progress/cancel/retry/dismiss rows were implemented after the
conversation scheme; catalog preparation now stays in the common owner. Persistent
quota settings/policy and physical/visual/independent acceptance remain pending. See
[installation scope and pending evidence](design/package-installation.md).

The proposed GUI schemes were shown before these Compose changes. The real
catalog remains empty; illustrative applications from the proposal are not
installable items. No current worker/device acceptance, release or Play policy
approval is claimed. Driver/process resource acceptance, full network permissions,
terminal/live-output/data/SAF/compiled-cache acceptance, curated recipes,
Stage 2 features and the remaining full plan still need development/acceptance.

Earlier failed shared-process interruption evidence is preserved. The new
controller requires observed worker death as the final service-group boundary;
it has not been proven on-device yet. Do not erase old failures or treat native
compilation as proof of reliable stop.

Local WebC/Wasm import now has streamed staging, sealed preview, installed-only
dependency pins and guarded native commit. Raw Wasm shares the same supervised
service runtime through a BinaryPackage adapter. Import has no instance/data
creation or auto-start side effects. Native SAF/product screens and common-owner
operation handling now follow the conversation scheme. Actual provider/visual/
rotation, independent and common-worker execution/stop acceptance remain pending.
See [the local import contract](design/local-packages.md) and issue #6.

The configuration backend now supports exact-revision standalone stopped-record
editing with atomic spec/reference/events and preserved datasets/history. The
common actor reserves the instance and dispatches DB admission to IO; native
preflight rejects unsupported features. The private typed draft now feeds the
native English Compose create/edit form after its conversation scheme, with
connection/revision guards and explicit no-clobber volume creation. Actual
next-run/visual/independent acceptance remains pending. See
[application configuration](design/application-configuration.md).

This checkpoint adds reviewed stopped-stack definition reconciliation,
protected old/restored datasets and private original-plan retention, plus actual
native/common-owner admission. The edit GUI and explicit missing-package download/
review now follow the English scheme and are implemented locally. Signed remote
catalog/index updates now have neutral/native/common-owner/
AppModel paths, bounded controlled transport, last-good snapshots and exact
selection pins; production trust configuration and accepted recipes remain open.
Standalone restart and explicit boot opt-in now have common supervision and
Android receiver/service wiring. Independent source review drives corrections
to foreground idle retirement; hard memory/CPU enforcement remains Unsupported.
These are local source/compilation milestones, not executed acceptance. See
[stacks](design/stacks.md),
[catalog updates](design/catalog-updates.md) and [restart/boot](design/restart-and-boot.md).
