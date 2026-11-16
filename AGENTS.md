# WasmDroid working instructions

- Read the product plan, README, `docs/backlog.md`, and verification evidence before changes.
- Communicate with George in Romanian; write code and technical documentation in English.
- Keep Wasmer dependencies and imports in `engine/drivers/wasmer-local` only.
- Preserve neutral serde contracts, endpoint identity, explicit unsupported errors, and sandbox defaults.
- On Windows prefer `%USERPROFILE%\.cargo\bin` rustup proxies: a separate Rust MSI can shadow them.
- Run README Rust checks. Build Android through Gradle, which generates matching native libraries and Kotlin bindings.
- Run targeted device tests with `tools/test-android.ps1 -Serial <selected-device>`; never restart or wipe unrelated emulators.
- Do not mark Stage 0 complete from a bridge test or built-in WASI fixture. Registry WebC, Python and real device HTTP acceptance are separate.
- Record exact tested revision, commands, ABI, outcomes, skipped checks and limitations in `docs/verification`.
- Keep credentials, local SDK paths, generated bindings, native build outputs and signing keys out of commits.
- The GitHub workflow is prepared locally; do not claim remote CI ran until a connected repository supplies that evidence.
