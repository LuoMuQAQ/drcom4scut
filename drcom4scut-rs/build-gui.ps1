# Build GUI with proper environment isolation
# This script ensures GNU toolchain is used

$ErrorActionPreference = "Stop"

Write-Host "Building drcom4scut GUI..." -ForegroundColor Green

# Save current PATH
$originalPath = $env:PATH

try {
    # Remove any Unix-style paths that might contain conflicting 'link' command
    $env:PATH = ($env:PATH -split ';' | Where-Object { 
        $_ -notmatch 'Git\\usr\\bin'
    }) -join ';'
    
    Write-Host "Cleaned PATH to avoid Unix link command conflict" -ForegroundColor Yellow
    
    # Build release version
    Write-Host "`nCompiling release build..." -ForegroundColor Cyan
    cargo +nightly-2026-09-06-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu
    
    if ($LASTEXITCODE -ne 0) {
        throw "GUI build failed with exit code $LASTEXITCODE"
    }
    
    Write-Host "`nGUI build completed successfully!" -ForegroundColor Green
    $exePath = "target\x86_64-pc-windows-gnu\release\drcom4scutGUI.exe"
    Write-Host "Output: $exePath" -ForegroundColor Cyan
    
    # Show file info
    if (Test-Path $exePath) {
        $fileInfo = Get-Item $exePath
        Write-Host "`nBinary size: $($fileInfo.Length) bytes" -ForegroundColor Yellow
        
        # Calculate SHA-256
        $hash = (Get-FileHash $exePath -Algorithm SHA256).Hash
        Write-Host "SHA-256: $hash" -ForegroundColor Yellow
    }
    
} finally {
    # Restore original PATH
    $env:PATH = $originalPath
}

Write-Host "`nBuild complete!" -ForegroundColor Green
