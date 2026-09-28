# Install a prebuilt Accessor (acc) binary on Windows x64.
#
#   irm https://raw.githubusercontent.com/T-Lind/accessor/main/scripts/install.ps1 | iex
#
# Optional parameters when run from a saved copy:
#   -Version 0.34.3   install a specific release instead of the latest
#   -BinDir  DIR      install into DIR instead of %LOCALAPPDATA%\Programs\accessor
#
# This downloads only the small acc binary; the first run fetches ONNX Runtime,
# the speech model, and the Piper voice into Accessor's assets folder.
param(
    [string]$Version,
    [string]$BinDir
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = 'T-Lind/accessor'
$target = 'x86_64-pc-windows-msvc'

if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    throw 'Windows on ARM is not supported by the prebuilt binaries; build from source with cargo.'
}

if (-not $Version) {
    $latest = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    if (-not $latest.tag_name) { throw 'Could not find the latest release; set -Version to override.' }
    $Version = $latest.tag_name -replace '^v', ''
}
# Accept both `0.34.3` and `v0.34.3`; a bare `v` prefix would build a `vv` URL.
$Version = $Version -replace '^v', ''
if (-not $BinDir) {
    $BinDir = Join-Path $env:LOCALAPPDATA 'Programs\accessor'
}

$asset = "acc-$Version-$target.zip"
$base = "https://github.com/$repo/releases/download/v$Version"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("accessor-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "Downloading $asset ..."
    Invoke-WebRequest "$base/$asset" -OutFile (Join-Path $tmp $asset)
    Invoke-WebRequest "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')

    Write-Host 'Verifying checksum ...'
    $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { $_ -match [regex]::Escape(" $asset") }
    if (-not $line) { throw "No checksum for $asset in SHA256SUMS" }
    $expected = ($line -split '\s+')[0].ToLower()
    $actual = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) { throw "Checksum mismatch for $asset" }

    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    Copy-Item (Join-Path $tmp 'acc.exe') (Join-Path $BinDir 'acc.exe') -Force

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -notcontains $BinDir) {
        $newPath = if ([string]::IsNullOrEmpty($userPath)) { $BinDir } else { "$userPath;$BinDir" }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        Write-Host "Added $BinDir to your user PATH (open a new terminal)."
    }
    $env:Path = "$env:Path;$BinDir"
    Write-Host "Installed acc $Version to $BinDir\acc.exe"
    Write-Host 'Next:  acc doctor   (first run downloads the speech files)'
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
