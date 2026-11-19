# WasmDroid development guidelines

- Follow [CONTRIBUTING.md](CONTRIBUTING.md) and the build instructions in [the development guide](docs/development.md).
- Keep application UI, notifications and accessibility text in English.
- Keep Wasmer dependencies and imports in `engine/drivers/wasmer-local`.
- Preserve neutral serde contracts, endpoint identity, explicit unsupported errors and sandbox defaults.
- Build Android through Gradle to generate matching native libraries and Kotlin bindings.
- Target an explicit device serial when running Android tests.
- Record tested revisions, commands, ABIs, outcomes and limitations in `docs/verification`.
- Keep credentials, local SDK paths, native build outputs and signing keys out of Git.
