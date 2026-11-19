# Engine API v1 — contracts and durable storage

The Rust types under `engine/src/api.rs` are the initial wire contract (serde,
snake_case). API version is `1`. Only the driver crate may import `wasmer*`.
An endpoint is present from the beginning: `local`, displayed as “This phone”.
Remote endpoints and OCI execution are reserved, not implemented.

## Artifacts and instances

An artifact has `reference`, `digest`, `kind`, `driver`, `platform`, `commands`,
`size_bytes`, and `labels`. References include their registry host;
`wasmer.io/python/python@3.12.0` and `docker.io/library/nginx:1.27` coexist.
Kinds are `webc`, `wasm`, `oci_image`. Digests use canonical `sha256:` followed
by 64 lowercase hexadecimal characters. OCI support is a model reservation only.

`InstanceSpec` includes `artifact_ref`, `endpoint_id`, `command`, `args`, `env`,
`working_dir`, `mounts`, `ports`, `network`, `internet_access`, `resources`,
`restart_policy`, `labels`, `tty`, and `stdin_open`.
Only `artifact_ref` and `command` are required. Unknown fields are errors.
Defaults: local endpoint, loopback, no internet, no TTY/stdin, no restart.
Limits are optional in the wire format; a future supervisor must select safe
defaults and enforce them before accepting untrusted packages.

Mount sources are tagged as `{"type":"volume","source":"data"}` or
`{"type":"host_path","source":"..."}`. Targets and working directories are
normalized absolute POSIX paths. Duplicate targets, traversal, backslashes and
control characters are rejected. Process arguments/environment reject NUL and
environment names reject empty names or `=`.
Host path access requires a future explicit capability; accepting the wire type
does not authorize access. Named volumes are independent of artifacts.
Ports use `host_port`, `instance_port`, `protocol` (`tcp`/`udp`). Zero host port
requests automatic allocation. Instance port zero is invalid.
PortLease reserves and retains a real host socket; it does not probe then reopen
an available port. Production workers now use a private TCP loopback namespace
for self-connect and internal ports. Only explicit port bindings publish host
ingress. There is no ambient host egress. Private stack routes require actual
persisted membership and per-generation grants from the common controller;
standalone services receive no peer access. Internet/LAN grants and integrated
network acceptance remain pending. See
[private service networking](design/private-network.md).

Resources: `memory_max_bytes`, `cpu_shares`, `timeout_seconds` (positive if set).
Restart: `no`, `on_failure`, `always`. The current driver rejects memory/CPU
limits and automatic restart rather than accepting unenforced settings. Android
enforces configured runtime timeouts. Named networks, remote transport and
restart supervision remain future work; live logs, virtual TTY attachment and
worker-process stats now have local implementations.

## Lifecycle and runtime boundary

States: `created`, `running`, `paused`, `restarting`, `exited`, `dead`.
Created instances start or become dead; running instances can pause, restart,
exit or become dead; paused instances resume, exit or become dead; restarting
instances run, exit or become dead; exited instances can start/restart or become
dead. Dead is terminal. Removal is separate from lifecycle transitions.

`RuntimeDriver` declares create/start/stop/kill/wait/attach_stdio/stats/remove and
reports capabilities. Android dispatches blocking native execution off UI/Binder
threads. The worker-resident ServiceDriver implements this trait and is used by
ServiceRuntime for create/start/stop/kill/input/stats. Drivers validate the entire
spec before side effects and return Unsupported for capabilities they cannot
enforce. See [driver and resources](design/driver-and-resources.md). The built-in
WASI probe remains a separate finite development API.

Planned Engine operations follow `images.list/pull/remove`,
`containers.create/start/stop/kill/remove/inspect/logs/attach`, `volumes.*`,
`events.stream`. General lifecycle, guest mounts and streaming are implemented
locally with integrated acceptance pending. The blocking EngineStorage exposes named
volume create/list/remove, local instance listing and paged durable events as
versioned serde JSON. It owns an `EngineSession`; only the local endpoint is
supported. Catalog validation, fixed registry/Python/HTTP harnesses, finite
built-in WASI and storage/cancellation probes remain explicit development APIs.
The common Android service retains this owner and dispatches blocking calls off UI threads.

## Durable storage

SQLite migration 1 creates `artifacts`, `instances`, `volumes`, `port_bindings`,
and `events`, with runtime kind/driver columns. Migration 2 adds lifecycle
compare-and-swap versions, last-error data, referenced-volume constraints and a
volume-removal journal. Existing instance mount references are backfilled in the
same transaction; invalid legacy data rolls back the migration. Unknown future
schemas are rejected without downgrade.

Migration 3 adds stack definitions, service membership/order and declared volume
references. The stack creation event sequence is its monotonic incarnation
revision. Definition/member/event publication and metadata removal are atomic;
datasets remain independent. Reserved stack labels cannot forge standalone
membership, and direct removal/retry cannot orphan managed members. Existing
instance/volume records and events retain their history. Definition operations
are distinct from runtime stack start. Managed retry atomically replaces a dead
member under stack/instance version checks, preserves current volume specs and
records new membership without rewriting the creation definition. Private
WorkerNetwork configs carry transient grants only over trusted host IPC; they
are not part of InstanceSpec, database events or exported backups. See
[stack contracts](design/stacks.md).

Stack GUI creation submits the exact reviewed normalized plan through
create_stack_from_preview, bounded to 96 KiB. Full plan validation runs before
dataset preparation and again under the store lease; changed installed digests
are rejected rather than silently replanning to different code. The original
document-based API remains available for callers that do not present a separate
review step. Explicit stack_summary and small member APIs omit args/environment.

`StateDb` supports create/list/inspect/transition/remove instance records and paged
events. Every state mutation and its event commit atomically. Exit codes must
agree with state; stale exit callbacks cannot overwrite a newer run. Referenced
volumes cannot be deleted, including references from stopped instances. Instance
removal preserves named-volume bytes. These metadata operations alone do not
prove that a guest ran or stopped. Events exclude args, environment and mount paths.

`EngineSession` takes a real exclusive OS lock before recovery. Active runtime
owners and volume files retain the same lock. A second owner cannot falsely
reconcile a live engine. After the last owner leaves, reopening marks interrupted
running/paused/restarting records dead, records unknown exit rather than success,
clears stale port bindings and appends recovery events.

`VolumeManager` confines access to app-private named directories through filesystem
capabilities. Read-only leases reject mutations; open files retain their lease and
prevent removal. Relative traversal and symlink escape are denied. OS locks use
`fs2` because the pinned Rust standard-library locks are unsupported on Android.
Removal journals intent, renames data to private trash, commits metadata removal,
then reclaims bytes. Reopening restores interrupted pre-commit removal or finishes
committed cleanup. Ambiguous or unregistered data is preserved with an error.
This tests process-interruption recovery, not filesystem durability under sudden
power loss. The local driver adapts these capabilities into its guest filesystem; actual
mount/read-only acceptance remains pending.

The content store uses `store/blobs/sha256/<hex>`. Ingestion streams to a temporary
file on the same filesystem, bounds bytes independently of Content-Length,
verifies SHA-256, syncs and atomically publishes. Reads reverify integrity.
Private app-owned roots are required. Manifests/index remain pending. Shared installation leases, dependency-aware
image removal, recognized-file garbage collection and an authenticated compiled
module cache are implemented locally; integrated acceptance remains pending.
Never deserialize downloaded native compiled artifacts as trusted executable code.


## Controlled package installation

InstallControl is a single-operation capability with active/cancelled/publishing/
committed/failed status. Controlled pull/catalog methods share it with the registry
transport, accept explicit new-byte/optional total-CAS budgets and retain native
ownership. Cancel returns false after publication wins; it never undoes committed
metadata. Supervisor cancellation additionally requires its current operation key
and generation. Legacy install APIs use the same implementation. Budgets cover
logical CAS staging, not the whole app, disk reservation or process. See the
[installation contract](design/package-installation.md) for bounds, resolver
limitations, data preservation and pending acceptance.


Android package activity snapshots contain filtered per-generation progress and
bounded recent outcomes, not recipe/env or private retry closures. A live control
in cancelled state remains pending until actual native completion; an image commit
alone does not establish application readiness. Catalog app preparation now runs
in the common owner, uses pure validate_service_spec before creating datasets and
reports separate setup failure while retaining data. Retry/dismiss require the
matching retained generation. The pure spec preflight validates supported driver
fields without requiring installed artifacts, creating data/metadata or running
guests; it does not certify runtime compatibility or application health.
