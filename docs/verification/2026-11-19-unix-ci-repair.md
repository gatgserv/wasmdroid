# Unix directory sync and socket descriptor verification

The [failed CI run](https://github.com/gatgserv/wasmdroid/actions/runs/44421695752)
tested `597e47475be0b253b1a2e67e453d1de28b6f0223`. Linux catalog asset seeding
failed with `EBADF` when syncing a directory capability opened with `O_PATH`.
Two runtime tests checked only the integer descriptor after it had been closed,
which allowed an unrelated, reused descriptor to fail the assertion.

Fix source `66e86b3d637bbf7a5d04cc2e0e1c0a9d12328b74` opens a readable directory
handle within the volume capability, preserves the no-symlink and exclusive-write
checks and still propagates sync errors. Both runtime tests retain the original
socket and check its identity through callback completion and cancellation.

On the Windows x86_64 host, all 298 engine and FFI tests passed. Formatting and
Clippy also passed:

```powershell
cargo fmt --all --check
cargo test --locked --offline -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked --offline -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The Unix paths and runtime tests require the Linux CI jobs. WSL could not start
locally. These checks do not cover Android device execution or the Google Play
release.
