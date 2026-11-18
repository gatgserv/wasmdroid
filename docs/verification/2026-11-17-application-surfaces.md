# Combined applications, scoped shortcuts and seed data

Final source revision: `c655c43f3595b36eb0fbd97cb66746c063209120`.
Main source batch: `e5325cca2098c931fa1baa9e9d17909946f94bc8`.
The final revision adds only the volatile Activity visibility fence. `git diff
--quiet e5325cca2098c931fa1baa9e9d17909946f94bc8 HEAD -- engine` verified an identical
Rust tree; the exact-source native run below therefore covers the final engine.
Android compilation ran on the matching final source before its local commit.

## Implemented scope

The combined-app, home-widget and Quick Settings proposal was shown before GUI
implementation. My Apps now presents actual managed groups with Start missing
parts, Stop, owner-resolved Open, per-part data and scoped metadata removal.
Normal cards do not expose commands/ports/environment. Actual membership, birth
identity, revisions and generations select scope; descriptive labels do not.

Native standalone identity survives same-spec Dead replacement; removing and
recreating an actual ID cannot reuse it. A group target uses its durable creation
event and current membership. Metadata pages are bounded and exclude specs/env.
Owner state tokens include current worker generations, ownership, pending restart/
boot work and a durable Stop All epoch. Stopped tokens stay stable across idle
owner recreation. Selected widget/tile targets use independent versioned captures
and immutable PendingIntents; late selection/state changes fail at actual actor
admission. Configuration Save never starts a guest.

Read-only shortcut binds suppress cold recovery, while normal binds/rebinds keep
the configured path. Android binding filter groups remain tracked until the last
group unbinds. Start uses visible/unlocked foreground admission; Stop uses the
actual bound owner. Open/Review bind every captured intent field and exact owner
state, without accepting caller URLs. Volatile visibility state crosses lifecycle
and actor threads. Actual mutation milestones refresh surfaces; query/lifetime
notifications do not create a refresh-bind loop.

Optional inline catalog seeds have32-file/64KiB plaintext limits, exact UTF-8
length/SHA256, safe destinations and a canonical manifest receipt. The encoded
whole entry must also fit128KiB before catalog acceptance. Only unpublished newly
scoped datasets with matching receipts admit retries. Exclusive volume capabilities
remain held through no-replace file publication and atomic group metadata commit.
Changed/unknown user data is refused, never overwritten or deleted for retry.
An author-owned HTML example is included; it is not a verified package/database
recipe. Browser origin continuity still needs independent transport work.

## Executed local verification

```powershell
cargo test --locked -p wasmdroid-engine --test application_targets --test catalog_assets --test catalog_preparation --test catalog_stacks --test automation --test browser_identity --test stack_updates
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo fmt --all --check
.\android\gradlew.bat -p android :app:compileDebugAndroidTestKotlin -PwasmdroidRuntime=true -PwasmdroidAbis=arm64-v8a -PwasmdroidRustProfile=release --console=plain
git diff --check
```

| Exact-source native suite | Result |
| --- | --- |
| Application target lineage/paging/epoch |3/3 PASS |
| Durable run/stop authority |10/10 PASS |
| Browser identity |4/4 PASS |
| Seed integrity/admission/no-overwrite |12/12 PASS |
| Catalog preparation and seed recovery |5/5 PASS |
| Catalog structures and encoded entry budget |13/13 PASS |
| Stopped group updates |6/6 PASS |

Total **53/53 executed native regressions PASS** on sourcee5325cc. Final Clippy
PASS7.97s with no warnings; formatting/diff checks PASS. Earlier52 cases and an
affected18-case rerun were superseded by this exact-source53-case run.

Android used the existing JDK21/SDK/NDK arm64 toolchain. The batch first compiled
in37s (native20.64s). Final full source compilation passed24s (native20.42s,
matching generated bindings, main Kotlin and previously compiled instrumentation
sources;33 tasks/3 executed). The Activity visibility correction then passed in2s,
with main and instrumentation Kotlin compiled again. This is Android debug
compilation with release-profile Rust, not APK execution or release signing.
Default string XML parsed with438 unique keys and no duplicates.

## Findings and corrections

The initial native compile rejected u64 SQLite decoding; the control epoch now
reads signed SQLite integers and checks conversion. A new target fixture confused
the returned replacement-instance revision with its separate group revision;
the corrected case explicitly asserts rejection of the stale revision, then
removes using the actual current group revision. Production gates were preserved.

Independent source review corrected mixed binding-group retirement, process-random
tokens breaking cold controls, pending automation missing Stop, cold Open before
the UI snapshot, name-based endpoint revocation, and Stop disabled while Start
was pending. Open now waits for a current snapshot and endpoint lifetime compares
execution/port/path identity. Android definitions cover these boundaries separately
from real OS behavior; compilation alone does not prove their assertions.

## Unexecuted and remaining evidence

Seventeen new Android definitions compiled but **did not run**:15 shortcut selection/
configuration/admission/presentation cases, one actual native-owner cold state/
late-selection case and one endpoint lifetime case. The Unix-only seed symlink
case was not compiled/executed on the Windows host. Unix/Android directory sync
code compiled into the native library; Windows tests make no directory-flush or
power-loss durability claim.

No phone installation, launcher/tile/lockscreen/FGS test, rendered UI/TalkBack/
rotation, actual guest HTTP/group/descendant behavior, provider clearing, real
recipe/license acceptance, full suite/lint, push or CI was performed. Original
issues remain open. Full browser egress/atomic port reuse/stable origin, real
curated recipes/assets/licenses, hard resource enforcement, LAN/QR, friendly
reports and final independent/device acceptance remain in the active full goal.
Foreground notification logical-app/progress aggregation also remains open;
its existing owner-work counter must keep its lifecycle meaning.
