# Explicit first-party catalog seed files

This backend supports small initial UTF-8 files for new private catalog stack
volumes. It does not introduce an accepted recipe, run a guest, fetch asset URLs,
change file permissions or grant access to existing datasets. The packaged
curated catalog remains empty. Complete recipe, license and Android evidence
are still required before a real entry can be published.

## Bounded recipe metadata

A schema-2 stack can supply an optional `assets` array of strict
`CatalogSeedFile` objects. Missing/empty arrays preserve earlier recipe JSON.

| Field | Meaning |
| --- | --- |
| `volume` | Declared non-external Compose logical volume, never an actual dataset name |
| `path` | Relative normalized destination within that volume |
| `text` | Inline UTF-8 content, with no NUL |
| `size_bytes` | Exact UTF-8 byte count, not character count |
| `digest` | Existing SHA-256 contract over the exact UTF-8 bytes |

There are at most 32 files and at most 64 KiB of content across the entire
recipe. The pure validator checks length and digest before filesystem access.
Unknown fields such as modes, host paths, download URLs or execution options
are rejected. There is no automatic asset downloader or executable permission
field. File extensions do not grant any runtime authority.

Paths reuse the existing capability-based data path validator and add portable
seed constraints: ASCII letters/digits/`._-` components, no empty/`.`/`..`
components, root/prefix/backslash/control/colon forms, trailing dots or Windows
device aliases. Paths are at most 1024 bytes, 16 components and 255 bytes per
component. Duplicate paths, file-versus-directory prefix conflicts, case aliases
and inconsistent casing of shared parent directories are rejected before writes.
These limits avoid two signed destinations selecting one filesystem entry.

`manifest_digest` hashes a domain-prefixed canonical serialization of all seed
fields and text, sorted by logical volume/path. Declaration order and incoming
JSON whitespace do not change the receipt. Changing content, destination, length
or digest does. Only the digest is stored in the private volume receipt label;
seed content and private paths are absent from Debug and error diagnostics.

## Admission and publication

The native stack preparer validates the whole recipe, verifies every asset
volume is newly scoped/non-external, and records the exact immutable seed
manifest when creating that volume. Seeding an existing volume requires the
matching private stack/logical-volume/seed receipt and an unpublished stack.
An ordinary user-created volume or an earlier prepared volume without the exact
receipt is rejected; its data is preserved. The common owner retains exclusive
writable leases through seeding and final membership publication. Shared and
read-only capabilities cannot seed.

`catalog_assets::seed` receives one logical volume's validated file subset and
the existing private transfer-cache capability. It inspects the entire target
tree before writing. Only expected real parent directories and completed target
files with exact matching length/digest may already exist. Unknown entries,
symlinks, directories at file targets, different bytes and incomplete files cause
a conflict without changing existing data. Parent and final components are
checked with symlink metadata; the volume root itself was checked at lease
admission. No ambient path comes from recipe text.

New files are written to cryptographically random `wasmdroid-cache-*` names with
exclusive creation in the private, unmounted staging capability. The completed
file is flushed and synced, then atomically hard-linked to the final destination.
Hard-link publication never replaces an existing target, including a target
created after preflight. There is no truncation/copy/overwrite fallback. Filesystems
without the required primitive fail explicitly. The staging guard removes only
the unique file it actually created. Existing cold transfer-cache recovery can
clean abandoned known-prefix regular staging files; it does not clean user data.

On Unix/Android each newly created parent entry and each published file's parent
directory is synced, including nested directories. The volume root is synced
before success. Windows checks content and no-replace behavior but does not
claim the same directory-flush/power-loss durability guarantee. Android behavior
still needs actual device acceptance.

This is atomic per file, not a transaction over all seed bytes. An I/O error or
later SQL failure preserves completed files and any new parent directories.
The stack is not published until preparation succeeds. An explicit retry under
the same receipt verifies and skips exact completed files, then creates missing
ones. Changed seed manifests, unknown data or edited files fail closed; recovery
never deletes or overwrites them to make the operation succeed. A matching retry
does not rewrite a user's already edited file, even if its length is unchanged.

## Example and remaining acceptance

`examples/catalog-seeds/notes/` contains an original Apache-2.0 static HTML notes
page and an exact inline asset manifest. It has no third-party library or guessed
package/command. It uses browser localStorage, not a guest database or dataset
write API. Address/port changes still affect origin storage visibility, and a
browser-data clear removes saved notes. The page requests text export but does
not claim the current WebView accepts it. Its page CSP does not establish a
general browser network isolation guarantee.

`engine/tests/catalog_assets.rs` defines twelve cross-platform hermetic cases
and one additional Unix symlink case. They cover integrity/limits, paths,
collisions, immutable receipts, complete-file publication, same-receipt retry,
partial recovery, preservation of differing/unknown data, lease admission and
the exact example bytes. The Unix case verifies parent and target symlinks do
not modify outside data. At this writing milestone root-coordinated compilation
and execution are pending. Root separately covers real receipt/new-volume/
publication integration; helper-only cases cannot establish those gates.

Real package command/mount/port compatibility, combined seeded HTTP behavior,
device/provider storage, interruption and no-overwrite recovery, source/license
provenance and catalog publication remain separate acceptance requirements.
This asset helper itself adds no GUI. The coordinated batch also integrates
normal combined-app cards and shortcuts after their separate English proposal.
Remote CI, push, signing and deployment remain deferred.
