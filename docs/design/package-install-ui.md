# Package cancellation UI proposal

Presented in the conversation before Compose changes. Local PackageOperationsPane
and the supervisor-owned package activity now implement these flows, with device/
visual/independent acceptance pending. Keep
the existing native Material Store/My Apps/Settings navigation and English-only
light/dark presentation. Illustrative Python progress is not a measured
download, Android compatibility claim or curated catalog entry.

Use one progress row per actual package operation, with its current package name,
phase and bytes for the current artifact. Concurrent package/data operations must
not overwrite a shared progress value or disappear when a different operation
finishes. Recreating the UI reconnects to current supervisor operation generations;
never cancel by reference alone or attach an old progress stream to a new run.

While resolving/downloading/verifying and active, offer Cancel. After an accepted
request, disable repeat input, show Cancelling, and await native completion before
claiming cancellation or removing the operation. The existing verified cache and
installed packages remain available. Retry creates a fresh control/generation.

Once metadata publication wins, disable cancellation and show Finalizing. If a
late click returns false, read current state/result; do not claim cancellation or
assume every false result means publication. Show installed only after the actual
operation succeeds. Catalog recipe/app preparation after image commit remains a
separate uncancellable preparation phase, with its own truthful failure result;
no package cancellation should undo an already committed image or delete datasets.

Quota failure explains insufficient space and preserves existing data. Competing
writer/duplicate request errors explain the active operation. Do not silently
auto-evict or invent a total-cache production limit. Advanced explicit cache cap
settings need a separate product/storage scheme; this proposal adds cancellation,
not a guessed persistent quota policy.

Acceptance pending: stale generation/late click, actual request/body/verification
cancel, commit race, multiple progress streams, queue/error/retry, rotation/rebind,
process death, background restrictions, other running services, TalkBack/large
text/themes/languages and actual registry/device behavior. The mockup and source
compilation do not prove any of these.


## Local implementation and ownership

The common supervisor retains each current generation's filtered progress and the
last 16 terminal receipts for its lifetime. Snapshots omit recipe/env data and
private retry closures. At most 16 operations are admitted; recipe input is bounded
to 128 KiB before retaining closures. Retrying uses the exact retained operation
and creates a fresh generation, while retry/dismiss require the matching prior
generation. Receipts are not durable across owner/process recreation.

The UI polls small owner snapshots independently of image/instance refresh. Active
and recent rows are parsed separately: a live cancelled control means Cancelling,
not a completed result. Image commit/progress Installed does not claim a catalog
application is ready. Old receipts cannot replace a new active generation; package
progress never mutates the separate data-transfer progress value. Snapshot refresh
is serialized and rejects a replaced connection. Pending registration offers no
cancel capability before an actual generation is known. Native row scrolling is
bounded so retained outcomes do not consume the whole destination.

Catalog recipe preparation moved from ViewModel continuations into the common
supervisor. Runtime spec preflight runs before installation/preparing datasets;
rewritten final specs are checked again before dataset creation. After native
image commit, application preparation is uncancellable, creates independent named
volumes and a created instance, and never starts guests. Failure preserves the
committed image and prepared datasets and gets a distinct setup-failed outcome.
This continues while its current owner is alive, not through arbitrary Android
process termination/background scheduling. Owner close waits for tracked native
work/preparation; history is discarded only when owner release is safe.

New presentation/preflight/common-owner generation regression sources are compiled
without execution. They do not prove actual GUI rendering, background survival,
service execution, provider behavior or curated catalog recipe compatibility.
