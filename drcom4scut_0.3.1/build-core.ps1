$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = $PSScriptRoot
$source = Join-Path $root 'vendor\drcom4scut-0.3.2'
$destination = Join-Path $root 'src\Resources\drcom4scut.exe'
$staging = Join-Path $env:TEMP 'drcom4scut-v3-core-build'
$sdkArchive = Join-Path $staging 'npcap-sdk-1.15.zip'
$sdkUrl = 'https://npcap.com/dist/npcap-sdk-1.15.zip'
$sdkSha256 = '52c7b9fb4abee3ad9fe739bb545c3efe77b731c8e127122bdf328eafdae3ed4f'
$expectedCoreSha256 = 'ce79e117d14d172cb172a7d2a8adb2c638eb32d952db28d8ce04602eb5445ec2'
$sourceDateEpoch = '1788546015'
$rustToolchain = 'nightly-2026-09-06-x86_64-pc-windows-gnu'
$rustCommit = 'f248f4038796913873f11ca65b1b901e311c8dae'

function Get-Sha256([string] $Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $algorithm = [System.Security.Cryptography.SHA256]::Create()
        try {
            return ([System.BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
        }
        finally {
            $algorithm.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

if (-not (Test-Path -LiteralPath (Join-Path $source 'Cargo.lock') -PathType Leaf)) {
    throw "Vendored core source not found: $source"
}

Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $staging -Force | Out-Null
Get-ChildItem -LiteralPath $source -Force |
    Where-Object { $_.Name -ne 'target' -and $_.Name -ne 'Packet.lib' } |
    Copy-Item -Destination $staging -Recurse -Force

Invoke-WebRequest -Uri $sdkUrl -OutFile $sdkArchive
$actualSdkHash = Get-Sha256 $sdkArchive
if ($actualSdkHash -ne $sdkSha256) {
    throw "Npcap SDK hash mismatch. Expected $sdkSha256, received $actualSdkHash"
}

$sdkDirectory = Join-Path $staging 'npcap-sdk'
Expand-Archive -LiteralPath $sdkArchive -DestinationPath $sdkDirectory -Force
Copy-Item -LiteralPath (Join-Path $sdkDirectory 'Lib\x64\Packet.lib') -Destination (Join-Path $staging 'Packet.lib')

$rustDetails = (& rustc "+$rustToolchain" --version --verbose 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0 -or $rustDetails -notmatch [regex]::Escape("commit-hash: $rustCommit")) {
    throw "Required Rust toolchain is unavailable or unexpected: $rustToolchain ($rustCommit)"
}

$previousLibraryPath = $env:LIBRARY_PATH
$previousRustFlags = $env:RUSTFLAGS
$previousSourceDateEpoch = $env:SOURCE_DATE_EPOCH
try {
    $env:LIBRARY_PATH = $staging
    $env:RUSTFLAGS = ((@($previousRustFlags, '-C link-arg=-Wl,--no-insert-timestamp') |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }) -join ' ')
    $env:SOURCE_DATE_EPOCH = $sourceDateEpoch
    & cargo "+$rustToolchain" build `
        --manifest-path (Join-Path $staging 'Cargo.toml') `
        --release `
        --locked `
        --target x86_64-pc-windows-gnu
    if ($LASTEXITCODE -ne 0) { throw "Core build failed with exit code $LASTEXITCODE" }
}
finally {
    $env:LIBRARY_PATH = $previousLibraryPath
    $env:RUSTFLAGS = $previousRustFlags
    $env:SOURCE_DATE_EPOCH = $previousSourceDateEpoch
}

$built = Join-Path $staging 'target\x86_64-pc-windows-gnu\release\drcom4scut.exe'
if (-not (Test-Path -LiteralPath $built -PathType Leaf)) { throw "Core build output missing: $built" }
Copy-Item -LiteralPath $built -Destination $destination -Force

$version = (& $destination --version 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $version -ne 'drcom4scut 0.3.2') {
    throw "Core version validation failed: $version"
}

$hash = Get-Sha256 $destination
if ($hash -ne $expectedCoreSha256) {
    throw "Core reproducibility check failed. Expected $expectedCoreSha256, received $hash"
}
Write-Host "Built and validated $version ($hash)" -ForegroundColor Green
