param(
    [string]$Abis = 'arm64-v8a',
    [ValidateSet('true', 'false')][string]$RuntimeEnabled = 'false',
    [ValidateSet('debug', 'release')][string]$RustProfile = 'debug',
    [ValidateSet('cranelift', 'singlepass')][string]$Compiler = 'cranelift',
    [string]$NdkVersion = '28.2.13676358'
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } elseif ($env:ANDROID_SDK_ROOT) { $env:ANDROID_SDK_ROOT } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$ndk = Join-Path $sdk "ndk\$NdkVersion\toolchains\llvm\prebuilt\windows-x86_64\bin"
if (-not (Test-Path -LiteralPath $ndk)) { throw "Android NDK $NdkVersion is required under $sdk" }
$targets = @{ 'arm64-v8a' = 'aarch64-linux-android'; 'x86_64' = 'x86_64-linux-android' }
$output = Join-Path $repo 'android\app\build\generated'
$previousPath = $env:PATH
# Prefer rustup proxies over a standalone Rust MSI installation in PATH.
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
Push-Location $repo
try {
    # Only clear this task's generated bindings, never source or app data.
    $bindings = [IO.Path]::GetFullPath((Join-Path $output 'uniffi'))
    $expectedBindings = [IO.Path]::GetFullPath((Join-Path $repo 'android\app\build\generated\uniffi'))
    if ($bindings -ne $expectedBindings) { throw 'Unexpected bindings output path' }
    if (Test-Path -LiteralPath $bindings) { Remove-Item -LiteralPath $bindings -Recurse -Force }
    foreach ($abi in $Abis.Split(',')) {
        if (-not $targets.ContainsKey($abi)) { throw "Unsupported ABI: $abi" }
        $target = $targets[$abi]
        $androidClang = Join-Path $ndk "${target}28-clang.cmd"
        $targetKey = $target.Replace('-', '_')
        $variables = @{
            "CARGO_TARGET_$($targetKey.ToUpper())_LINKER" = $androidClang
            "CC_$targetKey" = $androidClang
            "AR_$targetKey" = (Join-Path $ndk 'llvm-ar.exe')
        }
        $previous = @{}
        try {
            foreach ($key in $variables.Keys) {
                $previous[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
                [Environment]::SetEnvironmentVariable($key, $variables[$key], 'Process')
            }
            $arguments = @('build', '--locked', '-p', 'wasmdroid-ffi', '--target', $target)
            if ($RustProfile -eq 'release') { $arguments += '--release' }
            if ($RuntimeEnabled -eq 'true') {
                $runtimeFeature = if ($Compiler -eq 'singlepass') { 'singlepass' } else { 'wasmer' }
                $arguments += @('--features', $runtimeFeature)
            }
            & cargo @arguments
            if ($LASTEXITCODE -ne 0) { throw "Rust build failed for $target" }
        } finally {
            foreach ($key in $previous.Keys) { [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process') }
        }
        $library = Join-Path $repo "target\$target\$RustProfile\libwasmdroid_ffi.so"
        $jni = Join-Path $output "jniLibs\$abi"
        New-Item -ItemType Directory -Path $jni -Force | Out-Null
        Copy-Item -LiteralPath $library -Destination (Join-Path $jni 'libwasmdroid_ffi.so')
        & cargo run --locked -p wasmdroid-bindgen -- generate --library $library --language kotlin --config engine/ffi/uniffi.toml --no-format --out-dir $bindings
        if ($LASTEXITCODE -ne 0) { throw 'UniFFI generation failed' }
    }
} finally { $env:PATH = $previousPath; Pop-Location }
