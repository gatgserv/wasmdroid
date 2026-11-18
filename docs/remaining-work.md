# Remaining implementation and acceptance

The 2026-11-18 owner decisions define the Android v1 internal APK. Store uses
public Wasmer Registry packages. Resource controls show observed service-process
RAM/CPU and allow an optional execution timeout. A blank timeout applies no
WasmDroid execution deadline. Embedded browser sessions use separate WebView
profiles per application. Hard RAM/CPU caps and complete WebView traffic control
are deferred.

## Android v1 work

| Work | Scope |
| --- | --- |
| Store integration | Route normal-mode search and download through the existing registry APIs, then create a service from the downloaded package. Keep the package compatibility warning and existing installer guards. |
| Android runtime build | Build a runtime-enabled internal APK with matching native library and generated Kotlin bindings for the connected phone's ABI. |
| Phone acceptance | Exercise public registry packages, service start/stop, RAM/CPU observation, configured and unlimited timeout, and WebView profile separation. Record exact tested revision, ABI, commands, outcomes and any limitations. |
| Integrated regressions | Run the relevant Rust and Android checks, triage findings and retest fixes on the revision used for the candidate. |

Lifecycle, standalone restart/boot, native configuration, stack import/edit,
local package import, volumes/mounts/SAF, live logs/TTY, compiled cache, browser
profile retention/erase, widgets/tile, notification summary, selected Wi-Fi
ingress/QR, and reviewed diagnostic reports have local implementations. Their
device, provider and visual acceptance remain tracked in the
[implementation status](implementation-status.md). LAN/report checkpoints are
recorded in [their verification report](verification/2026-11-17-lan-worker-reports.md).

## Deferred and separately tracked work

The earlier estimates for a WasmDroid curated catalog included selecting and
packaging apps, per-recipe verification, media, hosting and trust. Those
activities are outside the v1 Store scope. Stack acceptance may use suitable
existing public artifacts or internal test fixtures. A database candidate and
a shipped catalog entry are separate from verifying the ordinary Store path.

Hard RAM/CPU limits and complete WebView traffic control are deferred. Process
measurement, execution timeouts and WebView profile separation each have their
own acceptance criteria. Explicit changes to previously saved WebView origins
and persistent CAS quota policy remain separate from those criteria.

Guest Internet/LAN access uses separate default-off permissions, including
same-Network DNS with either grant. Standalone startup, running handover and
managed-group grants have local source implementations. Actual network
behavior remains subject to physical acceptance.

One selected phone sequences installation and device checks. Source fixes,
independent checks and documentation can proceed in parallel. Final timing
depends on check results and repairs. Submit for remote CI and evaluate
distribution separately following the authorized workflow.
