# Store release sorting, dual-stack Wi-Fi and retained failures

The runtime-enabled APK was built from
`e2f23aafb0f435881940bdfa7b6c16ae1edc84c8` on `codex/android-api-37`.
It targets API 37, supports API 28 and includes ARM64 and x86_64 native
libraries built with the release-profile Cranelift runtime.

| Artifact | SHA-256 |
| --- | --- |
| App | `d6a040248fa708b17ebf0d5ccf0ddcbd7956a269f1254c3963d6d3686de15cc3` |
| Instrumentation | `d65e1ae8dea5bd2adaaa618f7480d916735a23b875f3018f28f821de9d4cbddd` |

The app APK passed signature and 16 KiB ZIP alignment verification.
Android lint found no issues. The existing Gradle 11 AGP visibility
deprecation remains. Host formatting, Clippy with warnings denied and all
295 engine and FFI tests passed. The Sony passed all 85 native ARM64 driver
tests under the debug app's isolated cache. The dual-stack tests cover atomic
port reservation, both private guest families and mapped-address restrictions.

## Physical Sony G8142

The phone ran Android 15 (API 35) on ARM64. The app's
`wasmer.io/wasmer/static-web-server@2.44.0` fixture used a read-only web root
and one declared, published TCP guest port. A laptop on the local network
made IPv4 HTTP requests, and Sony made IPv6 HTTP requests to its selected
Wi-Fi address. Both requests required the exact test page.
The two guest configurations and the stop-sharing check first ran on
`e0300b0eded355e3c9763c5e7de3b4fa5574e5ef` (APK SHA-256
`2c6afe0dd3ff97926eaff3b6b929dea95bd2dc6e764dd1c6e64d9e093e93231a`).

| Guest configuration | Wi-Fi ingress result |
| --- | --- |
| `--host 127.0.0.1` | Default sharing served the page through both relay addresses. The IPv4 client ran on the workstation, and the IPv6 client ran on Sony. |
| `--host ::` | A new generation served the page to both clients using the same TCP port on the IPv4 and IPv6 endpoints. |
| **Stop sharing** | Both previously advertised addresses stopped serving the page. The guest remained running, and reopening the dialog kept sharing off for that generation. |

The final offset-parser APK was installed on Sony and the `--host ::`
HTTP checks passed again over both advertised families. The official fixture
was left running with default Wi-Fi sharing.

The physical IPv6 link exposed optimistic addresses in Android's
`LinkProperties`. Selection accepts an assigned optimistic address and
still rejects tentative-only or duplicate addresses. The listener binds the
observed Wi-Fi address, and IPv6 source admission is restricted to its on-link
prefix. [RFC 4429](https://www.rfc-editor.org/rfc/rfc4429.html#section-1.3)

The workstation's IPv6 binding on its local Ethernet adapter was disabled.
The Sony-to-Sony IPv6 request verifies the phone's IPv6 listener and forwarding
path. A separate IPv6 peer on the LAN was unavailable during this test.

In the live Store on Sony, selecting **Newest release** displayed the
official Static Web Server release first, with its release date and
**Tested on Android** label. With the older
`wasmer.io/baran/static-web-server@1.0.5` configuration, a new failed run
showed the exit code and retained document-root error on the service card.
**Output** opened the journal in normal mode. Returning to **My apps**
displayed the same failure again.

## Android instrumentation and permission

- Sony, API 35, ARM64. **61/61** registry, sharing, descriptor, QR and
  retained-error instrumentation tests.
- Android 17, API 37, x86_64. **61/61** of the same tests. At `e0300b0`, the real app left
  default sharing off when local network permission was denied and advertised
  its address after the permission was granted through Android's UI. The
  harness restored the emulator's original permission state.
- Android 9, API 28, x86_64. **64/64**, including the native bridge. The prior
  APK exposed that Android 9's ISO instant parser rejected numeric UTC offsets.
  The final build uses an offset-aware parser and preserves subsecond ordering.

## Commands

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-device> -Abis <selected-abi> -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <selected-classes> -TimeoutSeconds 360
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
cargo test --locked --offline --target aarch64-linux-android --release -p wasmdroid-wasmer-local --lib --no-run
adb -s <selected-phone> shell "run-as dev.wasmdroid.app sh -c '<isolated-native-test-command>'"
apksigner verify --verbose <app-apk>
zipalign -c -P 16 -v 4 <app-apk>
```

The Store and share actions used the accessibility hierarchy. The workstation
and Sony exercised the separately advertised HTTP URLs with `curl -4` and
`curl -6` respectively. The native test binary and its temporary cache were
removed after execution. This source was tested locally.
