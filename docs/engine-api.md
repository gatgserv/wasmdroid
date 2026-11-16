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
The future network manager must reserve a socket, not merely query and release
an available port. Loopback must be enforced by the network implementation,
including outbound policy; it is not enforced by this contract alone.

Resources: `memory_max_bytes`, `cpu_shares`, `timeout_seconds` (positive if set).
Restart: `no`, `on_failure`, `always`. Named networks, remote transport, restart
supervision, stats, live logs and streaming are pending; do not claim support.

## Lifecycle and runtime boundary

States: `created`, `running`, `paused`, `restarting`, `exited`, `dead`.
Created instances start or become dead; running instances can pause, restart,
exit or become dead; paused instances resume, exit or become dead; restarting
instances run, exit or become dead; exited instances can start/restart or become
dead. Dead is terminal. Removal is separate from lifecycle transitions.

`RuntimeDriver` declares create/start/stop/kill/wait/attach_stdio/stats/remove and
reports capabilities. It is a blocking boundary: future async supervision must
dispatch off UI threads. Drivers must validate the entire spec before side effects
and return `Unsupported` for capabilities they cannot enforce. The built-in WASI
probe is not yet an implementation of this lifecycle contract.

Planned Engine operations follow `images.list/pull/remove`,
`containers.create/start/stop/kill/remove/inspect/logs/attach`, `volumes.*`,
`events.stream`. General runtime lifecycle, guest mounts and streaming remain
pending. The blocking UniFFI `EngineStorage` object now exposes actual named
volume create/list/remove, local instance listing and paged durable events as
versioned serde JSON. It owns an `EngineSession`; only the local endpoint is
supported. Catalog validation, fixed registry/Python/HTTP harnesses, finite
built-in WASI and storage/cancellation probes remain explicit development APIs.
The future Android service must retain this owner and dispatch calls off UI threads.

## Durable storage

SQLite migration 1 creates `artifacts`, `instances`, `volumes`, `port_bindings`,
and `events`, with runtime kind/driver columns. Migration 2 adds lifecycle
compare-and-swap versions, last-error data, referenced-volume constraints and a
volume-removal journal. Existing instance mount references are backfilled in the
same transaction; invalid legacy data rolls back the migration. Unknown future
schemas are rejected without downgrade.

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
power loss. These capabilities have not yet been adapted into a guest filesystem.

The content store uses `store/blobs/sha256/<hex>`. Ingestion streams to a temporary
file on the same filesystem, bounds bytes independently of Content-Length,
verifies SHA-256, syncs and atomically publishes. Reads reverify integrity.
Private app-owned roots are required. Manifests/index, reference counting,
garbage collection, concurrent install transactions and compiled-code cache are pending.
Never deserialize downloaded native compiled artifacts as trusted executable code.
