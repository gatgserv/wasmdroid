# Search state and storage observations: local verification

Matching tested source: `24c1a2530e3f75253ae6b77af658a8933acb5f18`, author and
committer gatgserv. Checks ran on the frozen source immediately before the local
commit. GUI implementation remains gated on showing its English scheme; this
checkpoint changes state/backend integration, not the Search/Storage rendering.
No push, remote CI, device execution, merge or release occurred.

## Independent corrections and scope

Search now retains exact submitted/displayed/draft query identities, bounded
pagination and explicit retry/cancel/owner-loss state. Read-review corrected a
dead Retry after Ready→disconnect→reconnect; retry availability is now explicit,
reconnect never fetches automatically and recovery retains its actual query.
Stale success/failure/next-page replies cannot mutate a newer request.

Storage captures trusted ownership and directory capability before releasing the
facade mutex, then scans fixed logical-file categories. Supervisor coalesces real
IO independently of observers and samples available filesystem bytes separately.
It does not implement a quota or total-app/physical allocation metric.

Independent source review found that cap-primitives4.0.3's Windows directory
enumeration reconstructs an ambient pathname. The final source refuses non-Unix
enumeration with Unavailable/platform_unsupported; Unix/Android uses fd-relative
nofollow operations. The earlier Windows nine-case pass preceded this finding
and is superseded: it never proved the rename/reparse race guarantee. The peer
read back the corrected gate and found no other concrete issue in scope.

## Executed and compiled checks

```powershell
cargo fmt --all
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine --test storage_usage
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi --lib
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo check --locked -p wasmdroid-engine --test storage_usage --target aarch64-linux-android --release
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

| Check | Result | Limit |
| --- | --- | --- |
| Corrected Windows storage suite |3/3 PASS,0.04s; compilation4.55s | Budget validation, retained ownership and explicit unsupported/no-enumeration behavior; not execution of Android counting |
| Engine and FFI library |19/19 +3/3 PASS; tests2.09s/0s, combined compilation6.68s | Selected libraries, including prior LAN regressions; not every integration suite |
| Selected-crate Clippy | PASS8.21s, warnings denied | Engine/FFI all targets on host |
| Android storage definitions | Compile PASS31.86s |11 Unix/Android definitions compile; none executed on Android |
| Integrated Android | PASS39s; native release24.01s;33 tasks,4 executed | Generated matching UniFFI plus main/Android-test Kotlin; no installation/test execution; no reported warnings |
| Formatting and whitespace | PASS | CRLF notices only |

The native storage source has12 definitions across platforms: two common,
one non-Unix-only and nine Unix-only. Host execution therefore covers three;
the Android compile covers eleven. Do not add both counts as distinct passed
tests or present compile-only cases as executed filesystem race evidence.

Freshly compiled `RegistrySearchState.kt` and `RegistrySearchStateTest.kt` also
passed **18/18 pure JVM cases,0.091s**. The ignored local runner was:

```powershell
.\.local\test-registry-search-models.ps1
```

It invoked Kotlin2.2.21 K2JVMCompiler with `-no-stdlib -no-reflect -jvm-target17`,
the two committed source files and a separate local class output. Model/runtime
classpath used the app's coroutines-core-jvm1.10.2, stdlib2.2.21, annotations13,
JUnit4.13.2 and Hamcrest-core1.3. Compiler process dependencies additionally used
script/daemon2.2.21, reflect1.6.10 and coroutines1.8.0. Execution used
`org.junit.runner.JUnitCore dev.wasmdroid.app.RegistrySearchStateTest` with no
Android stubs/adapters. This is a scoped host runner, not a new CI gate.

The eighteen cases cover request replacement, independent draft edits, deduped
append, exact failed-query/cursor retry, cancellation, owner loss/reconnect,
advanced disable, stale next-page completion, invalid/cyclic cursors and row/page/
retained-byte bounds. They establish the pure state semantics, not actual
registry/service/Android lifecycle behavior.

## Inputs and pending acceptance

Cargo.lock changed only the direct cap-primitives edge of wasmdroid-engine; its
existing4.0.3 version/checksum and all other packages stayed fixed. No new package
download or permissive filesystem fallback was introduced.

Logs are ignored local files: storage-usage-host-corrected.log,
search-storage-native-corrected.log, search-storage-clippy-corrected.log,
storage-usage-android-definitions.log, search-storage-android-final.log and
search-models-host-final.log under `.local`. All reported processes are terminal.

The English proposal's script syntax and eighteen element references were checked
locally. It uses illustrative values; actual layout/provider/device behavior was
not visually inspected. Search/Storage GUI is not implemented in this checkpoint.
Physical Android storage enumeration, guest rename/link/write races, free-space
observations, service reconnect/real registry/offline behavior, rotation,
accessibility, GUI acceptance, whole-product tests and final CI remain pending.
Hard resource enforcement/browser choices, useful verified catalog/DB artifacts
and guest outbound networking remain open; this checkpoint does not complete
the full goal.
