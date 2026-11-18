# Trusted compiled module cache

Implemented locally; runtime/device/cache-corruption acceptance is pending the
integrated verification phase. Compilation alone does not prove a disk-cache hit
or a warm-start performance improvement.

Production service workers use a shared-memory cache backed by authenticated
private compiled bytes. The earlier finite Stage 0 probes and benchmark keep
their original in-memory behavior, so their historical timings remain unchanged.
Every offline container's declared atom hash is recomputed from its actual bytes
before constructing the package graph. An outer verified WebC digest alone does
not establish that a publisher supplied correct inner cache keys.

The disk identity binds the source module hash, exact dependency lockfile,
driver/engine source and manifests, rustc version, build profile/compiler flags,
target triple and CPU features, runtime Wasm features, compiler deterministic ID,
artifact format and OS page size. A changed identity causes recompilation. Changes
to future dynamic tunables must also change that identity before being enabled.

Wasmer deserialization accepts native executable code and is unsafe for arbitrary
downloaded input. Before calling it, the driver obtains an owned byte buffer from
the private cache and verifies HMAC-SHA256 over the full identity, byte length and
SHA-256 of those exact bytes. A cryptographically random per-install key stays
inside the engine directory. It is never exposed by FFI, mounted for guests,
included in data-volume backups or written to diagnostics. Downloaded serialized
native artifacts and document-imported native code are never cache inputs.

Entries are atomically published after flush/sync. OS maintenance locks serialize
publication/eviction with readers across service processes. A busy, missing,
corrupt, incompatible or unavailable cache falls back to source compilation;
cache failure never broadens guest permissions. Initialization failures leave
the default memory cache active. Invalid key material disables persistence rather
than trusting old entries. No power-loss durability certification is claimed.

The maximum entry is 256 MiB; the disposable cache is trimmed to 512 MiB and 512
entries, with oldest-use eviction and recognized abandoned private staging files
removed first. These are logical-byte limits, not physical allocation guarantees.
Unknown files, nonregular entries, installed WebC blobs and data volumes are kept.
Settings clears compiled bytes and unreferenced download blobs independently of
the running modules' owned buffers. The private authentication key is retained.

Service runtime information reports disk hits, misses, saves and failures without
paths, keys or source bytes. Completed worker output includes the final counters;
final phone acceptance must observe a disk hit across distinct worker lifetimes
and validate the actual guest result. Host trust-boundary regressions are defined
in `engine/tests/native_cache.rs` and are not yet recorded as passing.
