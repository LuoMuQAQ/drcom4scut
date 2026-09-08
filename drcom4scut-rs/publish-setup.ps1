# 构建 Rust 安装版：GUI + uninstall + 嵌入二者的 Setup。
# 三个必踩坑：显式 nightly-gnu、--target gnu、CARGO_TARGET_DIR 纯 ASCII。

param(
    [string]$BuildDirectory = (Join-Path ([System.IO.Path]::GetTempPath()) 'drcom4scut-rs-target'),
    [string]$PayloadDirectory = (Join-Path ([System.IO.Path]::GetTempPath()) 'drcom4scut-rs-payload')
)

$ErrorActionPreference = 'Stop'
if ($BuildDirectory -match '[^\x00-\x7F]' -or $PayloadDirectory -match '[^\x00-\x7F]') {
    throw 'Use ASCII-only -BuildDirectory and -PayloadDirectory paths, e.g. C:\drcom-build and C:\drcom-payload'
}
$env:CARGO_TARGET_DIR = $BuildDirectory
$toolchain = 'nightly-2026-09-06-x86_64-pc-windows-gnu'
$target = 'x86_64-pc-windows-gnu'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root
$toml = Get-Content -Raw -Encoding UTF8 (Join-Path $root 'Cargo.toml')
if ($toml -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Missing package version' }
$version = $Matches[1]

$payload = $PayloadDirectory
New-Item -ItemType Directory -Force -Path $payload | Out-Null

Write-Host 'Building GUI and uninstall...'
Remove-Item Env:DRCOM_GUI_EXE -ErrorAction SilentlyContinue
Remove-Item Env:DRCOM_UNINSTALL_EXE -ErrorAction SilentlyContinue
& cargo "+$toolchain" build --release --target $target --bin drcom4scutGUI --bin uninstall
if ($LASTEXITCODE -ne 0) { throw 'cargo build GUI/uninstall failed' }

$out = Join-Path $env:CARGO_TARGET_DIR "$target\release"
Copy-Item (Join-Path $out 'drcom4scutGUI.exe') (Join-Path $payload 'drcom4scutGUI.exe') -Force
Copy-Item (Join-Path $out 'uninstall.exe') (Join-Path $payload 'uninstall.exe') -Force

$env:DRCOM_GUI_EXE = (Join-Path $payload 'drcom4scutGUI.exe')
$env:DRCOM_UNINSTALL_EXE = (Join-Path $payload 'uninstall.exe')

Write-Host 'Building setup with embedded payload...'
& cargo "+$toolchain" build --release --target $target --bin drcom4scut-Setup
if ($LASTEXITCODE -ne 0) { throw 'cargo build setup failed' }

$rel = Join-Path $root 'release'
$relSetup = Join-Path $rel 'setup'
New-Item -ItemType Directory -Force -Path $rel | Out-Null
New-Item -ItemType Directory -Force -Path $relSetup | Out-Null
$gui = Join-Path $rel "drcom4scutGUI-$version.exe"
Copy-Item (Join-Path $payload 'drcom4scutGUI.exe') $gui -Force
try {
    Copy-Item $gui (Join-Path $rel 'drcom4scutGUI.exe') -Force
} catch {
    Write-Host "Could not replace release\drcom4scutGUI.exe; published $gui"
}
$setup = Join-Path $relSetup "drcom4scut-Setup-$version.exe"
Copy-Item (Join-Path $out 'drcom4scut-Setup.exe') $setup -Force
Remove-Item Env:DRCOM_GUI_EXE
Remove-Item Env:DRCOM_UNINSTALL_EXE

function Sha256Of([string]$path) {
    (Get-FileHash -Algorithm SHA256 -Path $path).Hash.ToLowerInvariant()
}

Write-Host ("GUI     {0}  {1}" -f (Get-Item $gui).Length, (Sha256Of $gui))
Write-Host ("Setup   {0}  {1}" -f (Get-Item $setup).Length, (Sha256Of $setup))
Write-Host ("Uninst  {0}  {1}" -f (Get-Item (Join-Path $payload 'uninstall.exe')).Length, (Sha256Of (Join-Path $payload 'uninstall.exe')))
