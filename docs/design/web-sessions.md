# Embedded application web sessions

Implemented locally; provider/device and visual acceptance are pending. The
existing Open / Back / Browser scheme is retained. Native browser-data controls
are now locally implemented after the owner's English scheme was shown.

TCP ports are not cookie boundaries. Sharing WebView's default profile between
local applications would share cookies for `127.0.0.1`. Embedded views therefore
select an AndroidX WebKit profile derived from the SHA-256 of a private native
browser identity before any other WebView call. The profile's cookie manager
disables third-party cookies. Reopening the same application retains its profile;
a newly created application gets another profile even when a host port is reused.
Browser data is app-private
and is not part of the current per-volume backup format.

Normal worker forwarding now retains a numeric loopback host port per actual
native profile/guest TCP port. Migration006 preserves known active assignments;
new assignments and history survive stop/retry/removal. A saved busy port fails
explicitly instead of silently selecting another browser origin. Missing legacy
origins cannot be reconstructed; changed saved host ports remain Unsupported
pending their origin-change review flow. See
[stable origin contract](browser-stable-origin-proposal.md). This source change
still requires actual WebView/localStorage acceptance and does not strengthen
the complete/exclusive browser-network capability flags.

The installed WebView provider must advertise `MULTI_PROFILE`. An older provider
gets an explicit update message and the existing external browser option; there
is no shared-profile fallback. The external browser manages its own session and
does not inherit this isolation. Camera/microphone and geolocation requests are
denied, host file/content access is disabled, and no Android JavaScript interface
is registered. Renderer failure releases the failed view and shows an error.

Navigation and ordinary resource callbacks restrict the view to the actual
mapped loopback origin. These callbacks alone are not a complete WebView network
sandbox: service workers, WebSocket/WebRTC, redirect behavior and requests racing
service shutdown/port reuse still require explicit final security evaluation.
The guest's Rust virtual-network policy does not automatically apply to the
browser process. Do not certify unrestricted registry web pages or claim complete
browser egress enforcement from the callback checks.

Profiles remain preserved by default after instance removal, alongside separately
retained volumes. No age-based cleanup, removal-triggered erasure or global cookie
deletion is introduced. An explicit per-application erase backend and native
data-management pane are locally implemented; provider/rendered acceptance is
still pending.

## Owner-issued browser sessions

The common supervisor now publishes `BrowserSessionAuthority` mappings only from
the actual ready worker generation and its acknowledged TCP ports. AppModel's
browser targets carry that generation, guest port and local path. The previous
URL/port snapshot alone cannot acquire a browser session. Acquisition checks the
current ready/not-stopping Job and then the owner-issued generation mapping.
Unknown, stopped, stale-generation and unpublished-port selections fail. A private
native `browserIdentity(actualInstanceId)` lookup supplies the profile identity;
caller labels, target URLs and UI-provided IDs never assign that mapping.

Legacy rows use their original instance ID as the profile identity, preserving
the exact existing profile-name SHA-256 and bytes. New allocations use a separate
canonical `browser:<positive i64 creation-event sequence>` namespace, which cannot
collide with valid legacy instance names. Standalone/managed same-spec Dead retry
atomically moves the old profile identity to the replacement runtime instance.
New applications/package replacements get new identities. Preserving an alias
does not mutate or reuse a terminal runtime identity. Migration/retry evidence is
separate from actual provider/browser-data acceptance.

Profile retention preserves provider bytes and cookie scope; it does not preserve
origin-keyed storage access when an automatically allocated host port changes.
No storage migration or stable-origin gateway is implemented. Cross-port
localStorage, IndexedDB and service-worker continuity remains unverified. The
same-port provider retry regression checks cookies only and cannot establish
cross-origin storage continuity.

Opaque leases retain the exact actual instance, private profile identity,
generation and mapped host port. The
common controller revokes synchronously before stop IPC, worker death/slot release
or owner shutdown. Revocation invalidates ordinary request admission immediately
and posts destruction of the associated WebView. Late callbacks for an older
generation cannot revoke a replacement; a reused port never revives an old lease.
Acquisition is bounded to 64 active browser sessions. `LocalWebScreen` obtains
this authority before constructing/navigating a view, destroys the view before
releasing its profile lease, and closes on renderer/authority loss. Profile-specific
service-worker settings also disable host file/content access, independently of
the corresponding view settings; this adds no browser network guarantee. Existing
external-browser launch checks the lease at launch; the external browser remains
outside application profile/revocation control after that handoff.

This is lifecycle admission and revocation, **not an exclusive browser transport
gateway**. Requests that bypass WebView callbacks or race a previously accepted
request can still escape those checks. The embedded mode retains its existing
best-effort callback restrictions. Capability fields
`complete_browser_egress_isolation` and `exclusive_generation_gateway` are false.
A caller requesting the complete guarantee receives Unsupported; no permissive
fallback is substituted. A real generation-bound forwarding/network primitive and
its acceptance are still required for the full port-reuse/egress guarantee.

## Explicit profile-data erasure

`BrowserProfiles` is a process-wide owner of profile attachment and erasure.
`BrowserProfileLedger` grants active-view leases for hashed private profile identities,
rejects erasure while any view lease exists, and rejects profile reopening while
an erase receipt is pending. View destruction must finish before its lease is
released. Multiple views may share the same profile only within one actual worker
generation. A replacement generation cannot acquire that stable profile until
every old-generation view has been destroyed and released; old leases also fence
erasure via the new actual instance ID. `BrowserProfiles.acquire(ownerSession)`
accepts the owner-issued lease and derives identity/generation from it, rather
than accepting a raw UI profile mapping. Different applications retain independent
leases and data.

The common facade accepts the actual instance ID and resolves its private native
mapping. The product facade also returns an opaque `profile_token` derived from
that mapping. Erasure requires the displayed token and revision; the owner checks
the current mapping before admitting provider work. Reusing an actual instance
name for a different new application cannot consume an old confirmation even
when both profile revisions are zero. The token is not a label or configurable
profile assignment and is never saved in Bundle/saved-state output.
Internal `erase(profileId, expectedRevision)` requires the exact displayed revision and
both MULTI_PROFILE and DELETE_BROWSING_DATA provider support. It durably records a
random erasure generation before destructive work and calls
`WebStorageCompat.deleteBrowsingData` with that application's `Profile.webStorage`.
It never obtains/deletes the shared default storage or global CookieManager.
The native provider callback, followed by durable matching-receipt publication,
is the completion authority. A timeout or detached Activity cannot manufacture
success. The common supervisor retains the operation as background ownership
through its actual callback; closing waits for completion.

## Browser data controls and confirmation

The approved native Browser data pane is available from application data/details,
including applications without a mounted dataset. It reports preserve-by-default,
available/in-use/clearing/interrupted/changed/unsupported/unavailable states and
limits its action to the selected embedded profile. File datasets, other
applications and external-browser data are outside that action.

Clear and retry both require a new explicit confirmation bound to the exact
in-memory owner profile token/revision and the exact opaque prompt object shown.
A late callback from an older dialog cannot confirm a replacement prompt, even
when both revisions are zero. No write begins when the dialog is opened
or dismissed. Living views, a pending provider operation, unsupported capability,
stale metadata or lost connection prevent confirmation/admission. A changed
identity/revision discards the old confirmation and requires a reload. The common
owner remains authoritative if a view appears between the displayed snapshot and
the actual erase request.

The private controller retains no cookies, URLs, guest settings or saved-state
tokens. Leaving the pane cancels only observation; it does not cancel or complete
the retained provider operation. A clearing snapshot/poll alone cannot show a
success notice: the command must receive the actual owner acknowledgement and
then reload a matching current owner token, a later revision and a cleared
receipt. A late pre-ACK read or a disconnected owner's read cannot overwrite that
reload. An old-profile acknowledgement cannot claim a recreated application was
cleared. Interrupted/failed results remain explicit and retryable after another
confirmation; automatic retry or fake timeout success is not introduced.

Twelve unexecuted controller definitions cover explicit consent, same-zero-revision
identity replacement, revision changes, provider-ACK versus polling, stale reads,
connection changes, view disposal, interrupted retry, malformed metadata and
private diagnostics. They complement the fifteen earlier authority/provider
definitions. Compilation, rendered/accessibility/rotation and actual provider
execution remain distinct evidence.

The provider's documented scope is website data stored before the request,
including network cache, cookies and JavaScript-readable storage. Concurrent new
writes are not atomically covered. App-owned living views and new profile
admissions are fenced, but this does not certify all autonomous provider/service-
worker behavior. Returned state explicitly reports
`erasure_scope: website_data_stored_before_request`; `profile_directory_wipe` is
false. ProfileStore directory deletion is not claimed: loaded/living profiles
have deletion constraints and some disk deletion may be asynchronous.

Receipt failure or process interruption leaves a durable pending fence. Status
reports failed/interrupted rather than preserved or cleared, and reopening is
denied until another **explicit** erase retry completes. Restart alone never
automatically authorizes deletion. Unsupported providers are rejected before
creating a destructive pending receipt. These receipts contain only revisions,
operation generations and outcomes under hashed stable private profile keys; they contain no
cookies, site URLs, credentials or guest configuration. Browser data stays outside
volume backup and guest filesystem policies.

Primary API references: [profile selection](https://developer.android.com/reference/androidx/webkit/WebViewCompat#setProfile(android.webkit.WebView,java.lang.String)),
[profile storage](https://developer.android.com/reference/androidx/webkit/Profile),
[profile deletion constraints](https://developer.android.com/reference/androidx/webkit/ProfileStore#deleteProfile(java.lang.String))
and [request interception](https://developer.android.com/reference/android/webkit/WebViewClient#shouldInterceptRequest(android.webkit.WebView,android.webkit.WebResourceRequest)).
Profile-data clearing uses the pinned AndroidX 1.14 API verified in the local AAR
and the official [WebStorageCompat contract](https://developer.android.com/reference/androidx/webkit/WebStorageCompat).
The [ProxyController contract](https://developer.android.com/reference/androidx/webkit/ProxyController)
applies process-wide to WebViews; it is not adopted as a per-instance firewall.

`WebProfileIsolationTest` defines an on-device same-host/same-port cookie
regression. It is not evidence of a completed run or whole guest/browser E2E.
New unexecuted definitions cover generation/port reuse, stable replacement profile
identity, exact legacy hashes, namespace collision rejection, old-generation
view/erase fences and actual provider cookie retention across replacement;
stale/late revocation,
strict Unsupported, ordinary URL restrictions, session bounds, profile lease
exclusion, persistence/revision failure, interrupted explicit retry and actual
provider acknowledgement/cookie preservation in a second uniquely named profile.
Compilation is separate from executed evidence. Real stop/restart/rotation,
concurrent erasure/write/service-worker behavior, renderer/provider failure,
external handoff and full browser-network acceptance remain pending.
