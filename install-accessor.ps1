param([switch]$Speech, [switch]$Kokoro)
$ErrorActionPreference = 'Stop'
Push-Location -LiteralPath $PSScriptRoot
try {
    & cargo install --path . --bin acc --locked
    if ($LASTEXITCODE -ne 0) { throw 'Accessor installation failed.' }
    $cargoDirectory = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $accProgram = Join-Path $cargoDirectory 'bin\acc.exe'
    & $accProgram config set assets-dir $PSScriptRoot
    if ($LASTEXITCODE -ne 0) { throw 'Could not save speech-file location.' }
    if ($Speech) {
        & python scripts/setup_speech.py
        if ($LASTEXITCODE -ne 0) { throw 'Transcription setup failed.' }
    }
    if ($Kokoro) {
        & python scripts/setup_tts.py
        if ($LASTEXITCODE -ne 0) { throw 'Kokoro setup failed.' }
    }
    Write-Host 'Ready: acc setup; acc tts setup; acc -wakecode 29 speak'
    Write-Host 'If acc is not found, open a terminal with your Cargo bin directory on PATH.'
} finally {
    Pop-Location
}
