# Scoped application widgets and Quick Settings

The English application/widget/tile proposal was shown before implementation.
This source batch adds a home-screen widget with Start, Stop, Open and Change
application, plus one independently configured Quick Settings tile. Both use the
same common-owner application descriptors as the normal application list.
Compilation, executed regressions and physical Android acceptance are recorded
separately by the coordinator; this document is a contract, not device evidence.

## Target and selection authority

The common owner now uses native stable application lineage and the group's
durable creation event. Descriptor state tokens cover actual member revisions,
worker generations, ownership, pending restart/boot tickets and the durable Stop
All event epoch. They stay stable across idle owner recreation when the actual
stopped state is unchanged; a process-random token would prevent cold widget use.
Pending automation remains stoppable even before a worker exists.

Read-only and normal UI bindings have distinct Android Intent filter groups.
The service tracks all active groups and retires the owner binding only after
the last group unbinds. Read-only binds never consume cold recovery; subsequent
normal binds/rebinds still apply the configured recovery path. Actual mutation
milestones refresh shortcuts; generic query/lifetime notifications do not, which
avoids a refresh/bind feedback loop.

Open waits for a current UI snapshot before publishing its web destination, then
uses the owner-issued actual primary generation/port and the existing browser
lease admission. Descriptive names never control endpoint lifetime. The Android
cold-owner/late-selection and endpoint-name regressions compile but remain
unexecuted; the physical launcher/OS flow is still separate acceptance.

The common owner supplies a bounded page of metadata-only descriptors and
resolves a typed target `{kind, id, identity_token}`. A standalone target follows
the native stable lineage through the existing same-spec Dead replacement; a
group target requires its actual ID and durable creation incarnation. Reusing a
display name or removing/recreating a native ID cannot retarget a shortcut.
Labels and caller graph/URL data never authorize membership, lineage or actions.

Each Android widget ID has its own private selection; the tile has a separate
`tile` selection. The stored record contains only scope, revision, a fresh random
selection token and the native typed target. It contains no recipe, environment,
arguments, URL, worker state, desired-run authority or credentials. Saving uses
an exact prior-record comparison and a new revision/token, including when the
same target is selected again. An old configuration window cannot overwrite a
later Save. Save resolves the actual current target first and persists selection
only; it never calls Start or grants desired-run authority.

Widget deletion writes an unconfigured tombstone. A restored/reused widget ID
requires explicit selection and cannot inherit an old click. Malformed records,
failed writes, exhausted revisions and unknown scopes fail closed. Because
SharedPreferences can change process memory before reporting a failed disk
commit, a failed write quarantines that scope in the current process. Such an
unacknowledged snapshot cannot authorize an action.

Configuration uses an in-memory ViewModel, private read-only owner binding,
explicit Save/Cancel/Reload and a technical page window of at most 50 descriptors.
Next page replaces the displayed window; Reload returns to the first page and
reads the current selection explicitly. Late callbacks carry the current load
epoch and target; Save rechecks selection CAS after asynchronous resolution.
The editor excludes the saveable-state registry. It does not copy specifications
or private fields into SavedState, preferences or error text.

## Immutable clicks and actual admission

Each immutable activity PendingIntent captures the exact selection record,
action and owner state token. Its explicit component and data identity include
selection scope/token/revision, action and a hash of the captured state token.
Updating a newer widget cannot mutate an already issued click into another
target or generation. No shortcut exposes StopAll, arbitrary service intents,
shell commands, host paths or caller-selected web URLs.

The private action activity requires an unlocked, visible UI and rechecks the
current selection and actual widget ownership. It resolves the target and
requires the same owner state token and supported action. The common owner then
rechecks the supplied selection/visibility/lock guard at dispatch and again after
asynchronous preflight, before actual job/group reservation. Native target
identity, current membership, lifecycle versions, data ownership and live worker
generations remain authoritative. An old Stop cannot stop a replacement worker;
an old selection cannot silently control a newly chosen application.

Start establishes the existing foreground service from the visible user action
and reports OS/admission failure without selecting another target. Stop uses the
bound actual owner and does not require an additional foreground-service start. An
admitted owner operation retains its real completion independently of the
activity observer. Rotation observes one retained action; a process-restored
action activity cannot replay a historical click. Leaving before reservation
invalidates the visible-UI guard. Leaving after admission does not claim to
cancel an already owned guest. Stop retains datasets and browser data.

Open passes only native target identity, expected state token and the original
selection capture to MainActivity. The common owner resolves the current primary
web target and generation-scoped lease; it never accepts an arbitrary URL. The
tile maps Stopped to Start and Running to Stop only when the descriptor permits
that action. A partly running application opens exact-application review in
WasmDroid; it does not guess whether to start or stop its remaining parts.
Starting/Stopping/Unavailable do not issue a toggle. Long-press opens the tile's
separate application configuration.

## Android entry points and lifecycle

The widget provider is non-exported, the action activity is private, and the
exported TileService requires `BIND_QUICK_SETTINGS_TILE`. The exported
configuration activity is a visible system/user selection UI; widget IDs must
belong to this provider. Tile actions use `unlockAndRun`, then recheck the
captured selection after unlock. API34+ uses
`startActivityAndCollapse(PendingIntent)`; the older Intent API is used only on
older supported Android versions. See the official
[TileService contract](https://developer.android.com/reference/android/service/quicksettings/TileService).

Read-only shortcut/configuration/status bindings use `ACTION_SHORTCUT_BIND` and
do not request cold-owner recovery. Normal UI recovery remains a separate
explicit-policy path. The service tracks distinct binding groups so a completed
widget read cannot retire the owner while the normal UI remains bound. Widget
refresh runs after actual owner state changes and on platform widget updates;
per-widget refresh serials and selection checks reject stale asynchronous
results. A bounded read timeout closes the observer binding; it never releases
an admitted owner mutation. Generic owner-lifetime notifications must not request
another widget refresh, which would create a bind/query feedback loop.

After configuration Save the activity explicitly publishes RemoteViews and
returns the assigned widget ID. Android does not supply that first update for a
mandatory configuration flow. See the official
[widget configuration contract](https://developer.android.com/develop/ui/views/appwidgets/configuration).
The platform may delay periodic updates. Shortcut state and action results are
always re-resolved at admission, rather than treating an old RemoteViews as live
authority.

Android foreground-service/background-activity rules and launcher/system UI
behavior still govern whether a user click can launch the visible activity or
foreground service. No background privilege or OS survival guarantee is claimed.
See [foreground-service restrictions](https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start)
and [activity-launch rules](https://developer.android.com/guide/components/activities/secure-bal).
Physical acceptance must verify the actual launcher, tile, lockscreen and target
SDK behavior; inspecting intents does not prove a launch succeeded.

## Regression and acceptance boundaries

`ShortcutSelectionTest` defines 15 targeted Android cases for independent
per-widget/tile selection, concurrent configuration CAS, deletion/reused IDs,
failed persistence, corrupt/cross-scope records, revision overflow, invalid scope
and global-action refusal, exact captures, queued selection changes,
lock/visibility refusal, replaced native identities, same-lineage retry with a
fresh generation token, partial-state review, distinct immutable PendingIntents
and installed component export/permission declarations. They send no
PendingIntent and launch no guest, tile, widget or lockscreen UI. Their actual
execution status belongs in the verification record.

Device acceptance remains separate: add/configure/reconfigure two widgets and
the tile; Save without a start; start/stop/open the exact standalone and group;
preserve another app; replace a selection while preflight/unlock is pending;
delete/recreate a target/widget; test partial groups, rotation/process death,
long-press, locked-device and denied/blocked OS launch behavior. Verify actual
completion and persistent data, accessibility/touch targets, current labels,
provider/web lease behavior and idle owner retirement with simultaneous UI/read
bindings. No device/launcher/OS/Play acceptance is implied by source compilation.
