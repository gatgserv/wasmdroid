# Third-party notices

This is an initial direct-dependency record, not a completed release license audit.
Before APK distribution, generate a full transitive dependency notice bundle,
including the selected runtime backend, Kotlin/Android dependencies and native libraries.
Preserve original notices and satisfy each dependency's license terms.

| Component | License | Role |
| --- | --- | --- |
| Wasmer / WASIX | MIT | WebAssembly runtime, selected only for runtime builds |
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
