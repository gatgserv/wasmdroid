# Full Android implementation scope

This tracks the continuing owner goal: implement the Android product from the
plan and show screen schemes before GUI implementation. Stage 0 foundation alone
does not satisfy that goal. Docker and iOS remain deferred by the approved plan.
No release/merge/production action is implied by a completed local feature.
The owner selected English-only application UI, notifications, accessibility text
and future screen proposals; see [application language](design/application-language.md).

Android v1 uses Wasmer Registry as the sole active Store source, with observed
service-process RAM/CPU, optional timeout and separate per-app WebView profiles.
Hard RAM/CPU caps and complete WebView traffic control are deferred. The
current gate is the runtime-enabled internal APK and physical Android acceptance.

| Requirement | Current evidence | Remaining work / acceptance |
| --- | --- | --- |
| Rust monorepo, native Compose bridge, license | Foundation verification report | Keep builds reproducible and notices complete |
| Runtime-neutral serde API, driver/registry boundaries, endpoint | Initial contracts and regression tests | Full operations and capability enforcement |
| Public repository and CI | Approved baseline main, open PR #2; five jobs passed on pre-attribution-rewrite cd54068 | Validate each new submitted revision; leave merge to owner; see attribution mapping |
| Wasmer Registry Store and pinned resolution | Actual live GraphQL requests, hash/command/license metadata, existing query/download and warning, query-specific pagination and retry/cancel | Accept normal-mode search, download, create, start and stop on the connected phone. Check service/registry failure states, keyboard, rotation and accessibility. |
| Verified downloads and installed cache | CAS and graph transaction regressions, real Python/Bash/Coreutils/HTTP blobs, cancellable HTTPS, explicit CAS write budgets, leased removal/GC and native progress/cancel/retry/receipts | Physical/visual/independent acceptance and a separate persistent quota policy remain open. |
| WebC execution and dependencies | Actual Python tests on API 36 x86_64 and physical Android 15/API 35 arm64 | Generalized lifecycle driver/supervision |
| WASIX loopback server | Physical arm64 HTTP 200, private virtual port/root per instance; managed host port and forced-stop cleanup | Generalize beyond the fixed acceptance recipe |
| Runtime comparison and measurements | Physical Cranelift Python cold/warm and process peak RSS; clean unsigned release APK/AAB; Singlepass core-only measurements and Python rejection | LLVM build/license prerequisites and actual measurements if selected; release/min-API/16 KiB execution and persistent-cache warm starts |
| Product GUI designs before implementation | Interactive Store/My Apps/details/settings proposal presented before implementation; local Compose Store/My Apps/settings/details/configuration/output/WebView code now integrated | Physical visual/accessibility/rotation acceptance; complete remaining data/terminal flows |
| Install/start/stop/restart/remove lifecycle | Durable metadata/CAS/events, exclusive-session recovery; local worker-resident RuntimeDriver and common controller with private termination callbacks | Actual whole-group stop, port/volume persistence and lifecycle acceptance; earlier combined shared-process HTTP/CPU failures remain documented |
| Engine survives UI closure | Local common foreground service, private per-service workers, inherited engine ownership and Stop all notification implemented | Real UI-closure/death/recovery acceptance and background policy validation |
| Embedded web launch | Local WebView resolves actual loopback ports and selects a separate instance profile; denies device/file/content/JS-bridge access, gates unsupported providers, offers external browser and explicit provider-acknowledged profile erasure; native retained stable numeric origins/history for known/new assignments | Accept localStorage/cookie separation, data clearing, stop/retry and busy-port behavior. Full WebView traffic control remains deferred. |
| Volumes and data isolation | Private named-volume data operations, actual VolumeFs guest mounts, persistence/read-only/lease/traversal/symlink checks and journaled delete recovery | Actual provider/guest isolation and persistence across real guest restart |
| Occupied space and available storage | Bounded fixed-category logical-byte backend and scheme-backed Settings snapshot/partial/unavailable/refresh, distinct available space and metadata-backed package file sizes | Actual Android filesystem/running-guest/rendering/accessibility acceptance; persistent quota policy remains separate and unselected |
| Advanced mode | Native English Compose configuration form, private versioned next-run edits, live logs, native VT100/Canvas/IME terminal, virtual stdin/TTY and generation-scoped control IPC | Actual config/next-run/Python/Bash/SQLite/IME/rotation/accessibility acceptance |
| Events, logs, stats and user errors | Transactional paged events, private rotated live stdout/stderr, process RSS/CPU sampling and lifecycle-aware summaries; bounded allowlisted report preview and explicit fixed GitHub issue form after the English scheme | Actual live/rotation/resource/UI and browser-form acceptance; process counters include JVM/JIT and are not guest memory limits; raw logs/errors never enter the report |
| Compiled module cache | Local authenticated disk cache for production service workers, private per-install key, source/build/compiler/CPU/features binding and bounded eviction | Independent auth/eviction checks and real cross-process hit/warm-start acceptance; old benchmark remains memory-only |
| Network manager and sandbox | Private per-service TCP, explicit host publication, bounded drain, authenticated peer routes and transient Wi-Fi ingress; native-issued pre-spawn standalone outbound capability, actual default-Network bind/DNS host, explicit peer policy and complete native group-name reservation; scheme-backed identity/version-fenced default-off permission controls | Running handover/regrant and managed-group operator grants are integrated locally. Actual Android DNS/bind/VPN/stop/death and GUI acceptance, and unsupported scoped/transition address forms, remain. |
| No accounts, public packages, sandbox | Public-only registry policy, HTTPS, no host guest network or mounts | Enforce all requested guest permissions; prevent permissive fallback |
| Stage 2 backup/import/export | Local SAF data screen, atomic imports/exports, streaming per-volume tar.gz backup, bounded no-clobber restore and preserved old volume | Real provider/rotation/cancellation acceptance, independent restore regressions and multi-data-set UX |
| Restart/boot and execution timeout | Local standalone coordinator, durable authority, default-off boot, common-owner wiring and a configurable timeout with a blank unlimited setting | Verify settings/restart/crash/boot, positive timeout and unlimited execution on the physical phone. Hard RAM/CPU caps are deferred. |
| Stage 2 Compose subset/stacks | Local bounded definitions, public/imported-digest pins, protected data/private routes and common lifecycle; reviewed stopped reconciliation, worker metadata check and native editor after scheme; explicit missing-package download/review | Visual/SAF/accessibility and independent storage/transport/controller/real two-service acceptance |
| Stage 2 LAN sharing/QR | Actual worker-owned ServiceNetwork connector, selected Android Wi-Fi/FD binding, owner/worker cancellation fences and English default-off address/QR controls after their scheme; bounded local encoder and license assets | Physical Wi-Fi/peer HTTP/QR scan, interface/permission loss, stop/death/restart, visual/accessibility and independent actor acceptance; see lan-ingress design; compiled source does not establish those results |
| Stage 2 local package import | Local neutral streaming transactions, Wasmer-only inspection, UniFFI capability, common-owner retained SAF/preview/commit/cancel and separate native application creation after the screen scheme; shared WebC/Wasm service runtime and imported-group planning/update/worker pins; compilation only | Actual provider/rotation/accessibility/format/execution/selective stop and imported-group acceptance; see local-packages design and issue #6 |
| Stage 2 widget/tile | Native scoped widget/config/action/tile and current-owner lineage/state/selection guards implemented after the English scheme | Actual Android launcher, cold owner, unlock/FGS, pending automation, rotation and stale-control acceptance |
| Play distribution | Not submitted | Complete release artifact, notices/privacy/signing/account prerequisites, owner-authorized internal test and policy verdict |

Earlier signed catalog transport, media and recipe work remains documented in
[catalog updates](design/catalog-updates.md). It is outside Android v1 Store
acceptance.

All features require scope-matched tests. Targeted story checks remain distinct
from the final browser/real-stack acceptance suite. Unknown decisions or external
account prerequisites remain visible; they must not be replaced by invented policy.
The source-based [remaining-work audit and estimate assumptions](remaining-work.md)
separate missing implementation from compiled features awaiting final acceptance.
