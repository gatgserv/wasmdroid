# Worker driver and resource measurements

This is a local source implementation with integrated/device acceptance pending.
It does not supersede the historical combined shared-process stop failures.

## Execution boundary

`ServiceDriver` implements the runtime-neutral `RuntimeDriver`. The production
UniFFI `ServiceRuntime` uses its create/start/stop/kill/input/stats operations;
poll, terminal and resize share that same driver. This is a single-use facade
for one instance/run inside its private Android worker. Registry, installed
packages, durable lifecycle records, volume management and slot allocation
remain owned by the common coordinator.

Create validates the whole specification and the persisted instance before
native execution starts. A different instance ID cannot address this driver.
Failed starts cannot reuse the same facade. Capabilities report WebC/TTY support
and explicitly reject unsupported memory/CPU limits, pause, remote endpoints,
ambient host mounts and internet/LAN permissions. Restart/boot metadata is owned
by the common coordinator; the single-use facade never resurrects itself. See
[restart and boot](restart-and-boot.md) for local implementation, integration
hooks and pending acceptance.

The host supplies a private `WorkerControl` callback. Stop schedules its bounded
0–60 second grace deadline before attempting an execution lock, then requests a
guest signal. Kill invokes only this trusted callback, outside native locks.
The Android implementation targets `Process.myPid()` in the worker; Rust never
searches, stores or kills an arbitrary PID. The common controller independently
sends termination through the exact generation's Binder endpoint at its own
deadline. Its millisecond deadline remains authoritative; the native callback
uses a rounded-up seconds fallback. Startup without a ready facade retains the
direct worker-only termination path.

Native wait/poll means the guest root and tracked guest tasks have finished and
supplied a verified result. Native remove releases a completed execution only;
it does not remove durable records or volume bytes. The common wait-for-exit
still requires observed worker/Binder death and a committed terminal state before
releasing resources. Forced worker death does not invent a guest exit code.
Input and display queries retain independent bounded handles, so they do not
wait behind the execution mutex; kill and process stats also avoid that mutex.

## Measurement contract

`RuntimeStats` contains:

| Field | Meaning |
| --- | --- |
| `scope` | `worker_process`: the whole worker, including JVM, JIT/compiler, allocator and virtual guest tasks |
| `memory_bytes` | Current resident pages from `/proc/self/statm` × actual `_SC_PAGESIZE`, using checked arithmetic |
| `cpu_time_ns` | `CLOCK_PROCESS_CPUTIME_ID`: cumulative CPU of all worker threads since process birth |
| `sampled_at_monotonic_ns` | `CLOCK_MONOTONIC`: timestamp for same-generation sample intervals |

Reads of statm are bounded to 256 bytes and never query another process. Clock
errors, missing procfs or unsupported platforms return null fields. Zero remains
a valid measured value. The CPU and wall clocks are not sampled atomically;
interval percentages are approximate measurements. RSS includes shared pages,
so adding several worker RSS values can double-count shared memory. It is not
PSS, a process peak, virtual address space or guest linear-memory allocation.

The private statistics RPC uses the same run fencing and bounded control queue
as terminal operations, with a three-second read timeout. Late replies are
ignored. Completed worker reports include a final process sample; a forced
death may have no final sample. Statistics do not affect guest exit status.

## Resource summary

Shown in conversation before the card implementation:

```text
Apache                         Running
Service process: 142.6 MiB · CPU 37.2%
Includes JVM and Wasm compiler
[Open] [Stop]
```

Running cards sample about every two seconds while their Android lifecycle is
started. Hidden UI does not keep resource polling active. A new visible interval,
generation change or failed query clears the previous interval. The initial CPU
value is unavailable until there are two valid samples:

`CPU percent = 100 × (new_cpu_ns − old_cpu_ns) / (new_monotonic_ns − old_monotonic_ns)`

One busy logical CPU is 100%; multithreaded execution can exceed 100%. The view
does not clamp it to 100 or divide it by a guessed core count. MiB divides current
resident bytes by 1,048,576, displayed to one decimal using the user's locale.
Unknown measurements display unavailable, never a fabricated zero.

These measurements are observability, not enforcement. A Wasm linear-memory
limit is not an aggregate worker limit, and a sampled RSS watchdog cannot
guarantee a hard memory ceiling. CPU shares require actual scheduling support.
Both resource-limit fields remain explicit Unsupported until the selected
backend can enforce the approved contract. Wall-clock timeout enforcement is a
separate existing controller feature.

## Pending integrated acceptance

Compile and run the resource interval/null/reset regressions; inspect non-4 KiB
page arithmetic. On the selected arm64 device, verify real worker RSS/CPU,
generation fencing and no UI-background polling; exercise worker death, slot
reuse, stop during native lock contention and the independent coordinator
deadline. Prove the full two-service descendant cleanup and persistent-volume
contract. The extended live Python terminal acceptance defines capability,
stats and stale-generation assertions, but has not run for this implementation.
