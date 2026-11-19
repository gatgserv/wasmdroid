# Contributing

Start with the product plan, `docs/engine-api.md`, `docs/catalog.md` and
`docs/backlog.md`. Keep code, technical documentation and commit messages in English.
Contributions are provided under Apache-2.0, consistent with the project license.

Use a separate branch. Do not add Wasmer dependencies/imports outside
`engine/drivers/wasmer-local`. UI code calls the engine boundary only. Changes to
unsupported runtime options must fail explicitly, never silently drop fields.
Keep secrets, signing keys, private registry links and local SDK paths out of Git.

Run the [Rust checks](docs/development.md#rust-checks) and the checks appropriate
for the Android changes.
Record the revision, commands, target ABI, results and limitations in the PR.
Do not label emulator results as physical arm64 results or the finite WASI probe
as the complete Stage 0 acceptance. Do not weaken gates to pass a build.

Catalog entries must pin SHA-256, include publisher/license information in the
review, and follow `docs/catalog.md`. Android verification needs a reproducible
test report, not an assumption based on another operating system. Unknown owner
decisions remain open. Merges, public publication and Play uploads need owner authorization.
