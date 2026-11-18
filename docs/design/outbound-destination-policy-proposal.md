# Outbound TCP destination policy and coverage

The initial2026-11-17 proposal below now backs `network::egress` and fifteen
executed pure native policy regressions. Actual OS/VPN/provider acceptance remains
separate. The owner approved separate default-off Internet
and outbound-LAN permissions, including selected-network DNS for LAN-only access.
RFC1918/ULA as an initial supported LAN address class is an implementation
proposal below, not an owner-approved definition of every LAN topology. Missing
address/scope support remains open work. This does not substitute for WebView
isolation.

## Minimal contract

1. Add a pure `classify(address, trusted_context)` returning `Internet`, `Lan`,
   `Denied(reason)` or `Unsupported(reason)`. Separate permission matching from
   classification; Internet is never the complement of private space.
2. Trusted context is private and immutable per captured network epoch: own host
   addresses, applicable on-link routes/broadcasts, any trustworthy NAT64 prefix,
   and current plus historical private group-name inventory. No guest-supplied
   address labels, spec labels, guessed netmasks or boolean `public` authority.
3. Always deny host loopback/unspecified, actual host-owned addresses, multicast,
   limited/directed broadcast, reserved guest namespace10.203.0.0/24 and aliases
   of these destinations. This applies to external host sockets; existing virtual
   guest-local sockets/private group routes continue under their existing grants.
4. Classify RFC1918 (`10/8`, `172.16/12`, `192.168/16`) and ULA (`fc00::/7`) as LAN
   only after hard-denial precedence. This address class does not mean physically
   adjacent: the selected default/VPN network may route it remotely.
5. Use a reviewed, versioned current IANA special-purpose table with longest-prefix
   handling, not `!is_private` or an old hardcoded three-range denylist. A special
   entry marked globally reachable is not automatically a usable ordinary TCP
   endpoint; unsupported transition/overlay cases remain explicitly excluded.
6. IPv4 Internet candidates must be unicast, outside forbidden/special local space;
   IPv6 candidates must be ordinary supported global unicast, excluding the
   specific-purpose/transition/documentation ranges below. Reachability is never
   asserted from allocation/classification alone.
7. Global-address bits do not prove off-link location. An observed on-link global
   destination must not silently acquire Internet permission or an unreviewed LAN
   grant. Initially return Unsupported for that category.
8. Reject IPv6 link-local/zone-scoped sockets initially; `fe80::/10` needs actual
   interface scope and is outside the initially proposed RFC1918/ULA class. Never strip
   a zone ID to make the address appear globally routable.
9. IPv4-mapped/compatible aliases must never escape IPv4 denial. A simple first
   implementation can reject mapped/compatible wire forms entirely; canonical
   IPv4 routing would require consistently using the canonical address at both
   classification and actual connect, not only in a comparison.
10. Initially return Unsupported for6to4/Teredo and unimplemented translation
    forms. Known NAT64 requires real captured prefix plus RFC6052 decoding and
    classification of the embedded IPv4 under the same permission; no blind scan
    of every IPv6 suffix and no invented public/private conversion.
11. Resolve only when at least one effective permission is granted, using the
    exact current app-default `Network`, including the applicable VPN. No Wi-Fi
    request, underlying physical-network fallback, process-wide route override,
    alternate resolver, automatic retry or user-grant inference.
12. LAN-only DNS is an explicit approved control-plane allowance, even if that
    network's resolver itself has a global address. It does not grant TCP to DNS
    answers. Preserve the OS resolver/Private DNS rather than crafting raw queries.
13. Canonicalize and validate DNS spelling before provider dispatch; all known
    actual group aliases, including not-yet-granted/stopped members, must be
    reserved before external lookup and remain reserved after revocation.
14. Classify each numeric DNS answer independently, recheck current name inventory
    after asynchronous DNS, then enforce the same policy before and after actual
    connect. No hostname-based exemption, string-prefix test or second resolution
    between approved numeric result and socket connect.
15. Returning only permitted answers can support mixed responses; current seam
    instead rejects the entire response on any forbidden answer. That conservative
    behavior is safe but must not be described as filtering. Empty authorized set
    is a visible permission/unavailable outcome, not a public fallback.
16. Changes to network/default/VPN/context/grant/private inventory invalidate the
    capture. Retain actual provider work, slots and ownership until completion;
    dropping an observer or hitting a deadline is not a fake Closed receipt.

## Focused ranges and sources

The current IANA IPv4 registry distinguishes private, shared, link-local,
benchmarking, documentation, reserved and globally reachable exceptions. Relevant
non-Internet ranges include `0/8`, `100.64/10`, `127/8`, `169.254/16`, most
`192.0.0/24`, `192.0.2/24`, deprecated `192.88.99/24`, `198.18/15`,
`198.51.100/24`, `203.0.113/24`, `240/4` and limited broadcast. Multicast `224/4`
is separately forbidden. Do not accidentally drop a more-specific registry
exception when implementing a parent prefix, or mistake a blank/deprecated
registry value for globally reachable. [IANA IPv4 special-purpose registry](https://www.iana.org/assignments/iana-ipv4-special-registry/).

Relevant IPv6 exclusions/explicit unsupported categories include `::/128`,
`::1/128`, mapped `::ffff:0:0/96`, compatible `::/96`, discard `100::/64`, dummy
`100:0:0:1::/64`, protocol assignments `2001::/23` with specific exceptions,
Teredo `2001::/32`, benchmarking `2001:2::/48`, documentation `2001:db8::/32`
and `3fff::/20`,6to4 `2002::/16`, SRv6 `5f00::/16`, ULA `fc00::/7`,
link-local `fe80::/10`, multicast `ff00::/8`. ULA is LAN, not Internet.
`fec0::/10` deprecated site-local is not silently added to LAN. [IANA IPv6 special-purpose registry](https://www.iana.org/assignments/iana-ipv6-special-registry/).

The assignable general global-unicast allocation is `2000::/3`; currently
unallocated space is reserved, so that single prefix alone is not an allocation
or reachability oracle. Pin reviewed allocation/special tables rather than fetch
them at runtime. [IANA global-unicast assignments](https://www.iana.org/assignments/ipv6-unicast-address-assignments/).

ULA identifies the whole `fc00::/7` class; locally generated addresses use the
L-bit set (`fd00::/8`), while L=0 allocation was left for a future method. That
does not convert fc00 addresses into Internet destinations. [RFC4193 §3.1–3.2](https://www.rfc-editor.org/rfc/rfc4193.txt).

IPv6 has no broadcast addresses. IPv4 directed broadcast depends on actual
subnet information; do not ban every address ending `.0`/`.255`. `/31` point-to-
point links are a specific counterexample to ordinary network/broadcast host-bit
rules. [RFC4291 §2](https://www.rfc-editor.org/rfc/rfc4291.txt),
[RFC1812 §4.2.3.1](https://www.rfc-editor.org/rfc/rfc1812.txt),
[RFC3021](https://www.rfc-editor.org/rfc/rfc3021.txt).

## NAT64 and topology limits that must not be hidden

RFC6052 supports prefixes of lengths32/40/48/56/64/96, not simply “the last32
bits.” The well-known prefix is `64:ff9b::/96`; its non-global embedded IPv4 use
is prohibited. Network-specific prefixes can translate non-global addresses;
`64:ff9b:1::/48` is a local-use translation allocation. An implementation must
classify the effective IPv4 and preserve the selected capture, or honestly report
unsupported translation.6to4 and Teredo have different encodings and must not be
misparsed as NAT64. [RFC6052 §2–3](https://www.rfc-editor.org/rfc/rfc6052.txt),
[RFC3056](https://www.rfc-editor.org/rfc/rfc3056.txt),
[RFC4380](https://www.rfc-editor.org/rfc/rfc4380.txt).

Android `LinkProperties.getNat64Prefix()` is public from API30 and may return
null; link addresses/routes are available, but are not a proof that an arbitrary
remote/global prefix never undergoes translation. Unknown translator topology,
especially on older Android/provider/VPN combinations, cannot be solved by a
pure address-bit classifier. Do not describe unknown NAT64 or general router/
proxy behavior as fully mediated merely because recognized prefixes are denied.
[Android LinkProperties](https://developer.android.com/reference/android/net/LinkProperties).

IPv6 on-link determination is separate from assigning a local address/prefix;
never invent a `/64` from an interface address and call that exact routing
authority. On-link global addresses and unknown/hidden underlying VPN topology
need an explicit unsupported/limited capability outcome for a strict Internet-
versus-physical-LAN claim. This is different from the implementable narrower
contract of classifying actual socket destination addresses plus known context.
[RFC5942](https://www.rfc-editor.org/rfc/rfc5942.txt).

## Existing source gaps and concrete next tests

`service_egress.rs::DestinationPolicy` is a private seam, not an implementation.
Its current resolver authorizes every answer or rejects the entire batch.
`service_network.rs::reserved_destination` canonicalizes loopback/unspecified
and10.203.0/24, but does not implement this broader Internet/LAN classifier.
`service_broker.rs::reserve_aliases` covers own/incoming/outgoing grants and their
history; actual declared but never-granted group members are absent. Add a trusted
inventory derived from current native stack membership/plan to private config,
not public permission labels; enforce it before DNS and after answers arrive.

Android's default-network callback explicitly includes the app's applicable VPN.
`Network.getAllByName` resolves on that Network and binding must use that same
captured Network. These facts support the proposed routing contract; they do not
prove physical routing or real-device execution. [ConnectivityManager default callback](https://developer.android.com/reference/android/net/ConnectivityManager#registerDefaultNetworkCallback(android.net.ConnectivityManager.NetworkCallback)),
[Network APIs](https://developer.android.com/reference/android/net/Network).

Tests before physical acceptance: all permission combinations; every prefix
boundary/exception; own-host overrides; context/broadcast `/31`; scoped IPv6;
mapped private/loopback/guest aliases; unsupported6to4/Teredo; RFC6052 lengths and
non-global embedding; mixed/rebinding DNS answers; ungranted/current/retired group
names with zero external resolver calls; inventory change during DNS; current
default/VPN switch during bind/resolve/connect; unknown-context failure; retained
provider work through cancellation. Actual VPN/private DNS/IPv6-only/NAT64/physical
LAN/OS permission acceptance remains later and separately authorized.
