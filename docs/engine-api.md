# Engine API v1 — foundation

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
`{"type":"host_path","source":"..."}`. Targets are guest absolute paths.
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
`events.stream`. They remain unimplemented; no successful placeholder operations
are exposed. The current UniFFI facade exports typed `EngineInfo`, catalog
validation over the versioned JSON contract, and a finite built-in WASI probe.

## Durable storage

SQLite migration 1 creates `artifacts`, `instances`, `volumes`, `port_bindings`,
and `events`, with runtime kind/driver columns. Schema migration is transactional;
unknown future schemas are rejected without downgrade. Installed artifact
metadata can be saved/reloaded. Lifecycle persistence and event operations are pending.

The content store uses `store/blobs/sha256/<hex>`. Ingestion streams to a temporary
file on the same filesystem, bounds bytes independently of Content-Length,
verifies SHA-256, syncs and atomically publishes. Reads reverify integrity.
Private app-owned roots are required. Manifests/index, reference counting,
garbage collection, concurrent install transactions and compiled-code cache are pending.
Never deserialize downloaded native compiled artifacts as trusted executable code.
