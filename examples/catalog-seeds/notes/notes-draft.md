# Unverified Notes + Static Web Server draft

`notes-draft.catalog.json` is an actual schema-2 draft with one service, one
scoped read-only site volume and the exact existing first-party Notes seed.
It remains `android_verified: false`, with `evidence: null`, and is absent from
the packaged catalog. It cannot pass curated installation/preparation or signed
catalog publication. Parsing and a successful build do not approve this recipe.

## Immutable runtime and first-party content

| Item | Observed value |
| --- | --- |
| Public package | `wasmer.io/wasmer/static-web-server@2.44.0` |
| Registry-declared V3 size | 4,755,156 bytes |
| Registry-declared SHA-256 | `1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08` |
| Selected command | `webserver`, WASI runner, `webserver` atom |
| Private guest root | `/public`, logical volume `site`, read-only guest mount |
| TCP publication | guest 8080, automatically allocated loopback host port |
| Seed | existing `index.html`, 2,831 UTF-8 bytes |
| Seed SHA-256 | `8d9831a1328f8ce0890b5dc58483d5aabdec23cf85f4ea8ae41f0938e8c2ea25` |

The official [package page](https://wasmer.io/wasmer/static-web-server) and a
fresh public [registry GraphQL](https://registry.wasmer.io/graphql) read on
2026-11-17 confirmed the exact public, non-yanked version, V3 size/digest, command
annotations and no declared external WebC `use` bindings. The registry supplied
the credential-free, query-free immutable CDN URL recorded in the draft. No WebC
blob was downloaded or independently hashed during this source inspection.
The normal controlled installer must still verify actual downloaded bytes.

The [upstream v2.44.0 tag](https://github.com/static-web-server/static-web-server/tree/v2.44.0)
resolved to source commit `27aa3450b6bf70bd1fa553b2197b53032affcba1`.
Its [CLI source](https://github.com/static-web-server/static-web-server/blob/27aa3450b6bf70bd1fa553b2197b53032affcba1/src/settings/cli.rs)
defines the selected host, port, root, threads-multiplier and max-blocking-threads
options. The draft explicitly supplies loopback host, port 8080, root `/public`,
threads multiplier 1 and maximum blocking threads 2. These match the earlier
in-memory HTTP probe's invocation. They are server CLI configuration, not hard
resource enforcement or proof of this WebC's build features. The runtime harness
must fail if the exact artifact rejects the arguments or fails to serve the root.

## Factual license inventory and unresolved provenance

| Work | Directly inspected fact | Still unresolved |
| --- | --- | --- |
| First-party Notes HTML/seed | Original repository material, SPDX Apache-2.0 and repository LICENSE | Actual complete recipe/device behavior |
| Upstream Static Web Server 2.44.0 | [Cargo declaration](https://github.com/static-web-server/static-web-server/blob/27aa3450b6bf70bd1fa553b2197b53032affcba1/Cargo.toml) says MIT OR Apache-2.0; [MIT notice](https://github.com/static-web-server/static-web-server/blob/27aa3450b6bf70bd1fa553b2197b53032affcba1/LICENSE-MIT) and [Apache notice](https://github.com/static-web-server/static-web-server/blob/27aa3450b6bf70bd1fa553b2197b53032affcba1/LICENSE-APACHE) were inspected at that commit | Registry WebC build-to-source correspondence and packaged notice compliance |
| Exact registry WebC | Structured `license`, `licenseFile` and `repository` fields returned null; packaging manifest selects one `bin/webserver.wasm` module | Exact patches, build command/toolchain, enabled features, linked Rust/native/runtime components and their notices/SBOM |
| Transitive package graph | Returned manifest declares no external WebC dependency binding | This does not inventory libraries compiled into the atom or prove that it contains no third-party code |

The tag's source version matching the package version is an observation, not a
verified reproducible-build attestation. No package-level license declaration or
binary-source proof is fabricated. Upstream licensing facts and the factual
first-party license are distinct from a legal/distribution verdict for the exact
WebC and its complete dependencies. Review those missing facts before any
curated attestation, signing or redistribution decision.

## Executable later acceptance

`NotesRecipeAcceptanceTest` is a dedicated opt-in instrumentation definition.
The test-only Gradle source includes this example directory in the test APK;
missing `notes-draft.catalog.json` or altered/missing exact HTML fails, with no
fallback fixture. The production APK still receives only its normal curated
catalog/assets. The test never changes the draft's verification fields.

After root coordinates matching runtime arm64/release-native APK generation and
the owner selects the physical device/acceptance point, the bounded existing
runner command is:

```powershell
.\tools\test-android.ps1 -Serial <selected-device> -Abis arm64-v8a -RustProfile release -TestClass dev.wasmdroid.app.NotesRecipeAcceptanceTest -NotesLive -TimeoutSeconds 600
```

The `-NotesLive` switch is accepted only for this exact class. The runner and
test require the explicit opt-in before device work/network/guest admission.
Do not infer physical hardware from an arm64 ABI assertion; record the selected
device, OS and ABI independently. This command has not been run for this draft.
The test requires an unlocked device and notifications explicitly permitted for
WasmDroid. It checks that prerequisite before any binding, network or mutation;
it never requests or grants a permission automatically. This is a harness
prerequisite, not an observed claim about denied-notification behavior on Android.
APK installation can affect existing application processes, so use the agreed
acceptance point rather than an unrelated active user's session. The harness
itself refuses an owner with active controllers/package operations; it does not
stop those applications to make room or trigger cold policy recovery.

The harness asserts that the unverified curated route is rejected. It then uses
the existing advanced common-owner APIs: verified registry pull, actual stack
creation from the draft document and explicit no-overwrite import of the exact
seed bytes into the resulting scoped volume. It creates exactly two UUID-named
test-owned groups. A small additional test-only marker file distinguishes their
private document roots without editing the original HTML or its seed manifest.

The assertions check actual root reference/digest, import readback and refusal
to overwrite, exact HTTP200 body bytes through each allocated host port, distinct
ports/root markers, logical application counts in the actual owner snapshot
(idle 0, both running 2, one stopped 1) and the published foreground notification
at running milestones (2, then 1, then 2 after restart),
selective stop/restart with peer generation preserved,
listener closure, old owner browser-session revocation, persistent volume bytes
and same private profile identity after restart/replacement. The HTTP reader is
bounded and does not follow redirects. Cleanup stops/removes only each exact
acknowledged test-owned group only after resolving its captured native birth
identity and guarding Stop/removal with the exact current state token/version.
Only the original captured scoped volume is removed after that metadata removal
is acknowledged. Missing target capture or replaced lineage preserves the group
and reports incomplete cleanup. It never removes
shared installed images/CAS, calls Stop All or sweeps by prefix. If creation ACK
or cleanup is uncertain, preserve and inspect the exact reported test identity;
do not guess that a pending operation is idle or delete unrelated data.

This advanced-path run cannot establish curated seed receipt/prepublication or
one-tap Store acceptance: production correctly refuses an unverified catalog
entry. Native receipt regressions remain separate. It also does not instantiate
a WebView or test Notes editing, actual localStorage, download handling, provider
profiles, complete browser egress, atomic port-reuse isolation or stable origin.
Those flows need separate current physical acceptance. Profile-byte continuity
does not make origin storage visible after a host-port change.

Record exact source/APK/native/compiler/ABI/device/provider, root and seed digest,
HTTP/lifecycle results, stop outcome, cleanup outcome, unexecuted checks and
provenance review. Only actual matching recipe evidence plus completed review
can lead to normal curated acceptance later. No database compatibility is claimed.

## Current evidence

Three hermetic `engine/tests/notes_draft.rs` definitions validate the actual
draft, exact command/mount/pins/assets and unverified preparation refusal without
creating datasets or promoting the draft. Root reported these 3/3 PASS, alongside
catalog preparation 5/5 and catalog structural 13/13, 21 total, on the current source
tree. The initial integrated source `b4ea540bc9a1f1ce0637bd44e6ffad3c597aefe3`
compiled matching arm64 runtime/bindings/main/instrumentation Kotlin in 16 s
(native 1.73 s); root reported exact native 21/21 PASS and Clippy 1.50 s/format PASS.
The bounded early notification-permission prerequisite then compiled on the
matching final source `f6a44fb0a3a42055545703f6ede84a6de89c8866`: PASS 4 s
(native 1.00 s, 33 tasks / 2 executed). Production main/Rust was unchanged from
the main source above. The instrumentation definition has not run; actual
physical execution is separate evidence maintained by root.
