# Generation-scoped LAN ingress

The transient transport is now connected locally to the actual owning worker,
common owner, selected Android Wi-Fi and a user-visible English sharing dialog,
after the conversation scheme. This is an explicit ingress extension: the normal
loopback `PortLease` and default `InstanceSpec.network=Loopback` operate
independently from the relay. A running standalone web application with a
declared, published web port shares by default if exactly one Wi-Fi selection
is eligible and Android permits local access. Managed groups use manual sharing
for their declared primary web member. Sharing grants no guest egress.

`LanScope` records one native instance, worker generation, guest TCP port,
selected-interface epoch, explicit RFC1918 IPv4 address and an optional unicast
IPv6 address with an observed on-link prefix. Both listeners use the same
allocated port. The IPv6 relay admits only source addresses in that prefix.
Android observes the connected route and address in both owner and worker.
The relay binds each exact-address socket to the selected Android Network
before listening. One forwarding budget and revocation receipt cover both
listeners.

`GenerationConnector` must own the actual private worker/network capability.
Connecting to a remembered loopback host port is insufficient: death/reallocation
could direct a stale share into another generation. Both the connector and its
returned `ScopedStream` must match the whole immutable scope. The connector must
release pending connections when its future is dropped, retain no escaping stream
references and honor revocation. A supplied scope string alone does not attest
Android membership, user consent or a socket's provenance.

`LanLimits` bounds active/pending forwarding tasks to at most32 and connect time
to at most30 seconds; callers choose explicit values within those technical
bounds. There is no default product resource policy. Normal duplex forwarding
preserves half-close. Revocation fences new admission; awaited `shutdown()` or a
retained revocation handle's `closed()` supplies terminal cleanup evidence.
Dropping/cancelling an observer cannot acknowledge cleanup. Failed or panicking
cleanup is uncertain ownership until independently resolved. Child cleanup must
finish before the listener lease is released and closure acknowledged.

Portable tests exercise real retained loopback sockets through the same private
transport core. This test-only entry cannot weaken production private-address or
mandatory-binder validation. Host socket tests never establish selected Wi-Fi,
Android permission, Wasmer guest or a complete user sharing flow.

## Worker and owner integration

The owning `ServiceDriver` captures the real `ServiceNetwork` capability and its
private TCP stream, instance, generation and published guest port. It never
reconnects to a remembered host loopback port. Each enable attempt requires a
strictly increasing grant epoch, including Off/On on unchanged Wi-Fi. The FFI
adapter chooses an ephemeral host port and explicit technical bounds of16 clients
and10 seconds for connecting; these are forwarding bounds, not worker resource
limits. Stop/kill/drop fence new enables and revoke before namespace teardown;
native completion waits for actual forwarding cleanup.

The common owner resolves the actual standalone target or managed group's native
primary web member. Selection tokens are opaque references to a current Wi-Fi
candidate. Exact worker generation, port, grant epoch and private address fence
control messages, status and closed receipts. The worker reattests the selected
Network independently. Malformed or mismatched scopes cannot revoke a newer
grant. Closed-with-failure and uncertain cleanup remain distinct.

An explicit On attempt has a retained cancellation intent. The owner checks it
immediately before the actual Messenger send. The worker reserves on its Main
queue before launching the native thread; Reserved cancellation and dispatch
share one CAS. A no-native closed receipt is valid only when dispatch was prevented
or the request was declined before reservation. Once dispatched, cancellation
must await actual native closure. Cancelling an observer or closing the dialog
does not acknowledge native cleanup. Explicit Off survives dialog disposal.
Actual Binder death plus committed terminal state releases old-generation owner
captures; a timeout does not.

Android documents [binding a socket to a Network](https://developer.android.com/reference/android/net/Network#bindSocket(java.io.FileDescriptor))
before connection. The adapter duplicates the actual unconnected descriptor,
binds that duplicate to the selected Network, closes only the duplicate and
rechecks capture/admission before and after binding. It does not bind the process
or accept a user-supplied IP as proof. The
[NetworkCallback contract](https://developer.android.com/reference/android/net/ConnectivityManager.NetworkCallback)
is followed through ordered capability/link-property callbacks, without racing
synchronous lookups inside callbacks. Eligibility requires Wi-Fi, NOT_VPN,
non-cellular, non-suspended and canonical RFC1918 IPv4. An observed on-link
ULA or global-unicast IPv6 address with a prefix of 64–127 bits can add an IPv6
endpoint. ULA and stable addresses are preferred. API29+ also requires an
observed unblocked callback; API28 explicitly records that newer signal as
Unobserved and still requires actual selected-Network binding. Network/interface/
address/blocked-state loss invalidates captures and revokes admission. There is
no SSID/location lookup, default/cellular fallback or persistent grant.

The Android 17 target uses the platform's runtime `ACCESS_LOCAL_NETWORK`
permission for Wi-Fi sharing. The dialog requests it only after the user taps
the Android permission action. An eligible standalone web service can start
sharing once permission is granted. Both the owner and the worker check it
before admitting a Wi-Fi selection and while validating captures. A lost grant
invalidates the capture and hides the share address. Android 9–16 continue
using the existing network observation and worker admission checks. See the
[Android local network permission guidance](https://developer.android.com/privacy-and-security/local-network-permission).

## User flow and remaining acceptance

The English dialog exposes manual address selection when multiple selections
qualify, an HTTP exposure warning, the IPv4 address and QR, an IPv6 address
when available, and independent address copy controls. Explicit Start sharing
requires a visible, unlocked application. Stop sharing hides the addresses
and QR and awaits cleanup. Explicit Stop, an invalidated capture or a failed
initial attempt suppresses automatic sharing for that worker generation.
Re-enabling a stopped share in the same run requires a manual action.
Polling and copying revalidate the current scope and grant.

The first TCP publication added in the service form is selected for web access
by default. The application port starts empty, and the phone port is allocated
automatically unless configured. The form also allows selecting a different web
port or clearing the selection. **My apps** displays the active Wi-Fi URLs from
the current sharing grant independently of QR generation.

Pinned ZXing core3.5.4 generates QR locally, with tagged license/notice assets.
The generated matrix is tied to the exact displayed URI. Oversize/capacity errors
preserve the complete selectable address instead of truncating it. No QR web
service, scanner/camera integration, authentication or TLS is introduced. Anyone
able to reach the shared endpoint may access it subject to the guest's own
security; the UI warns that this transport is HTTP.

Host socket/model tests and matching Android native/Kotlin compilation are
separate from physical selected-Network binding, incoming peer HTTP, QR scanning,
API28/provider behavior, interface/permission loss, stop/death/drain and unlocked/
rotation/accessibility acceptance. Those checks remain pending until the final
implementation acceptance phase. Failed or uncertain native cleanup must never
be relabeled closed to make that phase pass.
