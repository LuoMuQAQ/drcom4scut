# 现代化 HeroUI v3 + WebView2 实验包专用构建与打包脚本
# 隔离输出至 release/heroui-preview，严禁覆盖 v0.3.6 正式发布物。

param(
    [string]$BuildDirectory = (Join-Path ([System.IO.Path]::GetTempPath()) 'drcom-heroui-target'),
    [string]$PayloadDirectory = (Join-Path ([System.IO.Path]::GetTempPath()) 'drcom-heroui-payload')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "1. 前端构建：Vite + Tailwind CSS v4 + HeroUI 3.x" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
Set-Location (Join-Path $root 'frontend')
npm run build
if ($LASTEXITCODE -ne 0) { throw "前端构建失败" }
Set-Location $root

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "2. Rust 编译：drcom4scutGUI & uninstall (GNU Target)" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
$toolchain = 'nightly-2026-09-06-x86_64-pc-windows-gnu'
$target = 'x86_64-pc-windows-gnu'

$env:CARGO_TARGET_DIR = $BuildDirectory
$payload = $PayloadDirectory
New-Item -ItemType Directory -Force -Path $payload | Out-Null

Remove-Item Env:DRCOM_GUI_EXE -ErrorAction SilentlyContinue
Remove-Item Env:DRCOM_UNINSTALL_EXE -ErrorAction SilentlyContinue

& cargo "+$toolchain" build --release --target $target --bin drcom4scutGUI --bin uninstall
if ($LASTEXITCODE -ne 0) { throw "GUI 与卸载器构建失败" }

$out = Join-Path $env:CARGO_TARGET_DIR "$target\release"
Copy-Item (Join-Path $out 'drcom4scutGUI.exe') (Join-Path $payload 'drcom4scutGUI.exe') -Force
Copy-Item (Join-Path $out 'uninstall.exe') (Join-Path $payload 'uninstall.exe') -Force

$env:DRCOM_GUI_EXE = (Join-Path $payload 'drcom4scutGUI.exe')
$env:DRCOM_UNINSTALL_EXE = (Join-Path $payload 'uninstall.exe')

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "3. Rust 编译：drcom4scut-Setup (内嵌 Payload)" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
& cargo "+$toolchain" build --release --target $target --bin drcom4scut-Setup
if ($LASTEXITCODE -ne 0) { throw "Setup 构建失败" }

Remove-Item Env:DRCOM_GUI_EXE
Remove-Item Env:DRCOM_UNINSTALL_EXE

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "4. 收集产物至 release/heroui-preview 隔离目录" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
$previewRel = Join-Path $root 'release\heroui-preview'
New-Item -ItemType Directory -Force -Path $previewRel | Out-Null

$previewGui = Join-Path $previewRel 'drcom4scutGUI-heroui-preview.exe'
$previewSetup = Join-Path $previewRel 'drcom4scut-Setup-heroui-preview.exe'
$previewUninst = Join-Path $previewRel 'uninstall.exe'

Copy-Item (Join-Path $payload 'drcom4scutGUI.exe') $previewGui -Force
Copy-Item (Join-Path $out 'drcom4scut-Setup.exe') $previewSetup -Force
Copy-Item (Join-Path $payload 'uninstall.exe') $previewUninst -Force

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "5. 打包便携版 ZIP (drcom4scut-Portable-heroui-preview)" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
$guiBytes = [IO.File]::ReadAllBytes($previewGui)
$utf8 = New-Object Text.UTF8Encoding($true)
$readme = (Get-Content -LiteralPath (Join-Path $root 'resources/portable-README.txt') -Raw -Encoding UTF8).Replace('@VERSION@', "0.3.6-heroui-preview")

$files = [ordered]@{
    'drcom4scutGUI.exe' = $guiBytes
    'README.txt' = [byte[]]($utf8.GetPreamble() + $utf8.GetBytes($readme))
    'licenses/NOTICE.txt' = [IO.File]::ReadAllBytes((Join-Path $root 'resources/licenses/NOTICE.txt'))
    'licenses/GPL-3.0.txt' = [IO.File]::ReadAllBytes((Join-Path $root 'resources/licenses/GPL-3.0.txt'))
    'licenses/lucide-LICENSE' = [IO.File]::ReadAllBytes((Join-Path $root 'resources/lucide-LICENSE'))
    'licenses/core-LICENSE' = [IO.File]::ReadAllBytes((Join-Path $root '../drcom4scut_0.3.1/vendor/drcom4scut-0.3.2/LICENSE'))
}

function Sha256([byte[]]$bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose() }
}

$sums = foreach ($entry in $files.GetEnumerator()) {
    '{0}  {1}' -f (Sha256 $entry.Value), $entry.Key
}
$files['SHA256SUMS.txt'] = [Text.Encoding]::UTF8.GetBytes(($sums -join "`n") + "`n")

$zip = Join-Path $previewRel "drcom4scut-Portable-heroui-preview-win-x64.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }

$stream = [IO.File]::Open($zip, [IO.FileMode]::CreateNew)
try {
    $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in $files.GetEnumerator()) {
            $entry = $archive.CreateEntry("drcom4scut-Portable-heroui-preview/" + $file.Key, [IO.Compression.CompressionLevel]::Optimal)
            $entryStream = $entry.Open()
            try { $entryStream.Write($file.Value, 0, $file.Value.Length) }
            finally { $entryStream.Dispose() }
        }
    } finally { $archive.Dispose() }
} finally { $stream.Dispose() }

function FileSha256([string]$p) {
    $bytes = [IO.File]::ReadAllBytes($p)
    Sha256 $bytes
}

Write-Host "====================================================" -ForegroundColor Green
Write-Host "打包完成！产物清单：" -ForegroundColor Green
Write-Host "====================================================" -ForegroundColor Green
Write-Host ("GUI      : {0,10} 字节 | SHA256: {1}" -f (Get-Item $previewGui).Length, (FileSha256 $previewGui))
Write-Host ("Setup    : {0,10} 字节 | SHA256: {1}" -f (Get-Item $previewSetup).Length, (FileSha256 $previewSetup))
Write-Host ("Uninstall: {0,10} 字节 | SHA256: {1}" -f (Get-Item $previewUninst).Length, (FileSha256 $previewUninst))
Write-Host ("Portable : {0,10} 字节 | SHA256: {1}" -f (Get-Item $zip).Length, (FileSha256 $zip))
