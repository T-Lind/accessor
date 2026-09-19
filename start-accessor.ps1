param(
    [string]$WakeCode,
    [ValidateSet('codex', 'mock')][string]$Agent = 'codex',
    [switch]$Text,
    [switch]$Speak,
    [switch]$NoSpeak,
    [switch]$Plain,
    [switch]$Events,
    [ValidateSet('system','kokoro','cartesia','off')][string]$Tts,
    [string]$Workspace = $PSScriptRoot,
    [string]$CodexBin
)
$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath $PSScriptRoot
try {
    $accessorExe = Join-Path $PSScriptRoot 'target\release\acc.exe'
    if (-not (Test-Path -LiteralPath $accessorExe)) {
        & cargo build --release --locked --bin acc
        if ($LASTEXITCODE -ne 0) { throw 'Accessor build failed.' }
    }
    # Prefer the currently running desktop app's compatible executable over an
    # older npm CLI. Never change the user's global installation or configuration.
    if ($Agent -eq 'codex' -and -not $CodexBin) {
        $desktopBinRoot = Join-Path $env:LOCALAPPDATA 'OpenAI\Codex\bin'
        $runningCodex = Get-Process -Name codex -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -and $_.Path.StartsWith($desktopBinRoot + '\', [System.StringComparison]::OrdinalIgnoreCase) } |
            Select-Object -First 1
        if ($runningCodex) { $CodexBin = $runningCodex.Path }
    }
    $accessorArgs = @('run', '--agent', $Agent, '--workspace', $Workspace)
    if ($WakeCode) { $accessorArgs += @('--wake-code', $WakeCode) }
    if ($NoSpeak) { $accessorArgs += '--no-speak' }
    if ($Plain) { $accessorArgs += '--plain' }
    if ($Events) { $accessorArgs += '--events' }
    if ($Tts) { $accessorArgs += @('--tts', $Tts) }
    if ($Text) { $accessorArgs += '--text' }
    if ($Speak) { $accessorArgs += '--speak' }
    if ($CodexBin) { $accessorArgs += @('--codex-bin', $CodexBin) }
    if ($MyInvocation.ExpectingInput) {
        $input | & $accessorExe @accessorArgs
    } else {
        & $accessorExe @accessorArgs
    }
    $accessorExit = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $accessorExit
