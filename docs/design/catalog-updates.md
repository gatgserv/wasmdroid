# Opt-in signed curated catalog updates

Local source implementation for issues #1 and #5. This is a metadata capability,
not an accepted package inventory, release trust decision, scheduled updater or
completed Android flow. The packaged `catalog/catalog.v1.json` remains empty.
The sixteen neutral catalog regressions passed in the previous integrated local
batch; current GUI definitions and rendered/device acceptance remain separate.

## Explicit trust and wire contracts

`CatalogUpdateConfig` is supplied by an owner-approved distribution configuration.
It has `channel`, a stable public credential-free HTTPS `index_url`, one to sixteen
`trusted_keys` (`id`, standard-base64 raw 32-byte Ed25519 `public_key`) and a
`minimum_sequence`. Empty keys return Unsupported. There is no production host,
default signing key, downloaded trust root, trust-on-first-use, account, private
registry or credential flow. Metadata URLs cannot have queries, fragments or
embedded credentials. Existing public HTTPS/redirect validation also applies.

The optional Ed25519 protocol is an implemented capability. Enabling it still
requires the owner's hosting, signing, release and revocation decisions. Private
signing keys are never app inputs and must not enter source, logs or fixtures.

The index is `SignedCatalogIndex` with `schema_version: 1`, `key_id`, `payload` and
`signature`. The last two fields use standard base64. Signature bytes cover the
literal ASCII bytes `WasmDroid curated catalog index v1` followed by a single NUL
byte and then the exact decoded payload bytes. JSON is not canonicalized or
reencoded for verification. Key selection is only from the trusted local config.
Ed25519 signatures for another protocol cannot be reused here.

The payload is a strict `CatalogManifest`:

| Field | Contract |
| --- | --- |
| `schema_version` | Exactly 1; unknown versions and fields fail |
| `channel` | Must match configured channel |
| `sequence` | Nonnegative integer, at least configured floor, at most signed SQLite integer maximum |
| `issued_at_unix_seconds`, `expires_at_unix_seconds` | Explicit UTC Unix-second validity interval; issued <= supplied now < expiry |
| `catalog_url` | Stable public credential-free HTTPS document |
| `catalog_bytes` | Exact nonzero length, at most 4 MiB |
| `catalog_digest` | Existing lowercase `sha256:<hex>` contract over exact catalog bytes |
| `package_reviews` | Exactly one matching publisher attestation per package, at most 512 |

Each `PackageReview` has `package_id`, `artifact_digest`, nonempty
`license_review` and `android_evidence`. Catalog schema 1 retains its original
single-instance recipe JSON; schema 2 also accepts bounded multi-service recipes.
The signed index and manifest remain schema 1. An optional `artifact_reviews`
map is omitted when empty, preserving legacy single-package review bytes.
Every schema-2 stack requires an exact reference-keyed map for all declared
artifacts, each with matching `digest`, nonempty `license_review` and nonempty
`android_evidence`. Extra, missing or mismatched reviews fail before publication.
The existing primary artifact digest and package-level Android evidence checks
still apply. See [catalog stacks](catalog-stacks.md) for recipe validation.
Remote entries must have `android_verified: true`; evidence must equal the
attestation, and the artifact digest must match. This checks the signed assertion,
not whether the referenced report exists, is truthful or satisfies all required
devices/licenses. A reviewer must verify those facts before publication. No
candidate recipe becomes verified because its metadata parses or is signed.

## Bounded transfer, cancellation and atomic publication

`CatalogUpdates::refresh` reuses the existing cancellable asynchronous HTTPS
transport behind a synchronous owner API. TLS, public-URL redirect validation,
URL-redacted errors, bounded resolver admission and existing 15-second connect /
180-second request timeouts remain in force. The index response is at most
128 KiB; catalog transfer is bounded by its signed size and then exact length and
SHA-256 are independently checked. Transport adapters/regressions can use
`refresh_with`; returned vectors are rechecked before parsing/publication.

The updater retains an existing `EngineOwnership` lease. A separate nonblocking
update file lock excludes concurrent writers without holding the engine's global
storage mutex during HTTPS. The complete verified index, catalog, sequence,
payload digest and acceptance clock are published in a single FULL-synchronous
SQLite transaction in the private engine root. Cancellation uses the existing
single-claim OperationControl. Accepted cancellation before publication rolls
back the transaction. Once publication wins, cancellation returns false and the
commit result is reported truthfully. Network, parse, signature, digest, storage
or cancellation failure never erases/replaces the last accepted snapshot.

Stored sequence prevents rollback. Reuse of the same sequence with a different
signed payload is an equivocation error, even if its catalog digest is unchanged.
An exact replay is idempotent and cannot extend the signed expiry. A backward
clock before the last accepted clock rejects refresh. Callers explicitly supply
Unix seconds; a manipulated device clock is not an independent trusted-time
service. No trusted-time guarantee is claimed.

Offline reads reverify signatures, limits and catalog digest under the current
trust configuration. Freshness is `current`, `expired` or
`clock_before_acceptance` or `below_minimum_sequence`. Expired bytes remain available for truthful offline
display. A missing snapshot returns null; corruption or retired trust yields an
explicit error rather than silently adopting a fallback or resetting the counter.
Changing trust roots therefore requires retaining the key needed to verify the
last accepted snapshot until an explicitly designed migration is available.
Deleting app data loses local rollback state; a shipped release floor can narrow
that risk. Raising the shipped floor preserves the older snapshot as visibly
below-floor metadata while allowing a qualifying newer refresh. Remote key discovery/rotation, reset/migration and emergency revocation
are still owner policy gates.

## Native integration and installation identity

The isolated UniFFI module extends EngineStorage with:

- `refresh_catalog(config_json, now_unix_seconds, control)` -> snapshot JSON.
- `catalog_snapshot(config_json, now_unix_seconds)` -> snapshot JSON or null.
- `catalog_package_for_install(config_json, sequence, catalog_digest, package_id,
  now_unix_seconds)` -> existing CatalogPackage JSON.

Snapshot JSON contains `manifest`, `catalog`, `accepted_at_unix_seconds` and
`freshness`. Configuration input is limited to 16 KiB at the native boundary.
These blocking methods take an independent owner connection and release the
storage mutex before network work. Install selection requires the exact sequence,
catalog digest and current freshness; an updated catalog cannot silently change
a recipe under the package ID. The selected existing CatalogPackage must still
go through the controlled installer, including its pinned resolved-artifact digest
and graph/CAS budgets. Refresh itself never installs bytes, creates an application,
starts a service, edits existing instances or changes an installed recipe.

The local `CatalogUpdateCoordinator` now retains refresh control through native
completion, fences cancellation by generation, participates in common-owner close
admission and reports metadata refresh separately from image installation. Its
immutable trust config is an optional owner-controlled APK asset
`catalog-updates.json`, bounded to 16 KiB. That asset is deliberately absent in the
current source/distribution. It contains public trust material only; downloaded
metadata, imported files and ordinary application preferences cannot enable it.
Missing config is `disabled`; invalid local asset syntax is `invalid`.

AppModel now reads local accepted snapshots, retains the packaged source when no
remote snapshot exists, exposes configuration/freshness/refresh state and routes
remote selection through exact native sequence/digest lookup immediately before
the existing common-owner controlled installation/application preparation. Local
snapshot reads never initiate networking. Explicit refresh and generation-scoped
cancellation methods are wired as backend actions; no automatic polling download
or retry is introduced. Failed snapshot reads preserve the displayed source and
still require native trust/freshness revalidation before installation. The native
settings/refresh UI follows the owner's English scheme shown before this GUI batch.

## Catalog update controls

The new native Settings pane renders approved-build configuration, missing remote
snapshot, current/expired/clock/release-floor status, refresh progress, retained
last-good data and actual update/cancellation outcomes. It shows only public
catalog sequence and acceptance/expiry times, formatted in English; keys,
endpoint configuration and full recipes are not editable or displayed. Composition
performs no network request. The existing common owner remains the refresh owner.

Check for updates is explicit and disabled when disconnected, unconfigured or
already refreshing. Cancel captures the displayed actual operation generation;
the model compares it and forwards that exact generation. A queued request has
no invented cancellation handle. The UI waits for the owner's terminal outcome
and never describes a publishing/accepted update as cancelled. First-fetch
failures/cancellation truthfully preserve the bundled source; they do not claim
a nonexistent previously accepted remote snapshot.

Store/details installation controls share a presentation gate. A remote catalog
needs configured trust, confirmed readable status and current freshness; an
expired/clock-invalid/below-floor/unknown or invalid-config remote selection is
disabled. Bundled selection remains available when remote updates are disabled.
The native snapshot sequence/digest/package lookup and controlled installation
still enforce the actual selection. A failed update may retain a usable current
last-good snapshot; it cannot silently unblock an expired one or replace the
selected application recipe. Refresh installs/starts/edits no application.

Dedicated unexecuted presentation definitions cover build configuration gates,
expiry/clock/release floors, actual cancellation generations, last-good update
failure and unavailable/invalid-trust remote installation. Main/instrumentation
compilation, rendered/rotation/accessibility and live HTTPS/late cancellation
acceptance are tracked separately; source definitions alone are not evidence.

## Acceptance still required

Definitions cover explicit trust/URL gates, valid persistence/reopen, rollback,
equivocation, exact replay, signature tampering/untrusted keys/domain separation,
unknown fields/schema/channel/floor, future/expired documents, byte/digest
corruption, network failure, cancellation phases/token reuse, commit failure,
concurrent writers, retained engine ownership, trust changes, clock rollback,
attestation/digest/evidence mismatch, unverified candidates and stale installation
selection. The neutral sixteen-test suite has a recorded passing local run; new
GUI definitions remain unexecuted and do not replace actual Android acceptance.

Required next evidence: coordinated neutral/native compilation, independent
regressions, actual HTTPS/redirect/body cancellation and interruption/recovery,
Android owner/process/rotation/offline behavior, exact catalog selection into
controlled install, actual Android recipe acceptance and real license review.
Final CI runs only after the owner-authorized local implementation is complete.
