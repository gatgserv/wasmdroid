# Curated package candidate inventory — 2026-11-17

**Every recipe below is UNVERIFIED for the current product.** Nothing in this
inventory is added to `catalog/catalog.v1.json`, marked `android_verified`, or
approved for redistribution. This is public metadata/source inspection performed
while the browser source batch awaited compilation. No candidate was downloaded,
executed, tested on Android, deployed or submitted to CI in this research step.

The source-era static-server HTTP probe is real historical evidence with a narrow
scope. It is not acceptance of the new common controller, browser profiles,
catalog recipe, persistent volumes or current compiled revision.

## Reproducible metadata observations

Public reads used the [official Wasmer registry GraphQL endpoint](https://registry.wasmer.io/graphql)
and the repository's existing `engine/graphql/resolve.graphql` shape. Each result
was re-resolved with the exact version shown below. The distribution request
explicitly asked for V3; two packages still returned V2. No authenticated registry,
account, package execution or temporary download URL was used or retained.
Reported byte sizes/digests are **registry declarations**, not newly downloaded
or independently hashed measurements. Production installation must verify them.

```graphql
query Candidate($name: String!, $version: String) {
  getPackageVersion(name: $name, version: $version) {
    version createdAt license licenseFile repository readme manifest
    package { private }
    distribution(version: V3) {
      webcVersion webcSize webcSha256Hash webcManifest
    }
  }
}
```

All four selected versions were public and not yanked at inspection. No
`use`/external WebC dependency binding was present in their returned WebC
manifests. That is not a binary/SBOM audit: libraries can be compiled into atoms
and filesystem volumes can contain third-party assets. Package license fields
were null for the first three entries; `licenseFile` was empty for all four.

| Public package/version | Format; declared bytes | Commands / entrypoint | Initial admission status |
| --- | --- | --- | --- |
| `wasmer.io/wasmer/static-web-server@2.44.0` | V3; 4,755,156 | `webserver`, `static-web-server`; entrypoint `webserver`; WASI runner | Candidate static site/file-listing runtime; recipe/license/current Android acceptance pending |
| `wasmer.io/php/php@8.3.403` | V3; 85,700,002 | `php`; entrypoint `php`; WASI runner | Candidate local PHP web runtime; needs reviewed application assets and build/dependency provenance |
| `wasmer.io/sqlite/sqlite@0.2.2` | **V2**; 3,575,496 | `sqlite`, `sqlite3`; no manifest entrypoint; WASI runner | **Blocked:** current installer requires WebC V3; database CLI, not an HTTP database service |
| `wasmer.io/wasmer-examples/php-wcgi-starter@0.2.3` | **V2**; 11,054,805 | `php`; entrypoint `php`; **WCGI** runner | **Blocked:** format and runner; template alone is not proof of PHP binary licensing |

Exact declared SHA-256 values:

| Package | `sha256:<hex>` |
| --- | --- |
| Static Web Server 2.44.0 | `sha256:1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08` |
| PHP 8.3.403 | `sha256:da8d3fcfcf02d2401787532c4af3fdaf5b680b05144a9591ca70b97131ee2f32` |
| SQLite package 0.2.2 | `sha256:4226126639def5cfbd30479a5849f433c50a2b8e0ff12530347f05cc5c319891` |
| PHP WCGI starter 0.2.3 | `sha256:a244b02ef120a1a045f221bcf0383752f1daf924ea8c79b306f9cd92c4d81c6f` |

## Static Web Server: first practical web candidate

Sources: [public package](https://wasmer.io/wasmer/static-web-server),
[upstream v2.44.0 package/license declaration](https://github.com/static-web-server/static-web-server/blob/v2.44.0/Cargo.toml),
[exact CLI source](https://github.com/static-web-server/static-web-server/blob/v2.44.0/src/settings/cli.rs),
[MIT notice](https://github.com/static-web-server/static-web-server/blob/v2.44.0/LICENSE-MIT)
and [Apache notice](https://github.com/static-web-server/static-web-server/blob/v2.44.0/LICENSE-APACHE).
The public tag resolved to source revision
`27aa3450b6bf70bd1fa553b2197b53032affcba1` during inspection. The registry does
not provide an artifact-to-source-build attestation for this observation.

The upstream package declares MIT OR Apache-2.0. The registry package omits a
license declaration, so packaged notices, Rust dependencies and source/build
provenance still need review before approving the exact WebC. Its WebC manifest
contains the `webserver` atom and a metadata README, with no declared site-asset
mount or WebC dependencies. No ready-made document UI is asserted to be bundled.

Proposed **unverified** neutral runtime recipe:

```json
{
  "artifact_ref": "wasmer.io/wasmer/static-web-server@2.44.0",
  "command": "webserver",
  "args": ["--host", "127.0.0.1", "--port", "8080", "--root", "/public"],
  "mounts": [{"source":{"type":"volume","source":"site-content"},"target":"/public","read_only":true}],
  "ports": [{"host_port":0,"instance_port":8080,"protocol":"tcp"}],
  "internet_access": false,
  "restart_policy": "no",
  "start_on_boot": false
}
```

The current catalog preparation path now supports bounded inline UTF-8 seed
files in newly scoped private volumes, with exact length/digest, immutable
receipts, exclusive leases and atomic no-replace publication. See
[seed contract](design/catalog-assets.md). This replaces the earlier missing-seed
prerequisite; an empty volume alone is still not a complete one-tap website.
The actual [Notes draft](../examples/catalog-seeds/notes/notes-draft.catalog.json)
combines this immutable runtime with the exact first-party HTML asset and stays
`android_verified:false`, with no evidence attestation or production promotion.
[Draft provenance and acceptance boundaries](../examples/catalog-seeds/notes/notes-draft.md)
record freshly inspected official version/CLI/license facts separately from
unresolved binary-source/transitive provenance. No new registry blob or guest
execution was performed while writing it. Upstream directory listing remains
feature-gated; the draft does not enable or advertise it.

Acceptance recipe: install and independently verify the pinned blob; seed two
unique private document roots through supported data import; create two normal
applications with the same guest port; receive each expected HTML marker via the
actual allocated host port and embedded profile; stop/restart one application and
verify content persists, old session authority is revoked and the peer remains
usable; observe actual worker/forwarding cleanup, cancellation and force outcome.
Record provider, physical arm64 ABI/OS, current source revision, notices, package
digest, cold/warm compile/start time, bytes and measured RSS/CPU with units.

Historical evidence: [2026-11-16 native HTTP report](verification/2026-11-16-loopback-http.md)
used the same digest and reported HTTP 200 on a physical arm64 phone with
in-memory documents and forced cleanup. It does not certify this proposed
persistent-volume/product/browser recipe or current controller source.

## PHP: useful web runtime, application recipe still missing

Sources: [public package](https://wasmer.io/php/php),
[official CLI server contract](https://www.php.net/features.commandline.webserver),
[CLI options](https://www.php.net/commandline.options) and
[historical PHP 3.01 license](https://www.php.net/license/3_01.txt).
The exact package was created 2025-01-14 according to the registry. Its README
declares PHP License 3.01; the registry's structured license/repository fields
are empty. The packaging version `8.3.403` is not an independently observed
`php -v` result. No exact Wasmer build-source revision, patched source or binary
SBOM was established here. Do not infer a license change from a current upstream
landing page or treat a runtime README as review of every bundled work.

The manifest exports a CLI `php` atom and embeds mounts `/etc/ssl` and `/icu`
from package volumes named under `php-wasix-deps`. It declares no application
document root and no external WebC dependency binding. Exact library versions,
certificate/ICU data notices, available PHP extensions and the actual binary
license bundle remain unreviewed.

Candidate invocation is `php -S 127.0.0.1:8080 -t /app`, mapping a reviewed
application volume at `/app` and guest TCP 8080 to automatic loopback host ingress.
This follows upstream CLI syntax; it has not been run in this artifact/runtime.
Upstream describes this as a development/demo server, not a public production
server. It is not a reason to grant LAN or guest internet access.

Before a useful catalog entry exists, select and pin separately licensed web UI
assets, prepare them through a reviewed reproducible mechanism, and decide whether
this controlled local demonstration fits the product. Do not fabricate a bundled
PHP admin interface, SQLite extension, MySQL connector or one-tap asset installer.
If a database-backed UI is selected, first verify its exact extension support and
isolate writable database storage from read-only application assets.

Acceptance recipe: verify pinned artifact and notices; run `php -v` and the exact
required extension queries without exposing environment/credentials; serve a
reviewed marker page from private `/app`; execute an input-validation and storage
write/read fixture; restart with persistent data and separate profiles; demonstrate
actual stop/descendant cleanup and a second concurrent application's preservation.
No external database, account or unrestricted network backend is a prerequisite
substitute. Record current device/source/compiler/provider and measured resource
outcomes; publish no Android-compatible claim before the real run.

The generic bounded seed mechanism is now implemented, but that alone does not
supply an accepted PHP application or establish available extensions. The Notes
draft uses a static server and makes no PHP/SQLite/database compatibility claim.

## SQLite: concrete database candidate blocked by format

Sources: [public package](https://wasmer.io/sqlite/sqlite),
[packaging manifest](https://github.com/wapm-packages/sqlite/blob/f213eab4e27e284a01c4bdd28e2c8569663cd308/wapm.toml),
[packaging source](https://github.com/wapm-packages/sqlite/tree/f213eab4e27e284a01c4bdd28e2c8569663cd308),
[official SQLite copyright statement](https://sqlite.org/copyright.html) and
[CLI semantics](https://www.sqlite.org/cli.html).

The registry README describes SQLite 3.27.2 while the inspected packaging source
README describes 3.26.0. The manifest declares the old `wasi: 0.0.0-unstable`
interface. These observations require exact binary/version/ABI inspection; a
current upstream SQLite feature list cannot be attributed to this 2019 package.
Upstream SQLite dedicates its own deliverables to the public domain; that alone
does not review the wrapper, toolchain or other linked work in this WebC.

The immediate correct outcome is an explicit format rejection: the existing
resolver demands V3 and received V2 even when V3 was requested. Do not weaken
that contract, relabel the returned bytes V3, or assume raw import bypass is
supported. A reviewed V3 build/republication or a separately authorized format
extension is required before Android execution acceptance can begin.

After that prerequisite, an unverified advanced recipe would use `sqlite3` with
`/data/app.sqlite`, one writable private volume mounted at `/data`, `network:none`,
no published ports and explicit stdin/TTY. It is an embedded database CLI, not a
TCP database server or browser admin UI. Required checks: exact `sqlite_version()`,
create/insert/select, graceful EOF and interrupt, reopen after worker restart,
transaction durability/error behavior, read-only and traversal isolation, clean
volume backup/restore, and preservation of another application's data. No real
database measurement or SQL execution was performed for this inventory.

## PHP WCGI starter: tempting template, two independent hard blockers

Sources: [public package](https://wasmer.io/wasmer-examples/php-wcgi-starter) and
[declared source manifest](https://github.com/wasmerio/wcgi-php-template/blob/9e706ba1698863ce00465b294ff096b1910a7c74/wasmer.toml).
The registry declares MIT OR Apache-2.0 for the template; the inspected source has
that declaration but package name/version `wasmer/wcgi-php-template@0.1.2`, not
the registry's `wasmer-examples/php-wcgi-starter@0.2.3`. Binary provenance and PHP
interpreter/embedded asset notices therefore remain unresolved.

The WebC has its own `php-cgi.wasm`, mounted `/app` assets and WCGI annotations
for RFC-3875 with `DOCUMENT_ROOT=/app` and `SCRIPT_FILENAME=/app/index.php`.
It has no external WebC dependency binding. The observed V2 container is already
outside the installer contract; its `https://webc.org/runner/wcgi` command is also
outside the current service worker's WASI runner selection. A website template
working on Wasmer Edge does not demonstrate compatibility with this local driver.

No install/start acceptance is proposed until both format and runner support are
deliberately implemented and reviewed. A future acceptance recipe must verify the
CGI gateway/environment/request-body/response/timeout boundary, exact assets,
private data and browser isolation. Replacing WCGI metadata with a WASI label would
not supply the missing request protocol.

## What remains before a publishable inventory

These four concrete observations narrow the next work: Static Web Server is the
most direct current-runtime candidate; PHP needs reviewed application assets and
build provenance; SQLite and WCGI have verified metadata-level admission blockers.
Searches for `redis`, `pocketbase` and `filebrowser` returned no public command
results in this bounded query. That is not proof that no such package exists.
No deployable two-service database/web stack has been established.

For every eventual entry: resolve and verify the full graph, bind version/digest
to build/source/license notices, supply actual application assets, define minimal
mount/network/port/permission requirements, test on the current physical arm64
product flow and record start/readiness/stop/restart/data/browser measurements.
Independent acceptance and owner-approved catalog signing/hosting remain gates.
Leave the shipped catalog empty until the corresponding evidence and review are
committed; source metadata inspection alone closes no compatibility criterion.
