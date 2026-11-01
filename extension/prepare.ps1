param([ValidateSet('chromium','firefox')][string]$Browser = 'chromium', [string]$Output = 'unpacked')
$ErrorActionPreference = 'Stop'
$destination = Join-Path $PSScriptRoot $Output
& node (Join-Path $PSScriptRoot 'prepare.mjs') $Browser $destination
if ($LASTEXITCODE -ne 0) { throw 'Extension preparation failed. Use a fresh output folder and install dependencies with pnpm install.' }
Write-Output "Load the extension from $destination"
