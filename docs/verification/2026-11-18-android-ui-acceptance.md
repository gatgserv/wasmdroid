# Android interface acceptance

Tested on 2026-11-18 from `2cbcd6d8e05c0fa4149409b485b57dd1338d6c66` on `codex/wasmer-registry-store`. Android builds used the Cranelift runtime, the Rust release profile, and debug APK signing.

| Test target | OS and runtime | Installed APK SHA-256 |
| --- | --- | --- |
| Physical Sony G8142 | Android 15, API 35, `arm64-v8a`, 4,096-byte pages, WebView `131.0.6778.200` | `153a5507655ac91a92d081e8f97b246dda8982a36e62a0f7aa15d8c6d1294b3c` |
| Isolated Android 9 AVD | API 28, `x86_64`, Google APIs system image | `d5ce755b8d74a8829cc0641069e412fb5cf8032964e6c28682e5085d1c899a02` |
| Isolated 16 KiB AVD | Android 16, API 36, `x86_64`, 16,384-byte pages measured on the running system, WebView `133.0.6943.137` | Same `x86_64` APK as Android 9 |

## Physical interface

| Path | Observed result |
| --- | --- |
| Store | The English accessible query field returned live Wasmer Registry results for `static-web-server`. A deliberately unmatched query rendered the empty state. The search query and results survived an orientation change. With Wi-Fi disabled and mobile data already off, a new query showed the connection error. Restoring Wi-Fi and tapping **Retry search** returned the results. |
| Create and start | From the installed `wasmer.io/python/python@3.13.20` Store item, the UI created a standalone service and connected a uniquely named volume. A small Python HTTP fixture was imported through Android's document picker and shown in **Manage data**. **Start** reached **Running**, with live worker RAM and CPU on the card, including a sample of `312.5 MiB` and `23.9%`. |
| Open, background, stop, start | Setting `dev.wasmdroid.web.port` to `8080` in the app's JSON tab exposed **Open** for the published TCP port. The fixture loaded in WebView and saved a `localStorage` marker. Going Home and reopening retained the same start counter and marker. **Stop** returned the card to **Start**. The next start incremented the persisted volume counter and retained the WebView marker. |
| Automatic start after reboot | The stopped fixture's **Start after device restart** switch was enabled, saved, and read back in the UI. The phone rebooted and waited for the first credential unlock. After unlock the WasmDroid worker was present before the activity was opened. In WebView the service read the volume and retained the previously saved marker. |
| Timeout | Entering `16` in **Stop after seconds (empty for no timeout)** allowed the app to reach **Running** and then stop automatically. The harness observed the stopped card 31.6 s after initiating Start, including launch and UI polling. Clearing the same field kept the worker and WebView available after an 18 s wait following **Running**. |
| Backup and restore | **Save backup** created a gzip archive through Android's document picker. The archive held the fixture script and counter `3`. A subsequent guest start changed the live data to `4`. **Restore backup** imported the saved archive, and the next guest start again showed `4`. The WebView marker was retained. |
| WebView provider capability | **Manage data** rendered **Unsupported provider** for single-application erase on WebView 131. Existing profile data remained readable. |
| Network default | With Internet and LAN both off, the Python fixture's `socket.getaddrinfo("example.com", 443, type=socket.SOCK_STREAM)` returned an ordinary error and the page displayed **Guest DNS unavailable**. |
| Internet enabled | **Network access** saved Internet on and LAN off. The service opened normally. Invoking the same DNS check led to an **Error** card. Android exit information reported `APP CRASH(NATIVE)`, status `6` (`SIGABRT`), for `dev.wasmdroid.app:runtime0`. The tombstone showed `FindStackTop()` in `art::Thread::InitStackHwm` on a JNI `AttachCurrentThread` path. [Issue #7](https://github.com/gatgserv/wasmdroid/issues/7) tracks the defect and separate DNS and raw TCP acceptance. |
| Wi-Fi inbound sharing | **Share on Wi-Fi** exposed the QR accessibility description and an HTTP address. The workstation, as a separate peer, fetched that address and received the fixture page. Disconnecting the phone's Wi-Fi removed the active sharing status and QR. After reconnecting, sharing stayed off and the UI offered **Start sharing**. |

## Compatibility and real transfer retry

- Android 9 rendered the real Store search for `static-web-server`. The UI started downloading `wasmer.io/edoardo/static-web-server@1.0.4`, reported **Downloading package…** with `15 / 5170 KiB`, and then showed **Installation failed** after the AVD's Wi-Fi and mobile data were switched off. After restoring the network, **Try again** completed and the UI displayed **Installed** with the verified-download receipt. The APK used the existing Wasmer Registry.
- The Android 9 Store remained scrollable, with readable installation text, a visible Download control, and navigation labels at a temporary system font scale of 1.30. The original font setting was restored. The on-screen keyboard's Search action submitted `static-web-server-wdui-absent`, showing the requested query and the empty state.
- On Android 9, `EngineBridgeTest`, `RegistryProbeTest` and `SupervisedHttpAcceptanceTest` passed 5/5 in 33.751 s. They cover the native library, a verified public WebC running actual Python, and two isolated supervised HTTP services on the same guest port.
- On the 16 KiB AVD, the same three classes passed 5/5 in 96.607 s. The installed APK also opened the Store UI. `WebProfileIsolationTest`, `BrowserProfileErasureTest` and `BrowserProfileCapabilityTest` passed 4/4 in 1.505 s. They exercised equal-origin cookie isolation, selected-profile erasure with a compatible provider, and the live-view exclusion.
- The 16 KiB AVD also exercised **Clear web data** from the actual app interface. After setting a WebView marker and stopping the Python service, the UI acknowledged the erase. The next start displayed a missing marker and incremented the existing named-volume counter.
- The 16 KiB AVD completed a guest OS reboot after the automatic-start setting was saved in the UI. The boot counter advanced and the app recorded `restore_completed`. A runtime worker was present before the activity was opened.
- From its WebView fixture, the guest set and read back a one-second `SO_RCVTIMEO` using the 16-byte `timeval` layout. A blocking read on a connected, idle TCP socket returned after 1.00 s. The probe verified the guest socket option independently of the app-wide service timeout.
- The 16 KiB AVD verified default-off denial for a numeric private TCP peer. Saving LAN-only permission and retrying that socket aborted the guest's worker. With Internet-only permission, a numeric public connection likewise aborted. Both produced `SIGABRT` on the ART JNI `AttachCurrentThread` path. These probes excluded guest DNS and extended the same [egress defect](https://github.com/gatgserv/wasmdroid/issues/7) to raw TCP.
- ARM64 APK alignment passed `zipalign -c -P 16 -v 4`. ELF inspection found `PT_LOAD` alignment of `0x4000` in the packaged `libandroidx.graphics.path.so`, `libjnidispatch.so`, and `libwasmdroid_ffi.so`. Live 16 KiB execution in this round was on the x86_64 emulator. [Android 16 KiB guidance](https://developer.android.com/guide/practices/page-sizes)
- `apksigner verify --verbose` passed for both debug APKs with one v2 signer per APK.

## Commands

```powershell
.\android\gradlew.bat -p android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PwasmdroidAbis=x86_64 -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <selected-avd> -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass <fully-qualified-class-list> -TimeoutSeconds 420
adb -s <selected-device> shell uiautomator dump <temporary-test-owned-xml>
adb -s <selected-device> reboot
adb -s <selected-device> shell am get-started-user-state 0
adb -s <selected-device> shell ps -A
adb -s <selected-avd> shell getconf PAGE_SIZE
apksigner verify --verbose <test-apk>
zipalign -c -P 16 -v 4 <arm64-test-apk>
llvm-readelf --program-headers <packaged-native-library>
```

The x86_64 build used a clean archive of the tested source revision. Interface actions used serial-targeted Android inputs and UI hierarchy or screenshot assertions. Fixture files entered the application through the Android document picker. Supplied host-side archive inspection and Android process-exit diagnostics complement the interface observations. Prior physical network and host checks are in the [network regression report](2026-11-18-network-regressions-arm64.md).

The physical fixture was stopped and its automatic-start switch was turned off and read back. The phone's original Wi-Fi and mobile-data settings were restored. The named test data and backup were retained for reproduction. Both isolated AVDs were shut down after testing.

## Remaining coverage

- Android 9 and 16 KiB runtime coverage used x86_64 emulator system images. This round leaves 16 KiB ARM64 runtime execution, a full screen-reader pass, and alternate-network or VPN guest recovery open. Guest access on the original Android network is blocked by [issue #7](https://github.com/gatgserv/wasmdroid/issues/7).
- Release signing remains pending. At the final CI read, [PR #2](https://github.com/gatgserv/wasmdroid/pull/2) was open at `23455dce2a8b2eef5fc22cf06426c81115f2f485`. Its successful and cancelled checks apply to that older submitted revision. Test source `2cbcd6d` has local evidence only.
