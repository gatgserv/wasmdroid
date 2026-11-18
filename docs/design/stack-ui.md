# Stack management UI proposal

The proposal was presented in the conversation before implementation. Native
StackScreens and StackImportController now implement these flows locally;
runtime/provider/visual/accessibility acceptance remains pending. The draft lives
in its ViewModel across rotation and is cleared after creation or explicit discard;
it is intentionally lost on process death. The document editor has no saveable
state registry, and its text is not written to saved-state bundles.

Prepared before Compose implementation. The conversation mockup uses
illustrative apache/database names; it does not advertise installed native Apache,
a curated database recipe or a verified catalog item. Existing native Material
navigation, English-only application text and light/dark themes remain the base.

## Entry and scope

My Apps adds Apps/Groups selectors inside the existing destination. Group summaries
use actual membership and small state APIs. They must not count a definition as
running, use a historical creation-plan identity after retry or copy private
environment values into list rows. Group creation/import is an advanced action;
ordinary group viewing and lifecycle controls do not require exposing YAML.

## Import and preview

The user supplies a unique stack identity and chooses a YAML/JSON document through
Android SAF, or edits the bounded document in advanced mode. Read at most 128 KiB
UTF-8 using the existing parser contract. Preserve drafts across configuration
changes without putting document/env contents in unprotected logs or saved-state
bundles. Oversize, malformed input, unsupported fields and missing installed
packages fail explicitly before creation; do not silently modify permissions,
download dependencies or invent a healthy preview.

Preview shows services, exact package/command selection, dependency order,
datasets, requested published ports and private peer scope. Explicit arguments/
environment are private inspection detail, never ordinary notifications. Creation
pins the reviewed normalized plan and stack identity; revalidation rejects changed
installed digests before dataset preparation. Disable stale preview/create actions
when the draft changes. Late results are ignored after edit/navigation; leaving
import cancels reads/preview, closes provider streams and preserves the prior draft.
An already submitted creation remains with the common engine when UI closes.
Creating a definition does not start any guest. Starting remains a separate action.
Import and edit review now use a read-only native inventory with the strict
Compose parser: package references, logical service names and installed/downloadable
status, with no private args/env copies or state mutation. Missing public packages
offer an explicit common-owner download; imported digests require importing exact
local bytes. Existing package progress/cancel/retry remains owner-retained. No
download occurs on composition or preview. Completion requires a new explicit
definition review and never creates a group or starts a service.

## Stopped definition editing

The four follow-on English proposals were shown before this GUI batch. Advanced
group details opens a bounded private editor from actual native source. Native
version/member/artifact fences and the exact reviewed preview remain authoritative.
Each edit invalidates review; delayed results cannot restore it. Review shows
keep/configure/replace/add/remove, protected datasets and the normalized next plan.
Apply submits that exact preview, separately from Start. The common owner rejects
active workers/data ownership even after a display state changes.

Stop group is offered when active. Reload and switching groups require confirmation
before discarding a changed draft. Back retains the ViewModel draft while revoking
review; Discard clears it. Source never enters saved-state bundles, logs or
diagnostic `toString`; it is intentionally lost on process death. An admitted
apply stays with the common owner when the editor closes. A durable success ACK
clears private source and prevents duplicate apply even after a UI connection
change. Stale publication retains the draft and requires reload/review.

## Group details

Show current member identities and actual created/running/exited/dead states,
per-member changing-state/error information, current published web targets and
existing logs/data/terminal child destinations. READY is not a health check.
Starting a member includes dependencies through the common controller. Dead retry
uses its stack-aware new-identity path, with membership refreshed afterward.

Provide Start group and Stop group, with operation progress and an explicit
capacity/start/rollback error. Keep group progress distinct from actual individual
states; UI must refresh even after partial failure. Individual stop leaves peers
under their existing generations. Explain that stop preserves data. Do not invent
an atomic restart or unconditional success from queued operations.

Remove definition is available only after inactive members and no worker/data
ownership. Confirm the exact group; remove only definition/member metadata and
preserve images and every dataset. Data deletion remains the separate existing
scoped workflow. Operation ownership stays in the foreground engine if UI closes.

## Acceptance still pending

Current group summaries page in batches of 100. Actual persisted membership hides
managed services from standalone removal, and group details refresh small metadata
without copying the creation plan. UI refreshes serialize to avoid older refreshes
overwriting newer operation results. Start/stop use separate operation keys so
Stop group remains available while startup is pending. Known explicit startup
cancellation is a notice; other failures remain errors and trigger state refresh.

No automatic database health/readiness, definition edit/reconciliation or
missing-package download has been added. Imported generic TCP publications do not
automatically become web launch targets; explicit web recipe metadata is still
required by the existing browser flow. A future advanced port/web selection needs
its own scheme and service/port-reuse acceptance.

Empty/loading/disconnected/error and partial states; SAF permission/read failure,
oversize/non-UTF-8/unsupported/missing package input; stale preview and duplicate
identity; creation without execution; capacity gate and partial rollback; single
member stop/retry and membership refresh; definition removal/data preservation;
rotation/back/navigation, TalkBack, large text and both themes/languages. Local
compilation and a conversation mockup are not physical visual or runtime evidence.
