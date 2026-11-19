# Standalone application restart and boot

This is local development source, including common-owner wiring. Compilation and
actual Android acceptance must be recorded separately before claiming readiness.
The work is tracked with the existing lifecycle issue #3. The existing advanced
create/edit form now includes automatic-start controls after its English scheme
was shown. Rendered/device/accessibility acceptance is separate from source.

## Authorization and execution

`InstanceSpec.restart_policy` keeps the existing neutral values `no`,
`on_failure`, and `always`. `start_on_boot` is an independent serde boolean with
default `false`; an old saved specification never gains boot authorization.
The common owner applies these policies. A single-use worker never restarts
itself, acquires a new slot or silently extends its generation.

An automatic restart requires both observed worker/Binder death and a committed
terminal-state/event transaction. A verified zero guest exit restarts only under
`always`; a nonzero exit, runtime error or worker death without a verified result
qualifies under `on_failure` and `always`. Owner stop, stop-all and coordinator
close suppress scheduling by that coordinator. Close alone retains durable run
authority for eligible same-boot cold-owner recovery. Wall-clock expiry records an execution-timeout error and
requests automatic termination; `on_failure`/`always` may react after death and
terminal commit. A later explicit owner Stop still wins. Forced death does not
manufacture an exit code.

`Dead` remains terminal. An opted-in restart of a Dead record uses the existing
version-checked replacement operation, preserving the configured display label,
specification and mounted datasets while assigning the actual new instance ID.
It never changes Dead back to Running. Lists must refresh to show replacement
identity. Managed stack members remain explicitly unsupported for independent
restart/boot until their group policy is defined.

## Reservations and cancellation

`ServiceRestartCoordinator` runs entirely on the common supervisor actor. A
registered generation can complete once. Its immutable ticket includes the
source/current IDs, old generation, exact terminal version, policy and state.
Before native replacement and again before worker admission the common owner
checks the current record, ticket identity, policy, membership, worker ownership,
data ownership and closing state. A replacement returns a new immutable ticket
and keeps the original reservation. Registering the newly reserved Job consumes
it. Cancel accepts either the old or replacement identity.

Delayed dispatch retains its reservation until admission completes. Foreground
active count includes jobs plus restart/boot reservations; a posted automatic
start must not expose a false idle interval. Manual start/stop, restart, edit,
data admission and removal cancel pending retries before their own side effects.
Stop-all/close cancel every pending boot and restart token. An OS timer or queued
start arriving after cancellation is inert. Failed revision, ownership, capacity
or native admission is reported and stays stopped rather than retrying a changed
operation forever.

Notification counts are hints across the actor/main boundary. An idle shutdown
requires a `ServiceLifetime` permit reserved on the actor after checking every
guest, boot/restart, data, stack, installation, catalog and tracked IO owner.
An unbound retirement permit fences later admissions. Main commits that exact
permit and uses `stopSelfResult(startId)`; a newly accepted start or binding
invalidates the permit. A bound idle UI may remove its foreground notification
but never retires/closes its cached supervisor. Android's
[Service API](https://developer.android.com/reference/android/app/Service#stopSelfResult(int))
fences accepted start IDs, and active [binding references](https://developer.android.com/develop/background-work/services)
retain the service lifetime. New binding/start reopens
admission, while actual destruction permanently fences the old owner. Pure
count re-query and per-actor-turn count coalescing do not establish this gate.

Technical anti-spin backoff is `min(60, 2^(n - 1))` seconds for consecutive short
runs, beginning at one second. A run lasting at least 60 seconds resets `n`.
This caps delay, not retries, and defines no user-selected product default.
Automatic Dead replacement carries the backoff to the new identity. Manual
start resets it.

## Durable desired-run authority and cold-owner recovery

Schema 5 adds `instance_automation`, linked to the actual instance by a cascading
foreign key. It stores desired-run, authority revision, exact generation, optional
boot epoch and run outcome. A legacy missing row is `desired_run=false`; restart
policy and a saved boot checkbox do not manufacture authority. These fields are
private engine metadata, not labels or Android preferences.

Manual Start commits a fresh desired=true/Starting generation after specification,
ownership and capacity validation and before any worker Binder launch. READY and
terminal transactions validate the exact generation and commit authority outcome
with instance state/events. A zero verified exit without an error is Succeeded;
nonzero/error is Failed; unknown worker loss is Interrupted. Manual Stop commits
desired=false before cancellation/signals. Stop All clears every desired=true row
in one transaction before its first worker signal. Failed writes leave workers
owned and report failure. An active generation remains active until real terminal
evidence; the intent write never fabricates process death.

Configuration Save, including a duplicate Save, data admission/restoration and
removal suppress prior desired authority before conflicting work. Native editing
also clears it within its own metadata transaction. Changing No to Always does
not restart an old completed run. Coordinator destruction preserves desired-run;
it is distinct from an explicit user Stop. Dead replacement copies authority and
increments its revision atomically with the new identity/data references, before
deleting the old record. A new worker still needs a fresh authorized generation.

Website data has a separate private `instance_browser_identity` mapping in
schema 5. Migration preserves each legacy profile key exactly as the old actual
ID. New applications use `browser:<durable creation-event sequence>`, a namespace
that cannot collide with a valid legacy ID. Same-spec standalone and managed
Dead retry atomically retarget the existing mapping to the new actual ID; a
new application or package replacement receives a fresh profile. There is no
caller assignment or label-derived identity and no automatic provider erasure.
The owner resolves profile state, erase and session admission through the actual
mapping. Old-view and replacement-view leases share that stable key: the old
generation must release its views before another generation can reopen/erase
the profile. This retains the original provider profile and website bytes across
authorized crash recovery without weakening generation-scoped revocation.
Website storage still follows normal origin semantics: a newly allocated host
port may create a different origin. Profile retention does not certify origin
continuity, an exclusive transport gateway or real provider acceptance.

A new `EngineSession` first acquires the existing exclusive ownership lock. Each
worker retains the inherited ownership descriptor and kills its own process when
the common owner's Binder dies. If any old owner/worker lease still exists, new
ownership fails rather than stealing a service group or modifying its authority.
Only after exclusive acquisition does recovery commit interrupted terminal
metadata, including Starting runs that died before READY. This closes the
desired-write/worker-launch and manual-stop/terminal-ACK crash windows. READY and
terminal acknowledgements are idempotent for the exact generation; a lost READY
ack cannot strand the later observed-death terminal commit under an old version.

Cold recovery requires persisted desired=true, an eligible configured policy and
the exact same boot epoch. Always accepts Succeeded/Failed/Interrupted; OnFailure
accepts only Failed/Interrupted; No never resumes. Android's public
[`Settings.Global.BOOT_COUNT`](https://developer.android.com/reference/android/provider/Settings.Global#BOOT_COUNT)
has existed since API 24. The app reads its nonnegative value without a guessed
default. Missing/denied/invalid identity blocks cold recovery with
`boot_identity_unavailable`. A different boot epoch cannot be bypassed by opening
the UI; separately selected boot opt-in can authorize a fresh boot run. Manual
and explicit boot launches may run without an available epoch, but create no
recoverable cold authority. A live private controller can still restart its exact
current generation without crossing a boot boundary.

The first UI binding establishes an actual started foreground coordinator before
requesting cold recovery. A binding alone does not create desired-run. OS denial
starts no recovery guest and records a fixed admission failure. Repeated binding
to the same owner reuses its result instead of authorizing another run. Android
Settings Force stop is distinct from in-app Stop: app code cannot persist an
intent while the OS force-stops it. Normal stopped-package delivery restrictions
still apply; post-reopen/OS reason behavior is unverified and no special force-stop
reactivation guarantee or invented API-28/29 reason is claimed.

Recovery enumerates metadata pages of 1–100 rows, at most 256 KiB each. SQL bounds
potential authority IDs before inspecting selected records; candidate frames
contain only actual ID/version/state/policy and authority, never private specs,
arguments or environment. Deterministic cursors advance across policy-ineligible
rows. The actor yields between pages and rechecks actual membership, data/worker
ownership, instance version and authority revision before scheduling. Manual
Stop/Stop All/configuration and owner close invalidate the scan/tickets.

Live jobs and pending boot/restart tickets consume the bounded worker capacity.
A full pool yields an explicit incomplete receipt with `capacity_blocked` and
`next_cursor`, alongside scheduled/skipped/scanned counts. Scheduled is not a
claimed started guest. A continuation or explicit application start must be
reviewed after capacity becomes available; no silent first-eight success or
unbounded admission/retry queue is implied.

## Actual Android boot entry

`EngineBootReceiver` handles only `BOOT_COMPLETED`, after credential-encrypted
storage is available. It immediately requests the existing `specialUse`
foreground coordinator. The receiver does not read a second database, launch an
Activity, schedule a delayed worker, react to package replacement or interpret
sticky recreation as authorization. The owner then opens the existing engine
session, plans opted-in standalone applications, and rechecks each revision and
ownership before actual admission. Dead candidates use versioned replacement.
One failed application must not prevent reporting other candidates' outcomes.
Duplicate boot requests in the same coordinator lifetime are deduplicated.

Android documents boot broadcasts as an exemption to background foreground
service startup restrictions; some service types have additional boot bans.
The app's existing `specialUse` type has no runtime prerequisite in the official
type table. This supports the selected entry path, subject to actual OS/OEM
admission and device acceptance. See [background-start restrictions](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)
and [foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types).
The `specialUse` declaration still requires separate Google Play review; source
implementation is not distribution approval. User force-stop, OS restrictions
and device policy may prevent delivery/start.

The private boot receipt records only a bounded outcome and timestamp. It is
observability, never a policy store or launch authorization. OS admission failure
is recorded as failure; a notification or a queued plan is not a started guest.

## Common-owner integration hooks

The following paths are wired into `ServiceSupervisor`:

1. The coordinator uses its actor with Handler scheduling. Its current record
   loader rejects closing/missing entries, accepts only Created/Exited/Dead, and includes
   real native membership plus `jobs`/`dataOwners` fences. Map exact JSON states
   to `State`; do not map running to exited.
2. After reserving a normal Job in `jobs`, call `started(id,generation)`; for an
   admitted automatic ticket call `started(id,generation,ticket)`. Add monotonic
   ready time to Job to calculate the completed running interval.
3. In `died`, after terminal metadata commit and actual death, remove the Job
   and call `completed` with the committed record/version and original generation
   before the final active count notification. Exclude stack-owned jobs.
4. In automatic `launch`, recheck `valid` on the actor. For Dead call the existing
   versioned `retryDeadInstance`, parse its actual replacement record, and call
   `replaced`. Recheck the resulting ticket before admission. Keep reservations
   until Job registration or `admissionFailed`; report failures in the common
   owner status instead of silently swallowing them.
5. Cancel pending tickets in every manual action above; include reservations in
   `onActiveCount` and expose `activeCount(): CompletableFuture<Int>` for the
   foreground owner. Stop-all and close also invalidate queued boot work.
6. Expose `restoreBootApplications(): CompletableFuture<String>` with bounded
   `{started,failed,skipped}` counts. `BootApplicationPlan` supplies candidates
   and exact rechecks. Boot reservations and run identity survive posted
   start/replacement steps; fresh manual work invalidates matching candidates.
   Return only when each admitted start has an actual outcome. No fake success
   is returned just because selection or dispatch succeeded.
7. Managed stack definitions reject independent restart/boot. The runtime driver
   accepts these coordinator-owned fields while keeping unsupported resource
   fields fail-closed. A bridge-only build still explicitly rejects unavailable
   execution; it never claims restart/boot execution support.

## Automatic-start configuration form

The existing Application tab shows an independent `After device restart` switch
and `When this application exits` radio choices: Never, On failure and Always.
Legacy specifications display Off/Never; opening the form grants no run intent.
The copy explains that Manual Stop prevents restart-on-exit and the device-restart
setting is separate. Android can prevent automatic startup. The unchanged CPU/
memory unsupported message remains visible; no fake enforcement slider is added.

These are typed fields in the private ViewModel draft. They survive form/JSON
conversion and a new-application connection reload. Boolean coercion, nulls,
unknown policies and aliases such as `on-failure` are rejected without echoing
input values. Original environment, labels and unrelated resource fields remain
lossless for native validation. No draft is serialized to saved state,
preferences, receipts or logs.

Save still calls only the existing create or versioned configure path. Editing
an existing application captures its exact native revision; conflict retains the
draft until explicit Reload, while Reload replaces it with current settings.
Cancel/Back discards the draft and issues no configuration mutation. A pending
save or volume creation cannot be duplicated or dismissed. Saving does not start
a guest or reauthorize desired-run; an existing application's prior authority is
cleared by the native configure contract. Start separately to enable the selected
exit policy. Boot opt-in remains a separate future boot authorization.

`ConfigurationBackend.group` reads actual native membership on inspect and again
before Save or volume creation. A grouped application is read-only with explicit
Unsupported guidance; labels cannot declare that membership. A membership change
or disconnected captured owner blocks the write and keeps the private draft.
The screen also uses current observed ownership/membership/revision to disable
Save; the native owner/SQLite guards remain authoritative.

Ten dedicated codec/controller regression definitions cover default opt-out,
all independent policy/boot combinations, strict invalid-input privacy,
configuration-only Save, conflict/reload/discard, real membership, membership and
connection races, delayed single-field events and new-application form/JSON retention. They are not executed
Android or visual evidence. Final acceptance must inspect TalkBack selection and
disabled states, large fonts, rotation/discard/reload, real native stale/busy
responses and actual subsequent manual/boot/exit-policy behavior.

## Resource enforcement remains gated

The source still rejects `memory_max_bytes` and `cpu_shares` before runtime side
effects and reports both capabilities as false. No enforced memory/CPU ceiling
is implemented by this batch. Current process RSS and CPU clocks remain
measurements only.

Before enabling either feature, define the requested scope and metric (guest
linear memory, committed bytes, resident bytes or worker/group memory), CPU
shares versus quota, failure semantics, and safe product defaults. Then provide
an authorized per-group Android enforcement backend covering every worker thread
and guest descendant, with atomic admission/readback and failure rollback. The
existing application has no implemented authority for configuring such groups.
AOSP's [cgroup abstraction](https://source.android.com/docs/core/perf/cgroups)
describes system-owned task profiles, controller ownership and optional memory
controller support; device availability/permissions have not been measured here.

A Wasm memory maximum excludes host/JVM/compiler allocations and other modules.
`RLIMIT_AS` counts virtual mappings rather than RSS; an RSS polling kill can
overshoot; niceness hints do not implement the requested CPU shares. None is
substituted for the current neutral fields. This is an unresolved prerequisite,
not completed enforcement or a reason to weaken the contract.

## Defined acceptance

Pure regressions cover death/commit conjunction, policy/owner-stop decisions,
duplicate completion, stale generation, changed revision/policy, data/worker
ownership, membership, old/new-ID cancellation, retained reservations, backoff
and failed admission. Boot regressions cover default opt-out, independence from
restart policy, live/member exclusion and admission revalidation. Neutral serde
regressions cover old specs and all policy/boot combinations. Lifetime ordering
regressions model stale zero followed by admission, retirement fencing, new
binding/start reuse, tracked IO and destruction. They are defined
for later execution; this document does not claim they ran.

The durable-recovery batch additionally defines ten native regressions covering
legacy opt-out, before/after-READY Stop crash windows, same/different/missing boot
identity, new-ID authority copy, success/failure policy, exact generation/revision,
idempotent/lost ACKs, atomic Stop All rollback, Save suppression, retained owner
leases and bounded private-free recovery pages. The coordinator definition also
rejects stale authority/desired/epoch candidates. Root executed all ten native
automation regressions and four native browser-identity regressions successfully
in the current integration batch. The Android coordinator/provider definitions
remain unexecuted; no actual crash/reboot or device acceptance is claimed.

Integrated Android acceptance must demonstrate verified success/failure exits,
actual crash/new-identity replacement, preserved volume bytes and names, stop
during backoff and replacement, full-slot admission failure, no foreground gap,
and two-service noninterference. Reboot the selected existing device only in the
explicitly authorized acceptance phase and verify actual guest HTTP/readiness,
default opt-out, duplicate broadcast handling, user force-stop/OS denial and the
receipt. Compilation, a selected plan or a mocked callback proves none of these.
