# Search and Storage GUI: local verification

Matching tested source: `8e90295519d4845de551427060814d6d87303d6c`, author and
committer gatgserv. Checks ran on the frozen matching source immediately before
the local commit. The English `search-and-storage.html` conversation scheme was
presented in the preceding final response before these GUI changes. Its SHA-256
was `cc8fb83ca7ecfa594143d9fab837ab5e687a38492c2ca54a995aee0e7455ba29`.
No new visual was needed for implementing this already presented scheme.

## Implemented and reviewed scope

Store's single LazyColumn now includes typed advanced search status/query labels,
empty/failure/retry/cancel/load-more controls, retained-result identity and guarded
row actions. The unverified warning and separate downloaded-artifact list remain.
Artifact cards show exact supported metadata file sizes with an explicit unknown
fallback; this is not a dependency graph or physical allocation total.

Settings now observes fixed-category storage, partial/unavailable values, an
observed sum and separately available space. Its visible composition refreshes
without background polling. AppModel captures actual current owner identity,
invalidates on disconnect and closes the controller with its view-model lifetime.
The controller coalesces its observation while the owner retains real IO work.

Independent source review corrected:

- Fast input followed by IME Search must submit the latest actual draft even
  before recomposition, while preserving actual owner/loading admission.
- Android JSONObject(String) alone does not prove single-root consumption. The
  codec now uses JSONTokener, actual EOF and literal-NUL rejection; count/status/
  sum/category and numeric consistency are checked before publishing a snapshot.
- Positive bytes with zero counted files are rejected as an inconsistent frame.
- Storage copy/rows use concise user-facing counts and partial status. Failed
  observations use only static Other/Data/Failed report tokens, with no snapshot,
  raw error, address or pathname in the diagnostic report.

Source review found no further issues in the final Storage corrections,
Search and shared wiring. Rendered and physical acceptance remained pending.

## Executed checks

```powershell
.\.local\test-search-storage-gui.ps1
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
git diff 24c1a25 HEAD --name-only -- engine Cargo.lock
```

| Check | Actual result | Limit |
| --- | --- | --- |
| Pure host state suite |28/28 PASS,0.123s |18 existing registry state cases +10 new storage controller/formatter cases |
| Android compilation | PASS16s;33 tasks,10 executed | Main/Android-test Kotlin and resource task graph; native/generated bridge preparation UP-TO-DATE from the matching backend |
| Resource XML |15 files parse; no duplicate values keys | Static resource validation, not visual/translation/accessibility acceptance |
| Whitespace | PASS | CRLF normalization notices only |
| Rust source comparison | No changed engine/Cargo.lock paths | Earlier backend native evidence retains its own exact revision; no native suite rerun is claimed |

No Kotlin warnings were reported in the final build. All reported processes are
terminal. The host runner freshly compiled RegistrySearchState.kt,
StorageUsageState.kt and their two committed test files using Kotlin2.2.21
K2JVMCompiler, JVM17 target, stdlib2.2.21, app coroutines-core-jvm1.10.2,
annotations13, JUnit4.13.2 and Hamcrest-core1.3. The compiler process additionally
used script/daemon2.2.21, reflect1.6.10 and coroutines1.8.0. Official SDK36 stubs
resolved unused Android signatures; no stub method supplied a fake JSON result.

Execution explicitly selected only:

```text
org.junit.runner.JUnitCore
  dev.wasmdroid.app.RegistrySearchStateTest
  dev.wasmdroid.app.StorageUsageStateTest
```

StorageUsageCodecTest's seven definitions require actual Android JSONObject and
were not executed. Three RegistrySearchPresentationTest admission definitions
also only compile in this batch. Thus20 new definitions comprise10 host-executed
and10 compiled-only; the eighteen existing search cases were re-executed. No
Compose screenshot/interaction was executed by those pure host cases.

The JSON codec accepts only exact supported Int/Long representations and rejects
lossy floating-point/coerced counts. The formatter's unsigned-range test does not
imply that Android JSONObject parses every native u64 integer losslessly; such
unsupported frames fail explicitly instead of displaying a narrowed value.

Ignored logs: `.local/search-storage-gui-host.log` and
`.local/search-storage-gui-android.log`; runner `.local/test-search-storage-gui.ps1`.
The unchanged native backend retains separate results in
[its verification report](2026-11-17-search-storage-backend.md).

## Pending acceptance

No phone/test APK install, actual Android JSON execution, actual filesystem
accounting, running-guest write/rename/link races, real registry/offline/reconnect,
keyboard/rotation/provider/visual/TalkBack/large-font or full product acceptance
occurred. Persistent quota/default/eviction policy remains unselected. Final
CI/push/merge/release were not performed; device work remains deferred until
implementation completion. The overall goal remains active.
