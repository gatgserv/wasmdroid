# Private service TCP network

Local source implementation; new runtime/device acceptance is pending. This
extends the production worker beyond the historical inbound-only fixed probes.
Stack routing has a local implementation; integrated acceptance remains pending.
Internet/LAN permissions and browser egress policy remain open. This does not
supersede the recorded shared-process stop failures.

## Namespace and authority

Each worker/run gets one `ServiceNetwork`. Guest TCP listeners, source-port
reservations and connected channels live in memory. Guest bind/connect/resolve
never call the host TCP stack or DNS resolver. There is no implicit connection
to the common coordinator, an ungranted service, the phone's loopback services,
LAN or the internet. A guest knowing an actual published host port still cannot use
that number to reach the phone's listener through its private namespace.

Guest IPv4 localhost and IPv6 localhost are separate address families. Wildcard
binds stay in the private namespace. A dual-stack IPv6 wildcard bind reserves
one port across both private loopback families. IPv4 peers appear as IPv4-mapped
IPv6 addresses to that listener. An explicit IPv6-only wildcard or `::1` bind
accepts only IPv6. Port zero allocates a private ephemeral port from 49152–65535. It
does not reserve a host socket. Source bindings survive a successful connect
until that endpoint closes/drops. Binding/listener ownership uses weak registry
entries so dropping a listener releases its bind reservation, while already
accepted streams remain separate connections. There is no TIME_WAIT emulation.

Direct connect and an explicitly bound socket's connect both use an exact peer
lookup. A missing listener refuses the connection. They never use upstream
LoopbackNetworking's first-listener fallback. Numeric localhost and the static
case-insensitive names localhost/localhost. resolve locally. A managed stack
can additionally resolve only its granted peers and their assigned virtual IPv4
addresses. Other names, custom DNS servers, ungranted IPs, UDP/raw sockets and
routing/interface administration remain denied/unsupported. Shared SO_REUSEPORT
listening remains unsupported.

TCP buffer options are advisory within a fixed actual capacity of 64 KiB per
direction. Requests outside 1–65536 fail. Accepted smaller requests retain the
actual capacity, which getsockopt reports: shrinking the pinned upstream ring
can silently drop queued bytes, so the service wrapper never invokes that resize.
Other TCP tuning follows the pinned in-memory transport; this is not a complete
host POSIX TCP implementation.

## Generation-scoped stack transport

The common owner derives each WorkerNetwork config from the actual stack_services
membership, not InstanceSpec labels. Stable sorted service aliases map to logical
10.203.0.2 through 10.203.0.9 addresses; these are virtual identifiers, not host
interfaces. Own alias resolves to private localhost; peer aliases (case-insensitive,
optional final dot) resolve only to explicitly granted peers. IPv6 stack mesh,
custom DNS and arbitrary host networking are unsupported. NetworkMode none still
disables networking. Standalone services get no stack peer authority.

Each directed edge has an independent 256-bit SystemRandom key. A private Unix
endpoint name hashes instance identity and generation, under engine/network in
the private app directory. Guest code receives no host path or key. Configs are
transient trusted IPC, absent from specs, durable events, logs, UI snapshots and
backups; debug formatting redacts keys. Config validation checks identity,
generation, unique peers/addresses/names and current database membership.

A connection sends a 12-byte WDP1/source virtual IPv4/source-port/destination-port
header and HMAC-SHA256 tag. The receiver validates the current incoming grant
before opening that exact guest listener and acknowledging it. Source identity
comes from the authenticated grant. Connect remains Opening until positive ACK;
read/write readiness, status and last-error expose failure rather than fabricated
success. Direct and bound connect share this authority path. Establishment and
incoming handshake each have a three-second bound. Established byte streams have
no blanket three-second lifetime and use bounded in-memory channel pairs.

Versioned replacement preserves unchanged identity/generation/address/key leases
and revokes removed/changed leases plus their accepted/pending pairs. A restarted
peer gets new keys and a new endpoint. Changing a third member does not deliberately
reset an unchanged edge. Unknown/dead endpoints cannot fall back to phone TCP or
another listener. The per-worker incoming broker admits at most 32 handshake/live
tasks; outgoing channels share the namespace's 128-pair bound. Incoming and outgoing
bridges participate in normal two-second drain and immediate stop revocation.

Endpoint creation rejects a symlink/non-directory root and overlong platform paths.
Normal drop removes only the socket inode this owner created. Process kill may
leave inert socket directory entries; fresh exclusive EngineSession recovery
removes only recognized socket files, preserving unrelated files and symlinks.
Joining a worker never runs that sweep against live peers. This relies on Android
app-private storage and the existing engine ownership lock; it does not claim
protection against another compromised native same-UID component.

## Publication and stop

Only InstanceSpec.ports entries create host ingress sockets, still exclusively
reserved on 127.0.0.1 through PortLease. The actual allocated host port is
published by the common controller after READY. Internal guest listeners work
without an InstanceSpec.ports entry. The forwarder connects only to its configured
guest port, trying that exact IPv4 localhost endpoint then its exact IPv6 endpoint
within the same namespace. It does not enumerate unrelated listeners.

Waiting for that guest listener is bounded to 30 seconds. Once established,
streams survive until EOF/error or service-stop cancellation; the old blanket
30-second connection lifetime would incorrectly truncate WebSocket/DB sessions.
Each host forwarder still admits at most 32 live/waiting connections. On normal
guest completion, new ingress is fenced and established forwarders get a bounded
two-second drain interval (three seconds as the outer poll deadline). The final
runtime_info.forwarding_drained flag records whether all forwarder tasks drained
without forced cancellation. It is not acknowledgement that the remote application
consumed every byte. Explicit stop still revokes the network immediately; OS death
remains the final group boundary.

Stop first fences new guest work, then closes this namespace and its forwarding
tasks. The namespace denies new binds/connects/resolution, closes pending
listeners and revokes accepted streams even if their original listener has been
dropped. The separate Android process/Binder death gate remains the final group
cleanup boundary; in-process network closure alone does not prove execution ended.

## Bounds

| Resource | Per-service bound |
| --- | --- |
| Reserved listener/client source ports | 128 |
| Allocated connection pairs, accepted or pending | 128 |
| Pending connections per listener | 32 |
| Ring capacity per direction | 65,536 bytes |
| Distinct pending listener poll wakers | 32 |

The maximum connection payload ring allocation is
128 × 2 × 65,536 = 16,777,216 bytes (16 MiB). This excludes metadata, upstream
transport wakers, guest allocations, host-forwarder buffers, JVM and compiler;
it is not a total process memory limit. Closed empty pairs no longer consume
admission even if callers retain their closed wrappers. Quota errors preserve
existing sockets instead of silently evicting another service's work.

## Wait receipts

While defining the fast-exit network acceptance, inspection found a lifecycle
race: waitForExit returned an exit envelope for an active job but a bare instance
record if Binder death won the race. The controller now retains the last 16
completed receipts, including generation and verified guest result, after
observed death and committed terminal metadata. A new run or removal invalidates
that instance's prior receipt. Waits before and just after completion receive the
same envelope. Retained receipts can contain guest output/specs and stay private;
they are cleared with the owner and never added to events/telemetry.

If no receipt survives owner recreation/eviction, the method returns the terminal
instance envelope with unknown generation/forced/result fields. It never attaches
an older run's latest-by-mtime output to a newly failed run or invents an exit.
Calling wait without an active or completed execution fails explicitly.
The existing private output/log retention is separate from this bounded receipt
cache; this change does not claim durable complete exit receipts.

## Pending acceptance

Driver-local regression definitions cover exact routing, bound connect, private
namespace isolation, wildcard/ephemeral ports, listener rebind, source ownership,
fixed buffers, stop revocation, backlog/port/connection quotas and empty-pair
reclamation. They have not run and are not compiled by the production Android
library or the Windows neutral engine/FFI targets.

SupervisedLoopbackTest defines live public Python execution in two workers under
the common owner: a held private listener must be invisible to the other service;
that second service binds the same virtual port, uses a second ephemeral listener,
rejects an unused port without fallback, connects through localhost, verifies two
distinct replies, exposes no host ports and preserves the first service. It checks
a late wait receipt too. A third explicitly published service exchanges bytes
through a real host socket after 31 seconds and checks its final drain flag.
This test has not run. Long-lived host forwarding and
IPv6/quotas/stop races still require integrated acceptance, along with the stronger
two-service hot-descendant/process-death/volume contract. New neutral grant and
managed-retry regression sources compile without execution. Driver broker tests
define wrong-HMAC rejection before listener enqueue, positive ACK/readiness, exact
byte delivery, authenticated source IP, stale revision rejection and grant rotation
revocation; their cfg(test) sources are not compiled by production Android or the
neutral Windows targets. SupervisedStackTest compiles but has not executed its
real two-worker TCP counter/restart/persistence scenario. Cross-stack denial,
capacity, startup races and unchanged third-peer flows remain required acceptance.
