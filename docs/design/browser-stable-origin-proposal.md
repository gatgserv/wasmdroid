# Stable numeric browser origin: proposal and implementation boundary

The initial 2026-11-17 investigation below was read-only. The subsequent approved
native implementation adds migration006, opaque captures and retained-listener
allocation; matching checks and physical acceptance are recorded separately.
This remains independent of the unanswered complete-WebView-firewall scope.

## Implemented contract decisions

- `state::BrowserOriginCapture` privately captures the canonical database,
  actual instance, native profile, exact specification/version and durable run
  authority before preparation. No public profile constructor or serialization.
- `PortLease::reserve_browser_origins` binds the entire actual listener set then
  atomically claims it. `validate_browser_origins` revalidates while leases are
  retained, before transferring those same listeners into the normal forwarder.
  Lifecycle publication independently rejects disagreement with known history.
- `browser_origin_assignments` is append-only selected mapping **and** history:
  host port is globally unique for local TCP and the key is private profile plus
  guest TCP port. There is no instance cascade or erasure/reclamation operation.
- Explicit unchanged host ports are supported. Changing a saved host port and
  ambiguous guest mappings are scoped Unsupported pending origin-change review;
  ordinary general `InstanceSpec`/raw `PortLease` contracts remain unchanged.
- New unpublished automatic candidates have32 bounded real bind attempts when
  history excludes an OS-free number. Saved/explicit busy binds fail immediately;
  claim races fail atomically, without silently publishing another origin.
- Migration recovers current local TCP bindings before lifecycle recovery drops
  them. Existing profiles remain `tracked_from_birth=false`; only new identities
  are tracked from birth. Missing legacy stopped/removed history is unrecoverable.
- No Android/UI/NSC change, old-data reset, complete/exclusive browser flag,
  all-protocol isolation claim or observed localStorage acceptance is included.

The remaining sections preserve the original design reasoning. The separate
current/history tables proposed there are represented by one permanent mapping
table because origin-changing operations are initially unsupported.

## Feasibility and exact current evidence

Preserving the **actual browser profile identity and numeric127.0.0.1 host port**
can preserve the same HTTP origin across same-spec worker replacement without
HTTP proxying, body rewriting, dynamic127/8 aliases or `*.localhost` resolution.
The existing cleartext policy already permits exactly127.0.0.1. It is viable for
known/new assignments if an unavailable saved port causes a clear start failure.
It is not an OS-wide port reservation while the worker is stopped, complete
browser egress isolation or an exclusive generation gateway.

- `engine/src/network.rs::PortLease::reserve` binds the actual TcpListener and
  transfers it without closing/reopening; host_port0 currently allocates afresh.
- `engine/drivers/wasmer-local/src/service_execution.rs` reserves all listeners
  before spawn and passes each retained listener to the existing forwarder.
  The browser receives the actual published port afterward.
- `engine/src/state/browser_identity.rs` maps actual runtime ID to private
  profile_id and atomically moves it for same-spec Dead replacement. Standalone
  and managed retry already use this mapping. New births get a fresh namespace.
- `state/lifecycle.rs` records active `port_bindings` at Running, then deletes
  them on exit/death/removal/recovery. Current state-change events do **not**
  preserve the allocated port list. `instance_browser_identity` cascades away
  when an instance is removed, while provider profile bytes may remain.
- `InstanceSpec` currently permits duplicate guest-port mappings; the browser
  authority independently rejects ambiguous mapped guest ports. A scoped browser
  origin operation must not silently redefine all general port forwarding.
- The legacy finite HTTP probe also uses raw PortLease allocation. It is not a
  normal logical application's browser-identity flow; all product ServiceExecution
  forwarding must use the new scoped allocator before claiming shared history.

## Proposed migration and private API

Migration6 adds private current assignments keyed by
`(profile_id,guest_port,protocol)` plus durable host-port ownership/history.
For the current local endpoint/address, a TCP host port can have only one logical
profile owner, including stopped/removed owners. No instance FK cascade may erase
that history. Separate active mappings from history if explicit configuration
changes are supported: an old port remains owned historically even when no longer
the current mapping. No URL, profile label or UI string assigns this ownership.

Suggested neutral operation:
`PortLease::reserve_for_instance(session,id,expected_version,binding)` returning
the actual live listener capability. Capture native private profile identity
before long package preparation; revalidate identity/version/spec at publication,
not merely a potentially reused actual ID. A small opaque native capture can
carry this private tuple. Actual common-owner generation/run admission remains
required; a saved assignment alone cannot authorize a worker or browser view.

For an existing automatic assignment, bind the exact saved port outside the
SQLite write transaction and retain the socket. Busy/error is failure; never pick
a replacement port. Revalidate current native identity/spec/mapping in a short
transaction before returning/publishing. The saved number is not a listener.

For the first automatic assignment, bind0 and retain the actual chosen listener;
claim that number through SQLite uniqueness/CAS only if the native tuple and
absence of another current assignment still match. Reject a number owned by any
other profile's history even if the OS calls it free. Initial allocation can try
another fresh candidate within a fixed technical bound before any origin has
been published. Exhaustion or concurrent same-key claim is explicit failure,
not a second silently published origin. Never probe-close-rebind a candidate.

Prefer binding the requested listener set, then one short claim transaction,
retaining every socket through spawn/forward/drain. A committed assignment/history
survives later spawn failure or interrupted start; partially prepared metadata
must not become Running. SQL failure before commit closes the private candidate
and publishes no mapping. Running/Ready actual mappings must agree with both the
durable assignment and the still-owned listener. Port metadata is not authority
to connect to a remembered host port.

The profile identity moves on retry, so current/history keys do not need to follow
old/new runtime IDs. Instance removal, browser erasure ACK, worker stop or ordinary
cache maintenance must not free historical ownership automatically. A clear
provider website-data callback is not proof that all old provider activity is
irreversibly destroyed. Retained history is finite address-space consumption;
any eventual reclamation needs its own owner-approved policy.

## Compatibility decisions that must remain explicit

**Legacy origin loss cannot be reconstructed.** Migration can backfill trustworthy
current actual port_bindings before recovery deletes them. Stopped/removed past
assignments are absent from both that table and current events. Do not read or
rewrite opaque provider databases, invent old ports, claim full historical
coverage, reset user data or silently mark migrated history complete. Known/new
assignments can become stable from this feature forward; an old stopped profile's
lost origin remains an explicit limitation. If the product demands a guarantee
covering every prior removed profile, that legacy case remains gated until an
approved migration/retirement strategy exists. New Notes instances need no such
retroactive reconstruction.

**Explicit host ports:** honour a matching explicit port and reject another
profile's historical port. A different explicitly requested port is a deliberate
origin change, unlike automatic restart fallback. It may be supported by
bind/check/commit of a new current assignment while retaining old history, but
must not be called same-origin continuity. Its user-facing review/consequence
needs to be specified without weakening the existing advanced configuration
contract. Changing the guest port and duplicate guest mappings also needs an
explicit scoped outcome; no silently different key or unstable fallback.

**Other processes:** another Android app or unmanaged listener may occupy the
saved port while WasmDroid is stopped. The next start fails closed. Retaining all
listeners across stopped apps/process death would require a materially different
background lifetime and is not proposed. Internal DB history prevents another
managed logical app reusing known ports, not another OS application. Existing
profile isolation and lifecycle revocation remain necessary; all complete and
exclusive browser-network capability flags remain false.

## Reviewable test/acceptance plan

Native checks: migration/backfill and unknown-legacy distinction; exact retained
socket proof; saved busy port with no fallback; concurrent first claim and loser
cleanup; history across removal/profile clear/retry and old-ID recreation; explicit
and ambiguous mapping behavior; SQL/commit/spawn failure; stale version/private
identity; final Ready consistency; old forwarder held until actual drain.

Actual Android Notes check, later explicitly authorized: write a unique
localStorage value in the isolated embedded profile, stop/retry/reopen at the
same numeric origin and verify value plus peer preservation. Repeat relevant
crash/recovery/provider scenarios. A uniquely test-owned listener occupying the
saved port must cause explicit start refusal and preserve data; release only
that owned listener and retry the same port. Do not claim service-worker/full
egress isolation, a real run or release acceptance from these proposed checks.
