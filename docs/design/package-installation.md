# Cancellable package installation and CAS write admission

Local source implementation, tracked in issue #5. Tests and physical acceptance
remain pending. This extends verified registry/CAS installation; it does not
complete the full Android product, catalog hosting/trust or package import.

## Cancellation and publication

Each installation owns one OperationControl and a unique claim guard. States are
active, cancelled, publishing, committed and failed. Cancellation and entry into
metadata publication race through one atomic compare/exchange. A cancellation
accepted while active prevents graph publication. Once publishing wins, cancel
returns false; SQLite either commits the entire image graph or reports failure.
No already committed install is described as cancelled or removed as compensation.
The Installed progress event occurs only after commit. Failed/pre-cancelled/reused
tokens cannot start a second operation, and a rejected second claim cannot
finalize the current owner's state.

Cancellation checks cover resolution boundaries, dependency traversal, cache
verification chunks, streaming reads/writes, final verification and staging
publication. Temporary files remain RAII-owned and are removed on failure or
cancellation. Verified blobs already published in the CAS remain available for
retry and ordinary leased GC; image metadata is not partially installed.

The production WasmerRegistry and Installer share the same control. Registry
requests and body reads run as async reqwest futures selected against cancellation,
behind the synchronous neutral Registry facade. The process uses one shared Tokio
runtime with two async workers and at most two blocking resolver workers, rather
than detached blocking HTTP threads per request. TLS verification, HTTPS-only
requests, existing redirect/public-URL checks, 15-second connect/180-second total
request timeouts and temporary-URL redaction remain in place. Metadata is bounded
to 4 MiB, and download bodies are bounded by the declared size plus CAS validation.

OS getaddrinfo cannot be forcibly interrupted safely. A resolver job retains an
admission slot after its HTTP caller is cancelled; at most four jobs can be queued
or running, with at most 64 returned addresses used per resolution. Residual OS
resolver work retains no engine ownership/CAS writer capability. This is bounded
resolver admission, not proof that every operating-system lookup terminates on
cancel. Arbitrary third-party blocking Registry implementations must cooperate;
passing an unrelated control to their transport cannot make them interruptible.

## Atomic installation of multiple pinned roots

`Installer::pull_many_expected_in_operation` accepts one through eight
`(ArtifactReference, Digest)` roots and an already claimed `OperationGuard`.
The caller retains that guard until the entire native operation finishes and
must give its registry transport the same cancellation control. The API rejects
an empty or oversized request before registry access. Repeated requested or
resolved canonical references are rejected even when their pins match; different
pins for one reference are a conflict. It does not silently select one pin.

Every requested root is resolved and checked against its exact requested
reference and expected digest before any download starts. A later root's missing
version, substituted reference or wrong digest therefore cannot download an
earlier valid root. This strict identity check is specific to the new pinned-root
API; legacy single-root mutable-reference resolution remains supported. The root
pins also constrain dependencies: dependency resolution cannot replace a pinned
root with another digest. Ordinary dependency/reference digest conflicts and
cycles retain the existing explicit failure behavior. This is not an atomic
registry snapshot; registry changes still fail against the checked digests.

All roots then share one `Work`, maintenance lease, `StoreWriter`, new-byte and
optional total-store budget. Dependency graphs are deduplicated by resolved
reference, while distinct references with identical verified CAS bytes reuse the
same blob. Existing package-count, depth, blob-size and total graph-size limits
apply across the union, rather than resetting per root. Cache hits consume no
new-byte budget; successfully staged bytes accumulate across every root. The
store cap continues to include existing files and staging peak. These are the
existing CAS policies, not new resource-enforcement limits.

There is exactly one publication admission, `StateDb::put_artifacts` transaction
and committed transition for the complete union. Cancellation before publication
prevents all new image metadata; an SQLite failure rolls the transaction back.
Previously installed metadata and verified CAS blobs are preserved. Earlier
verified blobs from the failed/cancelled batch remain reusable, with no
compensating deletion or partial-installed success. Once publication wins,
cancellation is refused and the transaction's actual result determines success.

Results preserve the caller's root order. Each `InstalledGraph.root` is that
requested resolved root; each `images` list contains the same complete union in
resolved-reference order. Consumers must not interpret the union as the root's
direct dependencies; those remain in the existing dependency-images label.
One Installed progress event is emitted per requested root, in request order,
only after the entire metadata transaction commits. All earlier progress remains
per image. A committed/publishing/cancelled control cannot start another install,
including reuse of the original still-borrowed guard. Legacy single-root APIs
delegate to the same one-root union path and still return only their graph.

`engine/tests/multi_root_install.rs` defines hermetic fake-registry regressions
for a bad/missing later root before download, root order and shared-dependency
deduplication under an exact combined budget, verified zero-budget retries,
accumulated new-byte/store/graph/package limits, cancellation after the first
verified root, SQL-trigger failure on the later image, duplicate/canonical/pin
conflicts, same-digest reference substitution, cancellation before resolution,
committed-guard reuse and the eight
root boundary/legacy single-root behavior. These definitions are not executed
evidence until the coordinated verification record states their actual result.

## Write budgets and maintenance

All ContentStore ingest paths share an exclusive writes.lock admission, separate
from the existing shared installation/GC lease. One writer owns graph staging
through metadata publication; competing writers get an explicit conflict instead
of silently evicting packages. Runtime reads/verification do not acquire this
writer lock. GC/uninstall remain excluded by the maintenance lease.

InstallBudget is caller supplied:

| Field | Meaning |
| --- | --- |
| max_new_bytes | Cumulative successfully staged bytes for this operation, including replacements; verified cache hits consume zero |
| max_store_bytes | Optional maximum logical regular-file bytes under blobs/sha256 during this operation, including staging peak |

Admission runs before the blob request and again before ingest. Replacement must
fit existing CAS bytes plus the full incoming temporary file, even if the corrupt
destination will later be replaced. Unknown regular files and abandoned staging
files count toward the selected total; they are not automatically deleted to fit.
Non-file entries or an inventory beyond 65,536 entries fail explicitly. There is
no directory traversal or symlink adoption in quota inventory. A zero new-byte
budget still permits verified cache reuse, even if an optional cap is now below
existing usage. It grants no additional writes.

Android uses current usable app-filesystem space minus the existing 16 MiB transfer
reserve as its per-operation new-byte budget, while retaining the graph's 1 GiB
and blob's 512 MiB limits. The optional total CAS cap has no invented production
default; it is an explicit caller setting. This is not a persistent global policy,
physical filesystem reservation, total-app/compiled-cache/volume limit or process
memory cap. Concurrent non-CAS disk usage can still cause ENOSPC; failed staging
and metadata transactions must preserve existing data.

## Platform ownership and GUI

UniFFI InstallControl exposes cancel/status without the engine storage mutex.
Controlled registry and curated-install APIs claim before parsing/resolution and
retain engine ownership until native work finishes. Curated installs verify the
catalog's expected digest on the exact resolved artifact before any download.
Legacy entry points delegate through the same controlled implementation.

The common Android supervisor tracks package-operation keys and random generations,
routes existing pull/catalog calls through the controlled API and rejects duplicate
keys. Cancellation requires both key and generation, so a stale UI request cannot
cancel a later install of the same package. Owner close requests cancellation and
waits for actual native completion before releasing storage/executors. A publishing
operation finishes truthfully even if close races it. Running service controllers
remain separate. The normal two-thread disk executor still bounds platform calls.

Native progress/cancel/retry/dismiss rows were added after the conversation scheme.
The supervisor retains filtered per-generation progress and 16 recent terminal
receipts, accepts at most 16 requests and bounds retained recipe input to 128 KiB.
Snapshots exclude recipe/env and private retry closures. Live cancelled controls
remain pending until native completion; image commit is distinct from application
preparation. Retrying/dismissing checks the exact generation.

Catalog application preparation is owned by the supervisor rather than a ViewModel
continuation. Pure native runtime preflight precedes dataset preparation; setup
failure keeps committed images and prepared data and never fabricates a ready app.
No total-cache settings or persistent quota policy was invented. See the
[GUI contract](package-install-ui.md). Physical foreground/background behavior,
rotation/network conditions and service independence still require acceptance.
This is not an Android background scheduling or process-survival guarantee.

## Pending evidence

Neutral definitions cover pre-cancel, last-verification cancellation, post-commit
refusal, token/claim ownership, concurrent cancellation/publication, exact new-byte
budget/cache reuse, staging peak, unknown-file preservation, competing writers,
runtime reads and staging-read cancellation. A transport unit defines cancellation
of a real local TLS exchange that intentionally never finishes its handshake,
without disabling certificate checks or production URL policy. These definitions
are not executed evidence.

ControlledInstallTest defines a fresh, uniquely owned cache root and an actual
public Python registry pull, cancellation after nonzero body progress, no partial
image metadata/staging leftovers and cleanup only of its own fixture after owner
release. It does not execute services or use shared installed application data.
Physical headers/body/DNS/error/late-cancel/background/other-service and quota
acceptance, independent regressions and final CI remain pending.
## Required environment settings

Wasmer Registry publishers can declare required environment variables for an
individual command using a WasmDroid annotation in `wasmer.toml`:

```toml
[command.annotations]
"dev.wasmdroid.required-env" = [
    { name = "SERVICE_PORT", kind = "port" },
    { name = "ACCESS_KEY", kind = "secret" },
]
```

The optional kind is `text` (the default), `secret` or `port`. The
declaration contains names and types only. Ordinary `wasi.env` entries
continue to supply defaults. A missing or malformed declaration does not
invent any requirements. Store reads valid declarations from the registry
when it resolves the installed artifact.

Users can mark a variable as **Required before start** for any service in
**Configure** → **Execution**. **Set up to start** opens the settings
whenever a required value is missing or invalid. Saving updates the service
configuration. Clearing a required value prompts again on the next attempt.
