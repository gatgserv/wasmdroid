# Third-party notices

This is an initial direct-dependency record, not a completed release license audit.
Before APK distribution, generate a full transitive dependency notice bundle,
including the selected runtime backend, Kotlin/Android dependencies and native libraries.
Preserve original notices and satisfy each dependency's license terms.

| Component | License | Role |
| --- | --- | --- |
| Wasmer core / WASIX / Cranelift backend | MIT | Default WebAssembly runtime builds |
| Wasmer Singlepass 7.4.2 | BUSL-1.1, separate upstream LICENSE | Optional development benchmark only; not enabled in default/distribution builds |
| UniFFI | MPL-2.0 | Binding generator and runtime support |
| serde / serde_json | MIT OR Apache-2.0 | API serialization |
| rusqlite | MIT | SQLite access |
| SQLite | Public domain | Embedded metadata database |
| sha2 | MIT OR Apache-2.0 | Digest verification |
| tempfile | MIT OR Apache-2.0 | Atomic verified blob publication |
| hex | MIT OR Apache-2.0 | Digest encoding |
| url | MIT OR Apache-2.0 | Catalog URL validation |
| thiserror / anyhow | MIT OR Apache-2.0 | Error handling |
| Tokio | MIT | Runtime probe executor |
| reqwest | MIT OR Apache-2.0 | Public registry HTTPS client |
| rustls / webpki-roots / ring | Multiple permissive licenses; see each upstream notice | TLS implementation and public trust roots |
| semver | MIT OR Apache-2.0 | Public package dependency constraints |
| AndroidX / Jetpack Compose | Apache-2.0 | Native Android UI |
| kotlinx.coroutines | Apache-2.0 | Main-thread-safe native calls |
| JNA | LGPL-2.1-or-later OR Apache-2.0 | UniFFI native loading; use Apache-2.0 option |

Upstream license texts and notices must be bundled before distribution, not replaced
by this summary. No registry packages are currently included. Apache-2.0 for
WasmDroid does not relicense dependencies or downloaded packages.

The Wasmer LLVM and Singlepass compiler crates have separate BUSL licenses in
version 7.4.2. The default Cranelift compiler is MIT. Do not infer compiler licensing
from the Wasmer repository's top-level license. LLVM is not a project dependency.
Singlepass is an explicitly selected non-production experiment; its presence in
Cargo.lock does not enable it in the default APK. Production/distribution use of
these BUSL compilers requires a separate terms review and owner decision. See the
[Singlepass license](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/lib/compiler-singlepass/LICENSE)
and [LLVM license](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/lib/compiler-llvm/LICENSE).
