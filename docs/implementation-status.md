# Full Android implementation scope

This tracks the continuing owner goal: implement the Android product from the
plan and show screen schemes before GUI implementation. Stage 0 foundation alone
does not satisfy that goal. Docker and iOS remain deferred by the approved plan.
No release/merge/production action is implied by a completed local feature.

| Requirement | Current evidence | Remaining work / acceptance |
| --- | --- | --- |
| Rust monorepo, native Compose bridge, license | Foundation verification report | Keep builds reproducible and notices complete |
| Runtime-neutral serde API, driver/registry boundaries, endpoint | Initial contracts and regression tests | Full operations and capability enforcement |
| Public repository and CI | Approved baseline main, open PR #2; five jobs passed on pre-attribution-rewrite cd54068 | Validate each new submitted revision; leave merge to owner; see attribution mapping |
| Registry search and pinned resolution | Actual live GraphQL requests, hash/command/license metadata | User-facing advanced search, offline/error UX |
| Verified downloads and installed cache | CAS and graph transaction regressions; real Python/Bash/Coreutils/HTTP blobs | Cancellation, quotas, manifests/index, GC, image removal |
| WebC execution and dependencies | Actual Python tests on API 36 x86_64 and physical Android 15/API 35 arm64 | Generalized lifecycle driver/supervision |
| WASIX loopback server | Physical arm64 HTTP 200, private virtual port/root per instance; managed host port and forced-stop cleanup | Generalize beyond the fixed acceptance recipe |
| Runtime comparison and measurements | Physical Cranelift Python cold/warm and process peak RSS; clean unsigned release APK/AAB; Singlepass core-only measurements and Python rejection | LLVM build/license prerequisites and actual measurements if selected; release/min-API/16 KiB execution and persistent-cache warm starts |
| Curated web catalog and recipes | Versioned validator; empty catalog | Actual Android-tested web applications, reviewed licenses, trustworthy catalog updates |
| Product GUI designs before implementation | Interactive Store/My Apps/details/settings proposal presented before implementation | Implement accessible, localized Compose screens and states |
| Install/start/stop/restart/remove lifecycle | Durable metadata/CAS/events, exclusive-session interrupted-state recovery; independent CPU-hot WASIX interruption | General RuntimeDriver/supervisor, real create/start/stop/restart/remove and port persistence |
| Engine survives UI closure | Not implemented | Foreground Service ownership, ongoing count and Stop all notification |
| Embedded web launch | Not implemented | Local URL construction, origin restriction, no Android JS bridge, browser option |
| Volumes and data isolation | Private named-volume data operations, persistence/read-only/lease/traversal/symlink checks and journaled delete recovery | Adapt capabilities into explicit guest mounts and verify persistence across real guest restart |
| Advanced mode | Contract/design only | Registry CLI packages, command/args/env/ports/mount editing, logs and interactive TTY |
| Events, logs, stats and user errors | Transactional paged durable events, progress callback and bounded probe output | Streaming/follow/since/tail, file rotation, runtime stats, friendly error/report flow |
| Compiled module cache | In-memory Wasmer default only | Private trusted cache keyed by runtime/compiler/features/platform; safe invalidation |
| No accounts, public packages, sandbox | Public-only registry policy, HTTPS, no host guest network or mounts | Enforce all requested guest permissions; prevent permissive fallback |
| Stage 2 backup/import/export | Not implemented | Android SAF, disposable restore checks, data preservation |
| Stage 2 restart/boot/resource limits | Models only | Supported Android background policies, limits, safe defaults and explicit Unsupported |
| Stage 2 Compose subset/stacks | Not implemented | Services/images/commands/env/ports/volumes/depends_on subset and dependency startup |
| Stage 2 LAN sharing/QR | Not implemented | Explicit opt-in, bind policy and user-visible controls |
| Stage 2 widget/tile and local package import | Not implemented | Scoped actions and validated .wasm/.webc import |
| Play distribution | Not submitted | Complete release artifact, notices/privacy/signing/account prerequisites, owner-authorized internal test and policy verdict |

All features require scope-matched tests. Targeted story checks remain distinct
from the final browser/real-stack acceptance suite. Unknown decisions or external
account prerequisites remain visible; they must not be replaced by invented policy.
