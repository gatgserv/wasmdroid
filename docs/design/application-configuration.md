# Application configuration

Tracked under the approved advanced-mode/lifecycle scope in
[issue #3](https://github.com/gatgserv/wasmdroid/issues/3). This batch implements
the native editing API, private typed draft codec and native Compose create/edit
form after its English conversation scheme. Actual acceptance is pending.

StateDb.configure_instance edits a standalone Created, Exited or Dead record with
an exact expected revision. Artifact reference, endpoint, kind and driver retain
their identity; a command must still exist in the installed artifact. Active
states and managed stack membership/reserved ownership labels are refused.
Changing a managed service belongs to its versioned definition workflow.

The specification and all named-volume references change in one SQLite
transaction with an instance_configured event and incremented revision. A no-op
does not manufacture an instance configuration event/revision. An explicit Save
can separately clear durable automatic-run authority, including a duplicate spec.
Missing/deleting volumes, unsupported
identity changes, stale revisions, invalid specs, overflow and failed writes leave
the old spec/references/events intact. Existing datasets, exit information, state
and previous execution output stay preserved. Removing a mount does not delete
its volume. Save creates no dataset, listener, guest or automatic restart.

The Android common actor checks live/starting/stopping worker ownership and data
ownership before reserving the instance for editing. Database state alone cannot
prove an idle worker during startup or pending observed death. The reservation
shares the existing per-instance data ownership fence, preventing launch/removal/
data work while the edit runs. Native work runs on IO, not the actor. Closing the
owner waits for completion. The facade captures engine ownership and releases
its shared session mutex before opening/writing a separate SQLite connection.
Native callers are trusted coordinator code, never guests or public Binder clients.

UniFFI applies the existing driver preflight and 128 KiB wire limit before a
mutation. Unsupported network, host mounts and unenforced memory/CPU settings
are still rejected explicitly. Restart/boot metadata is handled by the common
coordinator; its [implementation and acceptance](restart-and-boot.md) remain
distinct from saving a configuration. Invalid JSON errors do not
echo private field names/values. No migration or RuntimeDriver hot-update method
is needed: each later owned worker loads the newly persisted specification.

ServiceConfigurationDraft is the private in-memory codec for the form. It
represents display name, command, ordered argument rows, environment rows, guest
working directory, selected named volumes/read-only flags, TCP ports, network,
TTY/stdin and an optional timeout. Arguments are not shell-split; whitespace,
empty arguments, quotes and Unicode values survive. Duplicate environment keys,
mount targets and guest ports and invalid numbers/paths have field-only errors.
Names/values never enter its debug strings. This is not a secret-store feature.

The draft preserves unedited root fields, labels and runtime resource fields for
authoritative native validation. It rejects coercive or lossy nested conversion
rather than silently erasing unknown fields, unsupported source kinds or values.
Form budgets are 256 args, 128 environment rows, 32 mounts and 16 ports within the
same 128 KiB wire ceiling; larger representations can stay in guarded JSON mode.
The form does not approve unsupported capabilities. Explicit new-volume creation
uses a separate no-clobber volume operation and preserves newly created data
if subsequent configuration fails or is cancelled.

The native form has advanced-only create/edit destinations with Application,
Execution, Data and JSON sections in English only; private unsaved draft, masked environment
values, installed command/volume choices, explicit volume creation, automatic
phone port selection and field errors. Save applies on the next explicit start;
current running/owned state disables mutation, and stale revisions require reload.
Do not persist values in saved state, operation receipts, logs or error reports.
Read-only immutable package identity is descriptive, not a publisher trust claim.

The ViewModel retains private drafts across Activity recreation without saved-state
or preference serialization. Source epochs fence stale callbacks and asynchronous
loads. Each editor captures one common-owner connection; reconnection requires
explicit reload. Existing-record reload replaces the private draft with the current
native revision; new-record reload preserves it. JSON conversion refuses lossy
edits, immutable identity changes and invalid fields. Parsing is bounded to 128 KiB,
32 nesting levels and 16,384 structural tokens before the platform JSON parser.
Pending native save or volume creation prevents dismissal and duplicate actions.
Actual save acknowledgement clears the draft even if its connection changes;
refresh failure cannot offer a duplicate create. New volumes remain on discard.

Defined coverage includes transaction rollback, stale/no-op/active/member/identity
guards, actual retained dataset bytes, reopen, private events, draft round trips
and invalid input, actual native preflight, no-clobber volume creation, owner
data/SQLite-admission fences and controller source/revision/in-flight guards.
Compilation of these definitions is not execution evidence. Rendered/provider/
rotation/accessibility, actual next-run guest arguments/env/mounts/ports/TTY and
independent/runtime acceptance remain required with the final integrated checks.
