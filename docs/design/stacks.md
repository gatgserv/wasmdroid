# Compose subset and durable stack definitions

Definition/storage, private routing, dependency startup, reverse stop/failure
rollback and the initial native GUI are implemented locally, with actual runtime
acceptance pending. Individual managed-member start uses the common stack
controller and its scoped network authority. Versioned stopped-definition
reconciliation now has a backend; its edit GUI still requires a new English scheme.

## Source contract

The approved plan selects a valid Compose subset for Wasm packages. The parser
uses the primary [container specification](https://compose-spec.github.io/compose-spec/container_spec.html),
[services specification](https://compose-spec.github.io/compose-spec/05-services.html),
[workload specification](https://compose-spec.github.io/compose-spec/workload_spec.html)
and [named volumes](https://compose-spec.github.io/compose-spec/07-volumes.html).
It is intentionally not a Docker/OCI executor or a complete Compose implementation.

```yaml
services:
  database:
    image: wasmer.io/example/database@1.0.0
    command: [database, '--data', /data]
    volumes: ['data:/data']
  web:
    image: wasmer.io/example/web@1.0.0
    depends_on: [database]
    environment: {DATABASE_HOST: database}
    ports: ['127.0.0.1:8080:80/tcp']
volumes:
  data: {}
```

The example is a syntax illustration, not an installable recipe or Android
evidence. The real curated catalog remains empty. Planning requires already
installed public wasmer.io WebC artifacts or exact imported
`local.wasmdroid/sha256/<digest>` WebC/core-Wasm artifacts; it never downloads an image.

Accepted attributes:

| Position | Supported values |
| --- | --- |
| Top level | services, volumes, optional name/version compatibility fields; caller's explicit stack identity determines scope |
| Service | image, command, environment, ports, volumes, depends_on, working_dir, tty, stdin_open |
| Command | argv list or shell-word-tokenized string; absent/null selects the package entrypoint or sole installed command |
| Environment | explicit mapping with string/number values or KEY=value list; booleans must be quoted |
| Ports | TCP short syntax, bounded matching ranges, long target/published/host_ip/protocol form; host publication remains IPv4 localhost |
| Mounts | declared named volume with short :ro/:rw or long type/source/target/read_only syntax |
| Top-level volumes | empty scoped definition, or explicitly existing external volume with optional name |
| Dependencies | list, or long service_started with required=true/restart=false |

Command strings are tokenized without executing a host shell. The first argv
element must select an installed WebC command. Empty overrides and ambiguous
package entrypoints fail explicitly. There is no automatic host environment or
shell substitution: $$ becomes a literal dollar; unescaped variable interpolation
is rejected. Values are sent only as explicit guest arguments/environment.
Unknown attributes, host/bind/tmpfs/anonymous mounts, unscoped custom volume names,
UDP/LAN publication, health/completion/optional/restarting dependency conditions,
remote/OCI execution and unsupported resource/restart authority fail before
datasets or member rows are created. Dependencies mean started, not healthy;
health readiness will not be fabricated by a successful native worker bind.

## Bounded parsing and planning

The low-level pinned saphyr-parser 0.3.4 emits YAML 1.2 events; WasmDroid constructs
only its bounded node representation. JSON documents use the same path. Input is
limited to 128 KiB UTF-8, 8192 events, 16 levels and 64 KiB per scalar. Multiple
documents, aliases/anchors, tags, merge keys, duplicate mapping keys, NUL and
non-string keys are rejected. Quoted strings stay distinct from plain typed
scalars. Scanner errors expose only line/column, never parser details that may
echo private input. shell-words 1.1.1 handles command tokenization.

Current bounds match the eight-worker implementation: 1–8 services, at most 32
volume definitions, 32 mounts/128 environment entries/16 published ports per
service. Logical names use bounded ASCII; service aliases cannot ambiguously
collide by case or override localhost/numeric addressing. Stable generated member
and scoped-volume identities use a stack identity plus 128 bits of SHA-256 over
the logical name. Hashing names is identity construction, not authorization.

Plans retain the installed digest, selected command, explicit specs, declared
volumes and a deterministic topological order. Missing dependencies/cycles and
conflicting fixed host ports fail before publication. A deserialized plan is
revalidated, including ownership, budgets, installed artifact digest/kind/driver,
commands, volumes and unsupported authority. Normalized plan JSON is bounded to
96 KiB so initial records with current member specs fit the 256 KiB IPC response
budget. Larger inspect responses fail explicitly; metadata-only member and paged
summary APIs remain available without environment/argument copies.

Imported artifacts use the same local import graph/metadata lock as standalone
execution. Their optional plan metadata fingerprint is mandatory for imported
services and absent from legacy public-only plans. A constraining execution pin
is stored in the managed specification and checked again against the actual
worker artifact before graph/runtime construction. Descriptive labels grant no
authority; actual membership and network capabilities remain independently checked.
Reimported metadata requires a new explicit group review. The edit source keeps
actual user settings/restored mounts while refreshing this internal pin only in
the next reviewed plan. Public raw Wasm, arbitrary registries/OCI and host paths
remain unsupported. Native startup plan inspection also revalidates artifacts.

## Persistence and data safety

Database migration 3 adds stacks, stack_services and stack_volumes while preserving
existing records, events and instance-volume references. Stack/member rows and
their events publish in one SQLite transaction; no guest is started by creation.
The creation event's durable monotonic sequence identifies the stack incarnation,
so deleting/recreating a named definition cannot make a prior removal revision
valid again. Unknown future database versions remain rejected without downgrade.

The content-store installation lease pins artifacts against uninstall/GC while
preparing/publishing. Existing datasets are checked and leased before any new
dataset is created. A normal scoped dataset must have matching stack/logical
ownership labels; unrelated or unregistered data is never adopted. External
datasets must already exist and be explicitly named in the imported definition.
Leases prevent deletion or administrative replacement during the metadata commit.

Unexpected filesystem/SQL failure or interruption can leave newly prepared,
registered, labelled volumes. They are preserved for retry/recovery; no partial
stack/member rows or member-created events escape a failed publication transaction.
Never recursively delete possibly written data as a stack preparation rollback.
Reusing the same stack/logical volume after definition removal retains its bytes.

Both mounted and unused declared volumes remain referenced until stack removal.
Removal requires the expected stack incarnation and inactive member states; the
common controller also excludes pending workers and data-operation leases. It
removes member/definition metadata atomically and preserves images and all volume
bytes. Individual instance removal/retry cannot orphan managed membership.

The private initial plan retains the creation definition; the saved current plan
tracks later explicit updates. Current instance specs remain
authoritative after a separately approved data restore; the prior declared data
reference remains preserved until definition removal. Managed retry checks both
stack and instance versions, requires a dead inactive member, inserts a new
identity using its current spec, swaps membership and removes the old dead row
in one transaction. The creation definition retains its initial identities as
history; current membership APIs supply the new identities. Definition editing
now uses the versioned stopped-definition update below. Running reconciliation
and automatic hot replacement remain unsupported; applying a definition starts
no guest and preserves the explicit start/stop model.

## Versioned stopped-definition updates

Migration 4 retains the initial definition privately and adds protected retained
volume references. Existing schema-3 stacks/members/data are preserved; future
schema versions are refused. Ordinary summaries and events expose no private
definition or argument/environment copies.

The native edit-source API exports current actual member specs as a bounded JSON
Compose document, including versioned retry identities and separately restored
volume mounts. Restored datasets receive explicit external-volume declarations.
Dollar escaping round-trips literal guest values without host substitution. The
export passes through the same strict parser and compares actual normalized
specs; unsupported current settings fail instead of silently disappearing.

Update preview pins the stack revision, every current member identity/revision,
installed artifact digests, a normalized next definition and a revalidated
keep/configure/replace/add/remove change summary. Apply rechecks these after
storage admission and inside the metadata transaction. All members must be
Created, Exited or Dead and the common owner must additionally exclude live,
starting/stopping workers and data leases. Native preflight runs on IO and
supplies canonical member identities for actor reservations; caller labels do
not select owners. A captured connection factory retains the current engine
owner without holding the facade mutex across SQLite admission or preparation.

Apply updates specs, member/volume references, dependency order and private
definition in one transaction. Unchanged logical services retain actual IDs,
state, exit information and runtime history. Changed same-package specs increment
their instance revision; a no-op creates no revision/event. Added, reintroduced
or package-replaced members get new actual identities derived from a fresh
durable update sequence, preventing old controls from addressing a new incarnation.
Dead stays terminal; changing its same-package spec does not silently rearm it.

Every prior declared or actually mounted dataset becomes a protected recovery
reference until definition removal, including removed services, unused volumes
and separately restored data. Neither bytes nor images are deleted. New scoped
data uses the same no-clobber ownership/lease preparation as initial creation.
Failed SQL publication rolls back specs, membership, references, publication
events and definition history. Independently prepared labelled datasets and their
volume-creation audit events remain available for retry. A successful update
acknowledges durable metadata only and never starts/stops/downloads a service.
The actor holds group/data reservations until that acknowledgement and prevents
start/retry/removal/data work from overtaking it. Independent stacks remain usable.

## Common stack lifecycle

UniFFI exposes preview/create/inspect, stopped-update source/preview/apply,
paged summaries, small member summaries, current membership, managed retry and
removal. Private specs are returned only
by explicit inspection; events and ordinary lists exclude environment/arguments.
The common ServiceSupervisor guards runtime/data ownership before removal.

Starting a stack selects all members; starting one managed member selects its
transitive dependency closure. Current durable membership supplies identities,
never caller labels or the creation plan's historical IDs. The controller checks
states/data leases and reserves every new worker slot before binding any worker.
Existing ready members remain owned by their current generation. Dependency order
means native READY/service_started, not application health or database readiness.
Packages must handle their own readiness protocol; no health policy is invented.

Private topology covers active/reserved members of this stack, including other
already running members. New workers receive initial configs; ready workers apply
versioned configs through generation-scoped control IPC. Unbound reservations are
released without changing their durable created/exited state if startup fails.
Bound new workers are stopped and remain owned until observed Binder death and
terminal commit. Rollback never stops unrelated/pre-existing workers, unless an
explicit user stop targets one. Each worker retains its actual guest result/error;
the group future carries the startup failure. A service that naturally exits zero
after READY is not reported as a persistently running or healthy server.

Group stop cancels any startup, waits for its new-member cleanup, then awaits
members in reverse dependency order. It preserves datasets/images. Individual
stop revokes that member's routes and does not restart its peers. Starting it
again allocates a new generation; retry of a dead member uses the versioned new
identity operation above. The private transport reuses grants for unchanged
edges, so a third member's restart does not revoke surviving peers' flows.
See [private networking](private-network.md) for authentication and bounds.

## Pending acceptance and GUI

Host regression definitions cover normalization, malformed/unsupported/duplicate/
cyclic/host-expanding input, limits, transaction rollback, prepared data retention,
entrypoints/ranges/external datasets, membership, managed retry, grants and stale
versions. Driver definitions add actual Unix I/O authentication/ACK/revocation in
one host process; that does not prove Android process isolation.

StackDefinitionTest defines live public-package metadata plus native definition/
volume checks under the common owner. SupervisedStackTest defines actual two-worker
Python TCP service-name communication, a counter persisted in a named dataset,
independent database stop/restart, generation checks and group stop, with uniquely
owned cleanup. Neither test has executed. The TCP counter is not a curated database
recipe or working web application acceptance.

Capacity, startup rollback/stop races, cross-stack denial, stale/wrong grants,
third-peer stream preservation, hot descendants, UI closure and coordinator/
worker death still need independent and real-device evidence. Compilation is not
runtime, migration, storage, dependency, routing or product acceptance. GUI import/
preview/stack management schemes were shown before the local Compose flows were
implemented. Their visual/provider/accessibility acceptance remains pending;
definition editing/reconciliation and explicit missing-package download controls
are now implemented locally after the four-screen English proposal. Their
runtime/visual/accessibility acceptance remains open.
See [the group GUI contract](stack-ui.md).
