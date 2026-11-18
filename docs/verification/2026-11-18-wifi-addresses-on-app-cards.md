# Wi-Fi addresses on application cards

On Sony, the reported WordPress service was running with no TCP publications
in its saved configuration. A running guest needs an actual published web
port before the owner can provide an **Open** action or start Wi-Fi sharing.

Source `8984d4694a64ab5adda71c24ccda92ad3640f8ab` makes **Configure** available
in normal mode on stopped standalone applications. The first TCP publication
defaults to web access and an automatically allocated phone port. The form
leaves the application port empty, persists the selected web port in the
service definition, and retains explicit phone bindings.

The **My apps** card reads the owning service's current Wi-Fi sharing status
and renders the active IPv4 and IPv6 URLs independently of the QR code.
The dialog shows the text addresses above the QR and retains its copying
and sharing controls. A published loopback port alone never supplies the
Wi-Fi address displayed on the card.

## Build and automated checks

The debug build targets Android API 37 with API 28 minimum, with runtime-enabled
ARM64 and x86_64 libraries built using release-profile Cranelift.

- App APK SHA-256 `3e22303c82aaba9e2ece35b648bc0ab94d87efe042475d8ff92dcdf16708437e`.
- Instrumentation APK SHA-256 `73def018035618340a90d18b8b730b675f18075dbcc023575d2de78cba2ea105`.
- The app APK passed signature and 16 KiB ZIP alignment checks.
- Android 17, API 37, x86_64, project test emulator. **52/52** targeted tests
  passed. These cover web port persistence, automatic phone port selection,
  address display eligibility, QR independence and sharing lifecycle.
- Host formatting, **297/297** engine and FFI tests, Clippy and Android
  lint passed.

```powershell
.\android\gradlew.bat -p android --warning-mode all --offline :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial emulator-5596 -Abis x86_64 -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.ServiceConfigurationDraftTest,dev.wasmdroid.app.ServiceConfigurationTest,dev.wasmdroid.app.ServiceConfigurationControllerTest,dev.wasmdroid.app.LanQrEncoderTest,dev.wasmdroid.app.LanSharingCoordinatorTest,dev.wasmdroid.app.LanNetworkSelectionTest,dev.wasmdroid.app.LanEnableDispatchTest,dev.wasmdroid.app.LanStartIntentTest' -TimeoutSeconds 360
cargo fmt --all --check
cargo test --locked -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
```

The emulator's live service form confirmed that adding the first TCP
publication leaves the application port blank, enables **Use for web access**,
and selects **Choose phone port automatically**. With an active relay, the
running static-server card showed its Wi-Fi URL. The sharing dialog showed
the same URL and a readable QR element. The card removed the URL after
**Stop sharing**.

## Confirmed Stop with a nonzero guest exit

The Sony's previous WordPress runs exited with code 127 after owner stop
intent. The saved output includes repeated PHP `Failed to poll event` messages
and a termination signal. The owner had persisted a terminal state.

The combined build from `e338858a4a1dc6d1591206de854acda3c43fe467` uses a
worker completion acknowledgement of the Stop request in addition to the
supervisor's matching owner intent. A nonzero guest exit with both signals
is displayed as **Stopped**. The exit code and guest output remain
available. A guest failure without the acknowledgement retains its previous
failure presentation.

- App APK SHA-256 `5ad2f2be08f1667d093a0449bb391051d502030a7f7bc48bbacbe5e907bf1372`.
- Instrumentation APK SHA-256 `a06b4b4944aeeaa57521517f27ec986632b2d35efd20a5bfb46c300fe16598bd`.
- Lint, package signature and 16 KiB ZIP alignment passed again.
- The combined APK passed **56/56** selected tests on both the Sony G8142
  (Android 15, API 35, ARM64) and the Android 17 x86_64 emulator. These are
  the previous selected classes plus `dev.wasmdroid.app.ServiceFailureTest`.
- The rebuilt APK was installed on the Sony. The user had a running
  WordPress instance during interactive verification, so the remaining live
  checks used the project's dedicated emulator. The real WordPress
  re-run, Android 9 and remote CI remain outside this run.

The virtual Wi-Fi peer reached the advertised IPv4 relay, which returned
HTTP 404 for the static fixture's root. The fixture had no index file.
The follow-up request for a named file did not yield an HTTP 200 in this
static-fixture run. After stopping sharing, the URL disappeared. Stopping
the fixture itself committed an exited state with the existing expected-stop
reason when no guest status reached the owner.

## Live output and stable data view

On the Sony, the active WordPress service had no TCP publications. A recent
segment of its rotating live output showed repeated PHP
`Failed to poll event` messages while the owner still reported it running.
That running state alone did not establish working HTTP.

Source `ea4aa0e9b1e5d3b571b623750b348301de1e824e` exposes **Output** on started
standalone services in normal mode and reports **No ports published** for a
running service with an empty publication list. The browser data panel still
refreshes the current profile status, but only presents the progress bar
for initial loading or actual erasure.

- App APK SHA-256 `335aad02ab3c08286bd3e5e21ecb4f373f6ba68dfb2a2f1d4fbf60d81245bdc4`.
- Instrumentation APK SHA-256 `a06b4b4944aeeaa57521517f27ec986632b2d35efd20a5bfb46c300fe16598bd`.
- The Android 17 x86_64 emulator passed **68/68** selected tests. The
  previous Android set was extended with `dev.wasmdroid.app.BrowserDataControllerTest`.
- Android lint, APK signature and 16 KiB ZIP alignment passed. Native
  production code was identical to the earlier verified sources.
- An isolated instance of `wasmer.io/john5james6653/wordpress-woo@0.3.1`
  on the project emulator started the PHP server with application port
  8080 and automatic phone port allocation. The initial and request-time
  output had no poll errors. The Android 17 Wi-Fi peer reached the advertised
  URL and got HTTP 500 for `GET /`, confirmed in the guest PHP journal.
  This establishes the HTTP transport, while application setup remains open.
- The test opened live **Output** in normal mode. In a separate view check,
  **Manage data** stayed stable through three observations across periodic
  profile checks. The test issued **Stop** on the isolated WordPress instance
  after startup and confirmed the stored exit code 127 and the expected
  owner-stop reason, then removed the test-owned instance.
- The final presentation APK ran on the emulator. The previous combined build
  remained on Sony during the user's running WordPress session.

### WordPress database requirement

The installed public WebC v3 artifact for
`wasmer.io/john5james6653/wordpress-woo@0.3.1` was copied from the test
emulator's content store, checked against its recorded SHA-256 and read with
`webc` 12.0.1. The bundled `wp-config.php` reads `DB_HOST`, `DB_PORT`,
`DB_NAME`, `DB_USERNAME` and `DB_PASSWORD`. The bundled
`/app/wasmer/install.php` requires the same keys and connects through
`mysqli` with the TLS flag. The application configuration uses the same
MySQL client flag. There is no bundled `wp-content/db.php` SQLite drop-in.
Only the presence of those variable names was checked in the existing Sony
service, and none were present.

The HTTP 500 was reproduced without these database settings. A TLS-capable
MySQL-compatible database and successful initialization were not exercised
in this test, so successful WordPress application setup remains unverified.
