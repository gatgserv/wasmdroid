# Common supervisor and per-package service controllers

Owner clarification, 2026-11-16: a common instance supervisor (the conceptual
"process 0") launches an Apache service and a DB service. Each service controller
owns its initial execution and every worker/descendant launched for that package.
Stopping Apache must stop its whole execution group while DB remains available.
This is a systemd-style ownership model, not a requirement to run systemd or a
complete independent copy of the engine for each package.

```text
Common WasmDroid supervisor
  Apache controller -> initial execution + all descendant work
  DB controller     -> initial execution + all descendant work
```

Registry, verified package storage, durable metadata/events and volume management
remain common. Each start receives a fresh run generation. Membership belongs to
the service/run, not merely to a currently live parent's PID. Descendants retain
that ownership if their direct parent exits. Track creation as it happens; do not
reconstruct ownership by scanning process parent IDs at stop time.

## Stop contract

1. Fence the service generation against new descendant execution and restart.
2. Close its ingress and request graceful shutdown of its owned work.
3. After the bounded grace period, terminate the entire execution group.
4. Verify that owned execution has actually ended before publishing stopped state
   and releasing runtime/port/volume ownership. An exit status alone is insufficient.
5. Keep unrelated services and the common supervisor operational. Old exit/death
   notifications cannot change a newer run generation.

The current WASIX tasks are virtual processes/threads inside the Android host
process, not automatically a Linux child-process tree. The existing per-runtime
task counter is only a prototype of ownership. Killing a root PID alone would
not implement this contract. Experimental store interruption is currently
unreliable in combined physical-device regressions and must not be promoted to
the general runner.

## Execution mechanism to validate

The service controller and its execution boundary are separate concerns. Android
can host a service in another process using `android:process`; that worker can
contain all of a package service's WASIX work while common management stays in
the coordinator. This could supply a last-resort whole-service termination
boundary without duplicating the logical instance/registry/database. Extra process
memory, IPC, startup, ownership, crash recovery and background lifetime require
measurement and real-device tests before selection.

`bindIsolatedService` supports dynamically named isolated workers from API 29;
the project still targets API 28. Isolated workers have different permissions,
so access to verified blobs and explicitly selected volumes must be brokered.
They are not equivalent to same-UID named service processes. Do not silently
raise the minimum API or grant broad filesystem/network access to simplify this.
The local development implementation now has a common Android `EngineService`
and eight private same-UID worker slots for API 28 compatibility. Each controller
has a fresh generation and owns one worker containing all of its WASIX work.
Registry, database and volume management remain common. The engine lock is
duplicated over private Binder IPC and retained by the worker, preventing a new
coordinator from falsely recovering live work after coordinator death.

Workers link to coordinator Binder death and terminate their own process if the
owner disappears. Stop fences guest creation, closes ingress and requests guest
shutdown; after the grace period the controller requests termination through
that exact worker Binder endpoint. Workers kill only their own PID. Slots and
terminal metadata are released after observed Binder death, not after a stop
request or main guest exit. Normal exit uses a result acknowledgement before
process termination. A forced owner stop records an unknown guest exit code,
with a reason, instead of inventing zero. Dead remains terminal; explicit retry
atomically creates a new identity and preserves volume references.

This code has been integrated locally, not accepted through physical-device
verification yet. The user requested completing development before CI; no new
CI runs are triggered by these local changes. The fixed slot count is an
implementation capacity bound, not a claim of unlimited concurrent services.
Worker RSS/startup overhead and service/UI-closure acceptance remain pending.

The native facade now uses the worker-resident RuntimeDriver and private host
termination callbacks. Live RSS/CPU sampling and UI summaries are implemented
locally; actual overhead measurements and whole-group stop acceptance remain
pending. See [driver and resources](driver-and-resources.md).
The production worker also has a [private TCP namespace](private-network.md):
internal guest ports are independent of host publication, normal completion
allows bounded forwarding drain, and explicit stop revokes only its own sockets.
The common stack controller derives private peer authority from durable membership,
reserves all required slots before bind and starts dependencies before dependents.
It cancels newly added workers on startup failure while preserving existing ones;
an explicit stop of an existing dependency still reaches it during rollback.
Group failure does not fabricate individual guest errors. Reverse group stop
awaits each member's observed death and committed terminal state. See
[stack lifecycle](stacks.md) and the private transport contract.

Sources: Android's [service manifest](https://developer.android.com/guide/topics/manifest/service-element),
[process/thread model](https://developer.android.com/guide/components/processes-and-threads)
and [named isolated service API](https://developer.android.com/reference/android/content/Context#bindIsolatedService(android.content.Intent,int,java.lang.String,java.util.concurrent.Executor,android.content.ServiceConnection)).

## Acceptance

Run two real services under the common owner. Each must launch owned descendant
work. Stop the first normally and through escalation; verify that its full group
ends and ports close, the second still responds, and persistent volumes survive.
Cover a hot CPU descendant, an exited root with surviving descendants, launch
during stop, coordinator/worker death, stale run notifications, UI closure and
restart. Preserve the current Python sandbox, registry verification and volume
isolation/read-only checks. Build/lint or a mocked supervisor is not live evidence.
