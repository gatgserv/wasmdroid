# Catalog and recipe contract

`catalog/catalog.v1.json` is a valid legacy document with `schema_version: 1`
and a `packages` array. The parser supports schemas1–3. The shipped catalog
intentionally contains no packages until Android
acceptance evidence exists. `Catalog::parse` is the current executable validator.
Unknown schema versions/fields and duplicate IDs are errors.

An opt-in signed remote index/manifest and retained owner-side refresh backend now
reuse this catalog contract. The packaged catalog remains empty, and no production
hosting/trust configuration is shipped. See [catalog updates](design/catalog-updates.md)
for exact signatures, rollback/freshness, pinned installation and pending acceptance.

Each package requires `id`, `name`, `description`, `publisher`, `digest`,
`download_url`, `android_verified`, `recipe`; optional `evidence`, `web` and
schema3-only `media`. Absent media stays absent in legacy serialization; explicit
null media is an error.
IDs use 1–128 ASCII letters/digits/underscore/hyphen. Downloads use HTTPS and
must match the trusted catalog's SHA-256 before installation. Digest verification
does not establish publisher authenticity; catalog hosting/signing and update
trust policy must be designed before enabling remote updates.
`android_verified: true` requires a committed test report reference in `evidence`.
Technical validation cannot prove the report's truth: review must check it.

The legacy single `recipe` is an `InstanceSpec` from `docs/engine-api.md`.
Schemas2/3 also accept pinned multi-service recipes with the bounded Compose
document, exact artifact inventory, primary service and optional no-overwrite
seed assets; see [catalog stacks](design/catalog-stacks.md) and
[seed assets](design/catalog-assets.md). A web recipe declares
`web: {"instance_port":8080,"path":"/"}` and a matching TCP port mapping.
The engine constructs a loopback launch URL from its allocated port. Recipes
cannot supply an arbitrary external web launch URL. No unverified package is
automatically marked compatible with Android.

Example (documentation only; the package and download do not exist):

```json
{
  "schema_version": 1,
  "packages": [{
    "id": "example-site",
    "name": "Example website",
    "description": "An example recipe, not an installable catalog entry",
    "publisher": "example",
    "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "download_url": "https://example.invalid/site.webc",
    "android_verified": false,
    "recipe": {
      "artifact_ref": "wasmer.io/example/site@1.0.0",
      "command": "serve",
      "ports": [{"host_port":0,"instance_port":8080,"protocol":"tcp"}]
    },
    "web": {"instance_port":8080,"path":"/"}
  }]
}
```

Before publishing a package, record its license/redistribution permissions,
publisher provenance, immutable registry version/digest, commands and arguments,
supported Android ABI/device/OS, persistent data mounts, network needs, startup
and HTTP readiness check, stop/restart behavior, and measured size/time/memory.
Review permissions and pin dependencies. Schema3 adds bounded authenticated
inline icons/screenshots and explicit media provenance/review; see
[catalog media](design/catalog-media.md). This supplies no translation system,
new remote-image fetch or automatic compatibility/provenance certification.
