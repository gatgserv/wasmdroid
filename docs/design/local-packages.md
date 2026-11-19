# Local package import

Tracked by [issue #6](https://github.com/gatgserv/wasmdroid/issues/6).
This is local source implementation with compilation evidence only. Native SAF
and product import screens now follow the conversation scheme presented before
implementation. Runtime, provider, visual and independent acceptance are pending.

The neutral LocalImport owns a streamed private file, an engine ownership clone,
a shared maintenance lease and a single-use OperationControl. The UniFFI
LocalPackageTransfer accepts bounded byte chunks; it never accepts a host path,
provider URI or caller-selected executable fd. Android reads an explicit SAF
content selection on IO threads. The common service owner retains the generation,
input, native capability and operation through actual completion/cancellation;
ViewModel loss is not a native completion receipt. A fresh exclusive engine owner recovers only recognized
abandoned regular staging files. Connections never run recovery.

Preview seals input, checks exact declared/actual length and calls the Wasmer-only
inspector. Raw core Wasm requires a valid wasm32 module with an exported
`_start: () -> ()`; its only command is `main`. WebC v3 inspection checks framed
index spans, bounded CBOR structure before serde, a bounded atom directory,
actual atom hashes, core modules, command runner/atom bindings and dependencies.
Inspection does not compile or execute code. Component model, legacy WebC v1/v2,
wasm64, GC types, non-WASI runners, file/URL dependencies and unresolved named
tags are unsupported in this first local importer. Format validity is not a
promise that every module links with the installed WASIX runtime.

Metadata bounds are 4 MiB per index/manifest, nesting 32 and 65,536 CBOR items;
at most 256 commands/atoms/volumes, 128-byte command/atom names and 32 direct
dependency requirements. Core validation bounds allocating metadata sections,
types, functions, imports/exports, memories, locals and per-function body bytes.
Cancellation is checked between sections/functions; a single format-reader,
hashing, filesystem or function-validation call is not preemptively interrupted.
These parser/work bounds are not whole-process memory or CPU limits.

The Compose subset now also accepts already imported exact-digest WebC and core
Wasm identities. Planning reuses the installed-only dependency lock and pins local
artifact metadata as well as its bytes. A worker checks that pin against its actual
cloned artifact before loading the graph, closing the review-to-start race.
No local path, URL fetch or automatic dependency install is added. Group metadata,
reviewed updates and lossless source export share existing membership/data fences;
a successfully imported image is still not an accepted runnable-stack receipt.

The identity is `local.wasmdroid/sha256/<input SHA-256 hex>`. Filename and manifest
publisher names do not give registry identity or publisher verification. Named
semver or hash dependencies resolve only from installed verified WebC images,
with a bounded transitive closure. Preview exposes missing requirements without
network access or downloading anything. Install missing dependencies explicitly,
then review again. Pins retain reference, content digest and a fingerprint of
the installed artifact metadata. Commit and every worker start recheck pins.
Dependency metadata changes fail closed. Root dependency references protect the
entire reviewed closure from ordinary removal/GC. Reimporting identical input
with another lock fails instead of silently changing existing services.

Commit requires the current preview token and zero missing requirements. Under
the common CAS writer admission it verifies/reuses or ingests the original bytes,
then takes the single publication/cancellation latch before durable image
metadata. Failure may retain a verified CAS blob but publishes no fake image
success. Import creates no volume, instance or guest and does not auto-start.
New previews can replace an earlier dependency review; every commit rechecks it.

Input is limited to 512 MiB and chunks to 64 KiB. The explicit operation new-byte
budget covers input staging plus an additional full CAS copy unless the CAS blob
already verifies. The optional total-CAS staging ceiling uses the existing store
admission rules; private input staging is outside that CAS ceiling but inside
the operation new-byte budget. This does not reserve physical free space across
all operations, invent a total-cache product default or automatically evict data.
Write errors close the capability because write_all can have written a prefix.

The advanced Store offers SAF local selection without persisted URI grants.
MIME and display name are descriptive; the native parser decides the format and
identity. Optional provider length is checked against actual streamed bytes.
The picker accepts provider types broadly because package MIME availability
varies; only content URIs are opened. Provider input uses CancellationSignal and
a closeable descriptor, with cancellation/close dispatched away from the actor.
A provider read/cancel call can remain blocked; the UI stays cancelling and the
common owner waits for actual IO/native cleanup rather than reporting completion.

The common operation list reopens active/review/outcome details by exact key and
generation. Review keeps no executor thread waiting for human input and does not
block explicit dependency installation in Store. Missing named dependencies have
search actions; no dependency download is automatic. Recheck displays the new
native preview before commit. Recoverable pre-publication failures clear the
displayed preview and require another review; failed/cancelled inputs require a
new file selection. Installed imports can open the existing application form with
an explicitly selected verified command. Creation still starts no guest.

URIs, provider exception messages and native capabilities stay out of progress,
receipts, saved screen state and application specs. The owner retains at most
16 active package operations and 16 local outcomes. Native preview frames are
bounded to 128 KiB and render through a bounded parser and lazy command selector;
dependency names/ranges have 256-byte limits. English status and error strings use
the native UI. Dismissing a dialog hides it; explicit cancellation releases its
input after actual work. Service shutdown also cancels and awaits imports. This
does not add an Android background/foreground-resurrection guarantee.

Failed native capabilities now explicitly release staging/leases before engine
ownership on drop. Failed cleanup uses drop rather than calling abort, preserving
failure instead of manufacturing a cancellation. The owner reports cancellation
only when the shared native control accepted it and cleanup actually completed.

The service driver advertises both WebC and Wasm. Raw Wasm is adapted into a
BinaryPackage with an actual module hash and no host directories or packages.
Both kinds use the existing ServiceExecution, authenticated compiled cache,
WASIX tracked tasks, private network, explicit volume capabilities and stdio/TTY.
Android keeps the same per-package worker/controller and observes OS/Binder
death before releasing a stopped group. Import adds no alternate native-code
execution, host networking, parent-PID scan or systemd process.

Defined acceptance includes malformed/truncated/native/component files, wrong
atom hashes, bounded parser input, cancellation/publication, dependency review
changes, input/CAS budgets, owner lifetime/recovery and actual WebC/Wasm execution
through the common loader. Real SAF/provider/rotation, two active services with
descendants, observed selective worker death, HTTP/volume isolation and actual
GUI accessibility remain required. Compilation is not a passing test result.
