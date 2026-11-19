# Data transfer, backup and cache maintenance

Local implementation; integrated regressions, real Android SAF and visual
acceptance were pending at this checkpoint. Regression cases are defined in
`engine/tests/data_management.rs`.

## Files and ownership

Document providers never choose ambient engine paths. Import/export operates
through an explicitly selected named-volume capability. All transfer calls run
off the UI thread and use chunks of at most 64 KiB. An administrative reservation
blocks start/retry/remove for the selected inactive instance. Its unique lease
token prevents an old/cancelled continuation releasing a newer reservation.
Native file/volume capabilities retain OS engine ownership independently of UI
and coordinator facade lifetime.

Imports take an exclusive volume lease, stage bytes in a private engine cache
that guests cannot access, verify a known source length if supplied, flush/sync
and atomically rename through the destination capability. Explicit confirmation
is required to replace an existing regular file. An incomplete/cancelled import
does not publish partial data. The same private filesystem is required for that
atomic rename. Unknown paths, symlink destinations and directory replacement
are rejected. On owner recovery, recognized abandoned private staging files can
be reclaimed; user volume contents and unknown cache names are preserved.

Exports also hold an exclusive volume lease for a consistent selected file.
Deleting data accepts an exact relative file or empty directory, never recursive
deletion from a document-provider path. Enabling advanced mode grants no new
filesystem capability. File selection and transfer-target state survive activity
recreation; providers may still fail or revoke a grant, which is reported as an
operation failure rather than successful transfer.

## Portable backup format

The current unit is the selected named data volume. A standard GNU tar.gz stream
contains `wasmdroid-backup.json` (format `wasmdroid.volume`, version 1) and regular
files/directories under `data/`. It contains data and a volume identity, not
registry credentials or a full engine database/configuration dump. Applications
with multiple volumes expose the selected data set explicitly.

The source volume is exclusively leased while captured. Compression streams to
a bounded pipe: queued payload capacity is 4 × 64 KiB = 256 KiB, excluding active
producer/consumer buffers and codec state. A full second backup copy is not
staged on the phone. EOF is accepted only after the producer reports successful
archive completion; producer failure is propagated to the document copy.

Restore first stages the compressed source in private storage. Its byte budget
comes from available app storage; the unpacked budget is reduced again after
upload. The UI reserves a 16 MiB margin; filesystem allocation/metadata errors
can still occur and are reported. These are stream limits, not a measured disk
space estimate or power-loss durability certification.

The bounded reader validates gzip optional-header lengths, gzip integrity, raw
tar checksums, a bounded manifest, supported file types, relative destinations,
duplicate paths, name/entry counts, decompressed byte limits and trailing data.
Links, device/special/sparse entries and unrecognized metadata extensions are
rejected. GNU long-name entries are bounded and handled without an unbounded
metadata allocation. No archive entry chooses a host path.

Restore creates a new volume with no-clobber semantics. Extraction failure never
modifies the previous volume. Cleanup targets only this newly created,
unreferenced target; interrupted or unremovable new volumes remain recoverable.
After success, an inactive instance's spec, mount references, version and event
change atomically to use the new volume. The old volume is retained. A stale
instance revision prevents attachment after conflicting changes.

Cancellation reaches codec/read loops independently of their object locks.
Native capabilities are closed before potentially slow document-provider streams.
An atomic commit may win a last-moment cancellation race; no partial file is
published. A failed/cancelled external document write can leave an incomplete
document in the selected provider; it is not reported as a completed backup and
the application does not delete an arbitrary external document automatically.

## Images and garbage collection

Image removal rejects references from any instance and from another installed
package's dependency metadata. Removal preserves volumes. Downloads/installers
retain a shared store lease through graph metadata publication; uninstall/GC
requires exclusive maintenance ownership. GC cannot delete a newly downloaded
dependency before its install transaction commits.

GC scans only the private SHA-256 blob directory, keeps all installed digests,
and removes unreferenced blobs and recognized abandoned download staging files.
Unknown filenames and nonregular entries are preserved. Reported reclaimed bytes
are logical file lengths, not a claim about physical filesystem free-space gain.
Installed dependency images remain independent records until explicitly removed.
