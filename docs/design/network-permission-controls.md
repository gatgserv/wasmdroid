# Network permission controls

English conversation scheme,2026-11-17, shown before GUI implementation. This
scheme preceded the native dialog/controller implementation. The standalone
first-start/restart capability path and local checks are implemented; actual
Android networking/visual acceptance remains open.

My Apps exposes Network access for the selected actual application. The screen
identifies that app and has separate Internet and Private networks (LAN) controls,
both off for a newly prepared catalog application. A signed recipe's requests
never check the switches or become grants just because Install was tapped.

LAN helper text discloses use of the network's DNS to discover servers. Owner
approved this even when Internet is off; TCP answers remain independently subject
to the destination policy. The screen states that routing uses Android's current
application network, including its VPN. It exposes no implicit Wi-Fi/underlying
network override and does not change inbound Share.
It explicitly states that these permissions control the background service and
web pages may make their own network requests. No complete WebView firewall is
implied by either switch.

Save permissions applies a reviewed, versioned next-run change without starting
the app. Running/stopping, changed identity/version, unavailable owner/capability
and unsupported build states withhold editing/save and explain the next action.
Cancel does not persist a draft. Saving both off withdraws both future grants.
The native edit accepts only the two Boolean flags, checking actual birth
identity/version inside the configuration transaction. Actor worker/data-owner
admission remains separate. Unrelated configuration/data/boot/restart fields are
preserved; no metadata or native work completion is fabricated on UI cancellation.
Field-specific setters read the latest other draft flag, so fast callbacks from
one rendered frame cannot restore an old grant. Reload/dismiss/save use epochs.

The first scheme uses a standalone example. Managed combined-app grant scope,
atomic member/version admission and read-only member behavior require the next
native contract; do not silently apply a standalone edit to a managed member.
Destination coverage and platform limitations remain explicit in
[the researched policy proposal](outbound-destination-policy-proposal.md).

Prototype syntax/element references and local open/toggle/save plus running/
changed/unavailable guards were checked. Layout was not visually inspected;
no actual Android grant, DNS, socket or rendered GUI flow was executed by it.
Pure controller/capture-gate checks and matching compilation remain separate
from actual provider, startup, VPN/loss and accessibility/rotation acceptance.
