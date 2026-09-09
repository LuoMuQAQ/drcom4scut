# Package the versioned GUI produced by publish-setup.ps1. No user data is read.
param()
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$toml = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') -Raw -Encoding UTF8
if ($toml -notmatch '(?m)^version\s*=\s*"([^"]+)"') { throw 'Missing package version' }
$version = $Matches[1]
$release = Join-Path $root 'release'
$gui = Join-Path $release "drcom4scutGUI-$version.exe"
$guiBytes = [IO.File]::ReadAllBytes($gui)
if (![Text.Encoding]::ASCII.GetString($guiBytes).Contains("version=`"$version.0`"")) {
    throw 'Build the current version with publish-setup.ps1 before packaging.'
}
$utf8 = New-Object Text.UTF8Encoding($true)
$readme = (Get-Content -LiteralPath (Join-Path $root 'resources/portable-README.txt') -Raw -Encoding UTF8).Replace('@VERSION@', $version)
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
$zip = Join-Path $release "drcom4scut-Portable-$version-win-x64.zip"
$temporaryZip = "$zip.$([Guid]::NewGuid().ToString('N')).tmp"
$folder = "drcom4scut-Portable-$version/"
try {
    $stream = [IO.File]::Open($temporaryZip, [IO.FileMode]::CreateNew)
    try {
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($file in $files.GetEnumerator()) {
                $entry = $archive.CreateEntry($folder + $file.Key, [IO.Compression.CompressionLevel]::Optimal)
                $entryStream = $entry.Open()
                try { $entryStream.Write($file.Value, 0, $file.Value.Length) }
                finally { $entryStream.Dispose() }
            }
        } finally { $archive.Dispose() }
    } finally { $stream.Dispose() }
    $archive = [IO.Compression.ZipFile]::OpenRead($temporaryZip)
    try {
        if ($archive.Entries.Count -ne $files.Count) { throw 'Unexpected ZIP entry count' }
        foreach ($file in $files.GetEnumerator()) {
            $entry = $archive.GetEntry($folder + $file.Key)
            if ($null -eq $entry) { throw "Missing ZIP entry: $($file.Key)" }
            $source = $entry.Open()
            $buffer = [IO.MemoryStream]::new()
            try {
                $source.CopyTo($buffer)
                if ((Sha256 $buffer.ToArray()) -ne (Sha256 $file.Value)) { throw "ZIP mismatch: $($file.Key)" }
            } finally { $source.Dispose(); $buffer.Dispose() }
        }
    } finally { $archive.Dispose() }
    Move-Item -LiteralPath $temporaryZip -Destination $zip -Force
    Write-Host ("Portable {0} bytes  {1}" -f (Get-Item -LiteralPath $zip).Length, (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant())
} finally {
    if (Test-Path -LiteralPath $temporaryZip) { Remove-Item -LiteralPath $temporaryZip -Force }
}
