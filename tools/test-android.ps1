param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-zA-Z0-9_.:-]+$')][string]$Serial,
    [string]$Abis = 'x86_64',
    [ValidateSet('true', 'false')][string]$RuntimeEnabled = 'true',
    [ValidateSet('debug', 'release')][string]$RustProfile = 'debug',
    [ValidateSet('cranelift', 'singlepass')][string]$Compiler = 'cranelift',
    [ValidateRange(30, 600)][int]$TimeoutSeconds = 180,
    [ValidatePattern('^[a-zA-Z0-9_.$,]+$')][string]$TestClass = 'dev.wasmdroid.app.EngineBridgeTest',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } elseif ($env:ANDROID_SDK_ROOT) { $env:ANDROID_SDK_ROOT } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$adb = Join-Path $sdk 'platform-tools\adb.exe'
Push-Location $repo
try {
    if (-not $SkipBuild) {
        & ./android/gradlew.bat -p android :app:assembleDebug :app:assembleDebugAndroidTest "-PwasmdroidAbis=$Abis" "-PwasmdroidRuntime=$RuntimeEnabled" "-PwasmdroidRustProfile=$RustProfile" "-PwasmdroidCompiler=$Compiler"
        if ($LASTEXITCODE -ne 0) { throw 'Android build failed' }
    }
    & $adb -s $Serial get-state
    if ($LASTEXITCODE -ne 0) { throw "Selected device $Serial is unavailable" }
    foreach ($apk in @('android\app\build\outputs\apk\debug\app-debug.apk', 'android\app\build\outputs\apk\androidTest\debug\app-debug-androidTest.apk')) {
        & $adb -s $Serial install -r -t $apk
        if ($LASTEXITCODE -ne 0) { throw "APK installation failed on $Serial" }
    }
    $results = Join-Path $repo '.local\test-results'
    New-Item -ItemType Directory -Path $results -Force | Out-Null
    $safeSerial = $Serial -replace '[^a-zA-Z0-9_-]', '_'
    $stdout = Join-Path $results "$safeSerial.stdout.txt"
    $stderr = Join-Path $results "$safeSerial.stderr.txt"
    # Direct serial-targeted instrumentation avoids Gradle probing unrelated devices.
    $runner = Start-Process -FilePath $adb -ArgumentList @('-s', $Serial, 'shell', 'am', 'instrument', '-w', '-r', '-e', 'class', $TestClass, '-e', 'timeout_msec', ($TimeoutSeconds * 1000).ToString(), 'dev.wasmdroid.app.test/androidx.test.runner.AndroidJUnitRunner') -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    # Retain the process handle so Windows PowerShell can retrieve ExitCode after exit.
    $null = $runner.Handle
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not $runner.WaitForExit(1000)) {
        if ((Get-Date) -gt $deadline) {
            $runner.Kill()
            & $adb -s $Serial shell am force-stop dev.wasmdroid.app
            throw "WasmDroid instrumentation exceeded $TimeoutSeconds seconds; see $stdout"
        }
    }
    $runner.WaitForExit()
    $output = Get-Content -LiteralPath $stdout -Raw
    Write-Output $output
    $errors = Get-Content -LiteralPath $stderr -Raw
    if ($errors) { Write-Output $errors }
    $exitCode = $runner.ExitCode
    if ($null -eq $exitCode) { throw 'Unable to retrieve instrumentation process exit code' }
    $unexpectedSkip = $RuntimeEnabled -eq 'true' -and $output -match 'INSTRUMENTATION_STATUS_CODE: -[34]'
    if ($exitCode -ne 0 -or $output -notmatch 'OK \(\d+ tests?\)' -or $output -match 'FAILURES!!!|INSTRUMENTATION_FAILED' -or $unexpectedSkip) {
        throw "WasmDroid device tests failed; see $stdout"
    }
    Write-Output "WasmDroid instrumentation passed (ADB exit code $exitCode)."
} finally { Pop-Location }
