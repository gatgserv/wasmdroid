# First-party local notes seed

`index.html` is original project material under the repository's Apache-2.0
license. It has no third-party libraries, remote styles, fonts, analytics or
database dependency. The inline text manifest in `assets.json` is a seed-file
example only; it does not name a package, command, accepted catalog entry or
Android evidence. It is not included in the packaged curated catalog.

The page stores a note in browser localStorage and can request a text download.
It does not save changes into the service dataset. The browser profile can retain
bytes across same-spec instance retries, while scheme/host/port still determine
which origin can read them. A changed address/port or an explicit browser-data
clear affects access. Download handling and storage/provider behavior require
actual Android acceptance; this page makes no runtime compatibility claim.

The example page has an explicit content security policy and makes no network
requests. That page policy does not establish a general browser egress guarantee.
The application continues to report complete browser network isolation as
Unsupported where the concrete platform primitive is absent.

Before a real curated recipe can use this asset, select and verify an immutable
public static-server artifact, its exact command/mount/port, source and license
provenance. Then test the complete seeded-volume recipe, HTTP response, browser
storage, actual provider behavior, stop/retry and no-overwrite recovery on the
required Android device. No package or command is guessed here.

`assets.json` includes the exact UTF-8 bytes, size and SHA-256 of `index.html`,
targeting logical volume `site` at relative path `index.html`. Regenerate it when
the source file changes; do not update only the digest or length. The native
validator verifies both and refuses mismatches before volume creation.
