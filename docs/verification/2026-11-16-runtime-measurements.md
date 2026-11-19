# Physical arm64 runtime measurements — 2026-11-16

Application code: `4b0c9fd40ee39b40761e7453caf8d2b8841f4791`.
Device: Sony G8142, Android 15/API 35, arm64-v8a, 4096-byte pages.
Rust 1.95.0, Wasmer 7.4.2/WASIX 0.704.2, NDK 28.2.13676358, JDK 21.
Tracking: [issue #1](https://github.com/gatgserv/wasmdroid/issues/1),
[PR #2](https://github.com/gatgserv/wasmdroid/pull/2).

Raw samples and artifact SHA-256 values are in
[runtime-measurements.json](2026-11-16-runtime-measurements.json).
Cranelift passed **8 physical-device tests**, no failures/skips, **98.442 s**,
ADB exit 0: three bridge tests, core benchmark, Python benchmark, Python sandbox
regression and both HTTP/isolation regressions. See
[device output](2026-11-16-phone-runtime-tests.txt).
Singlepass passed its three bridge tests and integer-only benchmark, **4 tests**,
**0.184 s**, ADB exit 0. See [output](2026-11-16-singlepass-core-tests.txt).
Both clean Android debug builds passed lint: **0 errors, 9 existing warnings**.
Core Rust: 26 tests and Clippy passed. Cross-target arm64 Clippy passed for both
compiler feature selections. Native CI remains separate evidence.

## Timings and scope

Device timings use Android debug signing with **Rust release optimization**.
The fixed Python workload sums 100,000 integers in a Python loop, checks the
result and exits. It is not a general application-performance score.
Package: `wasmer.io/python/python@3.13.20`, WebC SHA-256
`a9fa8202f1bf6a4eca8d31ee5f2b1970c8f45af5c5520cc954d28a133333d8e7`.

| Cranelift Python phase | Measured value |
| --- | ---: |
| Verified graph parsing / runtime preparation | 1.069645 s |
| Cold compile and in-memory cache insertion | 34.589410 s |
| Cold compile through first workload exit | 35.184192 s |
| Warm compiled-module cache lookup | 101 microseconds |
| Warm workload relaunches through exit | 0.606322, 0.242222, 0.606122 s |
| Median warm relaunch, n=3 | 0.606122 s |
| Process RSS at measurement end | 484,073,472 bytes |
| Kernel process RSS high-water mark | 601,276,416 bytes (573.42 MiB) |

Registry requests/downloads and graph preparation are excluded from runtime
timings. Cold starts use a fresh runtime cache; the harness asserts absence
before compilation and presence afterward. The first measured sequence resolves
the command module, then launches it through the normal WASIX runner. Its total
is measured wall time, not an estimate formed by adding unrelated samples.
Warm relaunches use the same runtime and compiled module cache. They include
WASI environment setup, script execution, exit and bounded output draining.
They do **not** demonstrate disk-cache persistence or a warm new Android process.

RSS is for the entire instrumentation/app process, including JVM, native runtime,
registry setup, JIT, graph buffers and earlier tests. VmHWM is the kernel's
process-lifetime high-water mark, not a resettable per-instance memory metric.
No guest memory/CPU limit is inferred from these measurements.

The comparable core microbenchmark performs one million fixed integer recurrence
steps, verifies checksum `507449365` against a Rust reference, and has no imports.
WAT parsing/reference calculation are outside the reported compile/guest times.

| Integer-only core workload | Cranelift | Singlepass development experiment |
| --- | ---: | ---: |
| Cold compile | 5.127 ms | 3.347 ms |
| Instantiation / export lookup | 0.727 ms | 0.107 ms |
| Median guest execution, n=5 | 3.394 ms | 4.111 ms |
| Guest execution range | 3.318–3.537 ms | 3.960–4.128 ms |
| Whole-process peak RSS at this point | 132,177,920 bytes | 129,736,704 bytes |

These are short, uncontrolled mobile samples, with one cold compile per final
build and five execution calls. CPU frequency, scheduling, temperature and GC
were not controlled. An earlier Singlepass trial had a 7.792 ms execution median
and 2.800 ms compile time; raw data is retained to expose this variability.
Battery was charging, 22–23%, with observed temperatures 31.7–32.0 C across the
session. Do not claim statistical significance, a universal backend speedup or
steady-state production memory from this small sample.

Conversions: seconds = raw microseconds / 1,000,000; milliseconds = microseconds
/ 1,000; MiB = bytes / 1,048,576. Median sorts the raw samples and selects the
middle value (n=3 or n=5). Raw integer samples remain authoritative; displayed
MiB is rounded to two decimals. No measurement is substituted for a missing one.

## Backend compatibility and licensing

Cranelift remains the default, MIT-licensed compiler. The pinned Singlepass and
LLVM compiler crates have separate BUSL license files. This corrects the earlier
blanket Wasmer/MIT wording; it does not change WasmDroid's Apache-2.0 license.
Source: the published crates' LICENSE/Cargo.toml files, matching upstream commit
`7a48a071c7682a409d148cf37dc8da58322a123d`; see
[Singlepass LICENSE](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/lib/compiler-singlepass/LICENSE),
[LLVM LICENSE](https://github.com/wasmerio/wasmer/blob/7a48a071c7682a409d148cf37dc8da58322a123d/lib/compiler-llvm/LICENSE)
and [runtime feature matrix](https://docs.wasmer.io/runtime/features/).

Singlepass is only an explicitly selected, non-production development benchmark.
Its feature does not activate Cranelift: dependency-tree inspection found no
Cranelift crate in that selected FFI graph. Android release variants are disabled
for Singlepass; requesting `:app:assembleRelease --dry-run` failed because the
variant is absent. Default Cranelift release APK/AAB builds succeeded.
No production rights or distribution approval for the BUSL compilers is assumed.

The [pre-commit Python compatibility attempt](2026-11-16-singlepass-python-failure.txt)
failed validation with `exceptions proposal not enabled (at offset 0x18c8)`.
Its exact working-tree revision was not recorded; it predates the final core
benchmark addition. This is failed compatibility evidence on the same pinned
compiler/package, not an exact-revision passing test. The common Python benchmark
was not weakened, skipped, or given a Cranelift fallback to make Singlepass green.
Singlepass Python timings are unavailable. HTTP compatibility on Singlepass was
not tested; a core/WASI-fixture pass is not proof of WebC application compatibility.

LLVM was inspected, not built/benchmarked. Its 7.4.2 crate requires inkwell's
`llvm22-1-prefer-static` and LLVM 22. The selected Android NDK contains a **Windows
host clang 19.0.1**, not LLVM 22.1 libraries compiled for Android arm64. No matching
LLVM libraries were present in the inspected Android sysroot, and no llvm-config
was found on PATH. An Android LLVM cross-build and separate licensing decision
remain prerequisites. No LLVM timing, APK size or compatibility result is claimed.

## Clean artifact sizes

Both variants were packaged after `:app:clean`, which clears only generated app
build outputs, preserving Rust build caches and device data. A prior incremental
APK had old ZIP layout space (88,207,278 bytes versus 57,487,832 payload bytes);
it is not used for a compiler-size comparison.

| Artifact | Exact bytes | Signing / scope |
| --- | ---: | --- |
| Cranelift debug APK, Rust release | 57,629,631 | Debug signed; exact device-tested package |
| Singlepass debug APK, Rust release | 53,792,575 | Debug signed; development core/bridge tests only |
| Cranelift release APK | 52,510,168 | Unsigned; built, not installed or published |
| Cranelift release AAB | 33,439,184 | No release signing configured; built, not uploaded |

All artifacts contain **arm64 only**. APK/AAB sizes exclude downloaded Python,
Bash, Coreutils and server containers, installed data, compiled caches and store
delivery/split optimizations. AAB compressed upload bytes are not an installed APK
or a predicted Play download size. Android release execution has not been tested:
its WasmDroid native library SHA-256 matches the device-tested debug package, but
that does not establish whole-APK release behavior or Play policy acceptance.

## Reproduction and outstanding checks

With rustup Rust 1.95, JDK 21 and the SDK/NDK above, from the repo root:

```powershell
# Clean only this repository's generated Android app build directory.
.\android\gradlew.bat -p android :app:clean :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial $env:WASMDROID_TEST_DEVICE -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.EngineBridgeTest,dev.wasmdroid.app.CoreBenchmarkTest,dev.wasmdroid.app.RuntimeBenchmarkTest,dev.wasmdroid.app.RegistryProbeTest,dev.wasmdroid.app.HttpProbeTest' -TimeoutSeconds 600
.\android\gradlew.bat -p android :app:assembleRelease :app:bundleRelease -PwasmdroidAbis=arm64-v8a -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
# Non-production experiment, separately built and installed:
.\tools\test-android.ps1 -Serial $env:WASMDROID_TEST_DEVICE -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler singlepass -TestClass 'dev.wasmdroid.app.EngineBridgeTest,dev.wasmdroid.app.CoreBenchmarkTest' -TimeoutSeconds 600
```

CI for the prior HTTP revision `43ecdc5` passed all four jobs in
[run 43944813894](https://github.com/gatgserv/wasmdroid/actions/runs/43944813894).
The new workflow adds independent native compiler configurations; inspect the
submitted head's own checks rather than borrowing the previous green result.

Remaining: LLVM prerequisites/licensing and measurements if selected; actual
Singlepass application compatibility; release/minimum-API execution, 16 KiB device
validation, trusted persistent compiled-cache implementation and new-process warm
measurements; full transitive notice bundle and Play prerequisites. Product GUI,
general lifecycle/service/volumes/catalog and Stage 2 remain in the full backlog.
