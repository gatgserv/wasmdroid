# WasmDroid 0.1.0 Google Play candidate

Source `1dd8c4ec28632ffcf9a51faffca6e9214fe438e1` defines the first ATG
Services SRL candidate. The release uses Android application ID
`ro.atgserv.wasmdroid`, version code `1`, and version name `0.1.0`. It hosts
the in-app [privacy policy](../privacy-policy.txt) under Settings. The new
application ID gives this install its own Android app storage. The public
`v0.1.0` tag preserves the tested application's build inputs, with the public
documentation prepared separately.

## Signed artifact

- File `WasmDroid-0.1.0.aab`, 71,422,467 bytes.
- SHA-256 `1d8da87888702cbb826e7ecf0fb17a68d7d331f284e937844206253c97dd4a80`.
- Upload signer `CN=WasmDroid Play Upload`, RSA 3072, SHA-256. The upload
  certificate and the private keystore are managed separately.
- Runtime-enabled `arm64-v8a` and `x86_64`, Rust release profile, Wasmer
  Cranelift. The manifest specifies Android API 28 minimum and API 37 target.
- `bundletool 1.18.3` validated the bundle structure and reported
  `PAGE_ALIGNMENT_16K`. `jarsigner -verify` validated the upload signature.
  The local upload certificate is self-signed.

## Verification

Android assembly and release lint passed. The new application and test IDs
passed 51 instrumentation tests on the Sony Xperia G8142, Android 15 API 35,
ARM64. The locally signed debug test installation was removed after the
run. The existing Sony installation under the earlier application ID was
preserved. 298 host engine and FFI tests, formatting, and Clippy passed.
The Play-signed device install, Android 9 and Android 17 execution, and
production review remain pending for this candidate.

```powershell
.\android\gradlew.bat -p android --offline --warning-mode all :app:bundleRelease :app:lintRelease :app:assembleDebug :app:assembleDebugAndroidTest '-PwasmdroidAbis=arm64-v8a,x86_64' -PwasmdroidRuntime=true -PwasmdroidRustProfile=release -PwasmdroidCompiler=cranelift
.\tools\test-android.ps1 -Serial <sony> -Abis arm64-v8a -RuntimeEnabled true -RustProfile release -Compiler cranelift -SkipBuild -TestClass 'dev.wasmdroid.app.EngineBridgeTest,dev.wasmdroid.app.ServiceConfigurationDraftTest,dev.wasmdroid.app.ServiceConfigurationControllerTest,dev.wasmdroid.app.ServiceConfigurationTest,dev.wasmdroid.app.NetworkPermissionControllerTest,dev.wasmdroid.app.LanSharingCoordinatorTest,dev.wasmdroid.app.BrowserSessionAuthorityTest' -TimeoutSeconds 360
cargo fmt --all --check
cargo test --locked --offline -p wasmdroid-engine -p wasmdroid-ffi
cargo clippy --locked --offline -p wasmdroid-engine -p wasmdroid-ffi --all-targets -- -D warnings
bundletool validate --bundle=WasmDroid-0.1.0.aab
bundletool dump config --bundle=WasmDroid-0.1.0.aab
jarsigner -verify WasmDroid-0.1.0.aab
```

## Download size

`bundletool get-size total` estimated 22,546,434 download bytes for the Sony's
ARM64, API 35, density 410 configuration. The full upload contains both ABIs
and 33,898,093 compressed bytes of native debug symbols. Play uses those
symbols for diagnostics and delivers the selected device's APKs separately.

## Play Console and website

The owner reported **Internal testing: Active** for version code 1.
Play showed the temporary unreviewed app name. Sony's Play Store returned
**Item not found** when opened directly by package ID. The installation
through an enrolled tester's opt-in link remains pending.

The prepared store title is **WasmDroid by ATG Services SRL**.
The owner published the [privacy policy](https://wasmdroid.atgserv.ro/privacy.html).
An unauthenticated HTTPS GET on November 19 returned HTTP 200 as HTML.
Every in-app policy paragraph was present on the page. The owner later reported
that Google Play accepted the version code 1 production submission for review.
The review outcome and production availability remain pending.

## R8 release 0.1.1

Source `22c35a3cf5496cc793c5a570c328681fce54d705` enables R8 code and resource
optimization for the release build and advances to version code 2. The
first optimized experiment failed at native init because JNA resolves
`Pointer.peer` by name. The final rules retain the JNA classes and generated
UniFFI bindings. The dependency's unused Java AWT window APIs account for
the four explicitly scoped desktop class exclusions.

The isolated ARM64 test build with those rules downloaded Blogbase 0.3.6 from
Store, started the local service, and served its home page over Sony Wi-Fi
with HTTP 200. The optimized instrumentation runner failed before executing
its suite because it depended on AndroidX and Kotlin classes removed by R8.
The live app test provided device evidence for the optimized native
runtime. The 51-test instrumentation result applies to version code 1.

The version code 2 release bundle assembled and passed release lint,
`bundletool validate`, and verification with the same upload certificate
as version code 1. The release bundle contains the R8 mapping and the
archived mapping matches it. The retained bundle is 67,151,589 bytes with
SHA-256 `909ea08e3c79a9af58e1534b8fb80cc39bab263171cf64215599d127c4bbd31f`.

For the same Sony configuration, `bundletool get-size total` gives
15,311,986 bytes for version code 2. The estimated download saves 7,234,448
bytes or 32.1% (decimal MB rounded to two places). Compressed DEX decreases
from 8,851,679 to 1,639,766 bytes. Version code 2 is staged locally.
