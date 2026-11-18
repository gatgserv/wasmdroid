# Curated multi-service catalog recipes

This extends the neutral curated catalog for issues #1 and #5. It supplies
structural metadata for an explicitly reviewed installation. It does not add a
verified shipped recipe, authorize a guest run, seed application assets or grant
access to existing datasets. The packaged catalog remains empty until actual
package, license and Android acceptance is recorded.

## Compatible wire contract

`Catalog::parse` accepts catalog `schema_version` 1 and 2. Version 1 accepts the
existing `InstanceSpec` recipe object unchanged. Version 2 accepts that same
single recipe, or this strict object through the untagged `CatalogRecipe` enum:

```json
{
  "type": "stack",
  "document": "services:\n  web:\n    image: wasmer.io/example/web@1.0.0\n    ports: ['8080']\n",
  "artifacts": [
    {
      "reference": "wasmer.io/example/web@1.0.0",
      "digest": "sha256:<64 lowercase hex characters>",
      "download_url": "https://packages.example.invalid/web.webc"
    }
  ],
  "primary_service": "web"
}
```

This illustrates the schema only; its placeholder digest/package/report is not
installable or verified. Every artifact object and stack object rejects unknown
fields. A single recipe does not gain an envelope, a `type` field or a changed
`InstanceSpec` shape. `CatalogPackage::single_recipe()` and `stack_recipe()` give
typed borrowed views; `artifacts()` returns declared installation roots in their
declared order. For a single recipe, its sole artifact combines the existing
recipe reference and top-level digest/download URL.

Stacks have one to eight services and one to eight unique declared artifacts.
An artifact can serve several services. All references must be public
`wasmer.io/namespace/package@version` references, with an exact canonical semantic
version parsed by the existing semver dependency. Tags, ranges, missing versions,
private registries and imported local digests are rejected for these recipes.
Immutable SHA-256 pins remain required even with an exact version reference.

The unique artifact-reference set must equal the Compose service-image set:
there are no undeclared downloads or unused declarations. The selected
`primary_service` must exist. The top-level package `digest` and `download_url`
must equal that primary service's declared artifact. Optional top-level `web`
must select a declared TCP port on that same primary service and a local path.
Another service's port cannot authorize the web target.

## Validation before installation

`stacks::catalog::validate_recipe` is pure: it does not open `StateDb`, resolve
packages, access files, create data, download content or start guests. It reuses
the existing strict bounded Compose YAML/JSON parser and `InstanceSpec::validate`
for every service. The same dependency ordering rejects cycles and missing
dependencies. Existing parser contracts reject unknown Compose features,
duplicate keys, anchors/aliases, host interpolation, host bind mounts, invalid
guest paths, duplicate target mounts, non-TCP publications and ambiguous aliases.
The complete document is checked for duplicate fixed host ports across services
and mounts to undeclared volumes. Automatic host port allocation remains allowed
on multiple services.

Only newly scoped named-volume definitions are accepted. External volumes and
unscoped dataset names are rejected even when a same-named local dataset happens
to exist. Catalog metadata cannot grant authority to existing private data.
Root artifact URLs use the current stable public HTTPS catalog URL policy:
credentials, queries, fragments and local/private IP literals are rejected.
The controlled transport additionally applies its existing resolver, redirect,
cancellation and content-publication safeguards; structural URL parsing alone
is not a network capability.

The pure check cannot establish installed command availability or actual WebC
runtime support. A missing command override remains deferred to the installed
artifact's real entrypoint metadata; the internal validation placeholder is
never returned as a plan or executed. After controlled installation the native
preparer rechecks every declared installed artifact digest and uses the existing
`StackManager` preview/create-plan path. That stage checks commands, final
normalized plan budgets, actual ownership, datasets and package state again.
Creation still does not start any service. Parser or metadata acceptance is not
Android execution evidence.

## Signed artifact attestations

The index/signature/manifest protocols remain unchanged. `PackageReview` gains
an optional `artifact_reviews` map:

```json
{
  "wasmer.io/example/web@1.0.0": {
    "digest": "sha256:<64 lowercase hex characters>",
    "license_review": "path to a reviewed report",
    "android_evidence": "path to actual matching Android evidence"
  }
}
```

A stack requires exactly one map entry per declared artifact reference. Duplicate
JSON review keys and maps with more than eight entries are rejected. Each
digest must match, and each license/evidence attestation must be nonempty. The
primary package-level digest, `android_verified`, matching package evidence and
license review remain required. A legacy single recipe can omit the map; if it
supplies one, it must exactly match that single artifact too. The signed whole
catalog digest continues to bind the original document, primary service,
artifact inventory, commands and metadata. Reusing only artifact pins does not
authorize a changed graph under an earlier catalog selection.

These are publisher assertions checked against signed metadata. They do not
prove that reports exist, licenses permit redistribution or Android accepts the
complete stack. Publication still requires actual per-artifact and combined
recipe evidence, source/license review and explicit owner trust configuration.
No host, key, license verdict or verified entry is synthesized by this backend.

## Regression definitions and evidence

Normal My Apps cards now consume actual native membership and common-owner
state/selection fences. Start missing parts, Stop, Open, per-part data and scoped
metadata removal follow the English combined-app proposal; labels do not select
runtime members. Optional bounded inline `assets` seed newly prepared private
volumes with immutable receipts before publication. Encoded package entries must
fit the existing128KiB installation boundary before catalog acceptance. Actual
recipe/guest/provider/visual acceptance remains pending.

`engine/tests/catalog_stacks.rs` defines twelve hermetic structural cases:
legacy wire compatibility, schema separation, shared artifact references,
primary artifact/web binding, exact immutable inventory, URL constraints,
dependency failures, dataset authority, fixed-port conflicts, strict parser/spec
checks, budgets and redacted diagnostics. `engine/tests/catalog_updates.rs` adds
four metadata cases for complete stack attestations, whole-recipe selection
pins, optional legacy single maps and duplicate attestation keys. All synthetic references/reports
are fixtures, not approved catalog entries.

These test definitions were pending compilation and execution at this checkpoint. Device group HTTP/data/stop/retry, actual asset seeding and
complete recipe acceptance remain separate. No GUI, remote CI or release change
is included in this backend batch.
