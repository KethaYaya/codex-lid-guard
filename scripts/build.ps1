$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$extensionRoot = Join-Path $projectRoot 'extension'

Push-Location $extensionRoot
try {
    if (-not (Test-Path (Join-Path $extensionRoot 'node_modules'))) {
        & cmd.exe /d /c npm install
        if ($LASTEXITCODE -ne 0) { throw "npm install failed with exit code $LASTEXITCODE" }
    }

    & cmd.exe /d /c npm test
    if ($LASTEXITCODE -ne 0) { throw "npm test failed with exit code $LASTEXITCODE" }

    & cmd.exe /d /c npm run package
    if ($LASTEXITCODE -ne 0) { throw "VSIX packaging failed with exit code $LASTEXITCODE" }
}
finally {
    Pop-Location
}

Write-Host "Built $extensionRoot\codex-lid-guard.vsix"
