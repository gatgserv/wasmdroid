# Scoped outbound transport mechanism

The initial transport dependency is connected to a private Android standalone
first-start/restart path, explicit destination policy and reviewed normal
permission controls. Local checks remain distinct from actual Android network
acceptance. Old no-capability runtime entry points still reject positive flags;
persisted `NetworkMode::Lan` remains unsupported. Default loopback/private-stack
behavior is preserved. Running handover/regrant and managed combined-app grants
remain open work; full outbound delivery is not claimed.

`NetworkMode` currently governs private networking/host publication. Its Lan
variant is not an approved outbound-LAN permission. Transient Share is a separate
inbound feature. The owner approved both guest Internet and guest-to-LAN
connections, with separate permissions disabled by default. This is the approved
product direction. The owner also approved the captured network's DNS for discovering LAN
servers while Internet access is disabled, including disclosure that the network
provider can observe the queried name. DNS still uses the selected capability,
and resolved TCP destinations must independently satisfy the effective grants.
The implementation uses persisted, explicitly saved next-run flags and a fresh
network/worker capture. Destination coverage and remaining forms are documented
in the [policy contract](outbound-destination-policy-proposal.md).

On Android 17, the LAN grant flow also requires the host app's Android local
network permission. The user requests that permission from the review dialog
and explicitly saves the resulting guest grants. Android applies its own
local-network restrictions to worker sockets and mDNS, while verified ordinary
DNS answers still pass through the guest destination policy. Internet-only
operation does not require the Android LAN permission. Permission state is
checked at save time, and boot or automatic restart issues no permission prompt.

`InstanceSpec.lan_access` is an additive Boolean, independent of Internet access
and `NetworkMode`. It defaults to false and is omitted when false, preserving
legacy serialized bytes. Android configuration accepts supported requests; actual
startup requires a native-issued one-use capability. No-capability startup still
refuses positive flags. Other unsupported resource/protocol constraints remain.

Catalog recipe requests are not operator consent. Normal single preparation uses
`catalog_install::effective_single_spec` through `catalog_application_spec` to
clone the validated recipe with both outbound flags false. Signed request
metadata is unchanged for a later reviewed grant. Validation preflights that
effective clone. Install/preparation does not start a guest; a later reviewed
configuration supplies explicit permissions. The managed Compose subset has no
outbound grants and rejects tampered positive fields. Inbound Share remains
independent. No default storage quota or hard memory/CPU enforcement is added.

## Authority and real work ownership

The manager captures a mandatory private SelectedNetwork capability, mandatory
destination-authorizer interface, explicit technical limits, actual runtime
Handle and EngineOwnership. There is no ambient resolver/network or default
allow policy. Scope contains actual worker identity/generation, persisted
native record revision, grant epoch and Network epoch; clones retain a private
seal. Matching text fields alone do not recreate that capability.

Native capture issuance verifies the exact worker/specification/version and
Starting desired-run authority before long preparation, retaining its ownership
lease and proof. Matching public scope fields cannot create an Android adapter.
The one-use prepared capability is attached to actual ServiceNetwork before guest
spawn, with postcompile/native/stop revalidation. Driver startup releases its
control mutex before compilation/injected callbacks. Stop fences even preparation
without a published execution; no-capability create/start paths remain closed.

The private UniFFI host callback captures the actual application default Network,
including VPN, and bounded own-address/direct-route/known-Pref64 context. A Weak
native revoker is installed before preparation so loss/Stop can signal during
compilation without waiting for READY or creating a callback ownership cycle.
Provider guards compare fresh actual default/context before and after bind/DNS;
current() is atomic-only. Callback observation/dispatch is not an atomic OS
routing-transition or hidden-topology guarantee. API29+ capture waits for actual
unblocked status; API28 lacks that callback observation and retains OS enforcement.

Binder/resolver calls run in retained blocking tasks. Each owns its actual native
engine lease; binder work owns a duplicated descriptor through real Java return.
Guest-side capture checks read the native revocation signal. Admission still
checks the Android capture and installs the platform revoker before publication.
The provider checks capture validity before and after blocking DNS or bind. The
FFI wrapper releases the foreign callback on a native OS thread because an
in-flight Wasmer fiber can drop the last reference after a capture is replaced.
The original Java ownership PFD stays borrowed until Rust entry duplicates it,
including the no-egress production path, so argument-lowering failure cannot leak
a detached ownership fd. The legacy owned-fd constructor remains compatible.

The numeric scope field currently called spec_revision records the actual durable
instance revision at issue; it is not a separate configuration-only counter.
Lifecycle metadata may advance that revision while the installed specification
stays identical. A new attachment still needs the factory's actual current
record/spec check; replacement rejects lower record revisions rather than treating
all forward metadata revisions as a different worker permission.

One retained bounded runner owns commands and all child/provider futures. A
borrowed unconnected TCP descriptor is bound through the exact selected capability
before connecting. Resolution uses that same capability. Identity/current-state
and destination authorization are checked before and after external work.
Timeout/cancellation may complete an observer, but a pending platform future
retains its slot and ownership until it actually returns. Tokio connect/stream
work owns its descriptors and can be dropped safely during revocation.

Closed is published only after queued/child/provider work and actual streams are
gone. Panic/uncertain cleanup is an error, not a successful drain receipt. An
observer cannot manufacture cleanup by cancellation. Full guest client drop
cancels pending late connect/publication and established forwarding; a normal
TCP write half-close still permits its response. The latter is not a full drop.

## Namespace, replacement and lifecycle

Guest loopback, own aliases and private stack routes win before any external
dispatch. Reserved numeric targets cannot fall through. Canonical ASCII DNS
labels are required at this stage; malformed/Unicode-separator/percent/empty-label
spellings are rejected before a provider call. Current and previously configured
private aliases remain reserved for this generation, even after grant removal;
bounded history overflow fails before changing the existing grant table.

After a resolver await, the current actual namespace/manager/private alias
classification is checked again before exposing an external answer. Pending DNS
cannot publish an old public result after a real private alias is installed.
Actual native plan/member inventory now reserves every group service name,
including stopped/ungranted members, before provider dispatch; generation history
retains removed aliases. Caller labels do not select that inventory.

A replacement requires actual old drain, the same real worker/specification,
a strictly newer explicit grant epoch, a current new capability and an unfenced
namespace. Old capture loss revokes only its retained manager. Stop/kill/drop
permanently fence outbound admission even if the first installation has not yet
happened. Interactive terminal interruption revokes current work without
permanently removing the persisted permission; a later explicit capture still
needs its own current authority and newer epoch. No automatic regrant is implied.

Execution fences first and observes both LAN and outbound drain before terminal
success. No asynchronous shutdown is awaited under its control mutex. The common
Android owner still requires actual Binder death and durable terminal commit
before reusing a worker slot. Force-killing that exact worker remains the final
process boundary when native/provider work does not terminate.

## Remaining product integration and acceptance

The first Android path revokes a lost capture and requires restart to capture
another network. Same-generation handover/regrant and managed group operator
permissions remain source work. Scoped/link-local/transition/on-link-global
destination forms outside the supported policy remain explicit Unsupported.
UDP/raw/multicast and browser/other host-HTTP paths are separate capabilities and
are not certified by a TCP bridge. The permission dialog discloses that web pages
may still make their own requests. No native HostNetworking shortcut is used.

The driver transport/adapter/startup definitions use real loopback listeners with explicit injected test
policies/providers and gated futures. They are cross-compiled, not executed on a
supported runtime in this checkpoint. Those fakes never establish actual Android
Network binding/resolution. Final device/app-process/stop/death/no-leak evidence
and actual positive guest permissions remain required before feature completion.
