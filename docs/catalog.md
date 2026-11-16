# Catalog and recipe contract v1

`catalog/catalog.v1.json` is a valid versioned document with `schema_version: 1`
and a `packages` array. It intentionally contains no packages until Android
acceptance evidence exists. `Catalog::parse` is the current executable validator.
Unknown schema versions/fields and duplicate IDs are errors.

Each package requires `id`, `name`, `description`, `publisher`, `digest`,
`download_url`, `android_verified`, `recipe`; optional `evidence` and `web`.
IDs use 1–128 ASCII letters/digits/underscore/hyphen. Downloads use HTTPS and
must match the trusted catalog's SHA-256 before installation. Digest verification
does not establish publisher authenticity; catalog hosting/signing and update
trust policy must be designed before enabling remote updates.
`android_verified: true` requires a committed test report reference in `evidence`.
Technical validation cannot prove the report's truth: review must check it.

`recipe` is an `InstanceSpec` from `docs/engine-api.md`. A web recipe declares
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
Review permissions and pin dependencies. Future catalog UI metadata (icons,
screenshots, translations) needs a versioned contract extension.
