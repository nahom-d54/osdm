param([ValidateSet('chromium','firefox')][string]$Browser = 'chromium', [string]$Output = 'unpacked')
$ErrorActionPreference = 'Stop'
$destination = Join-Path $PSScriptRoot $Output
New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach ($name in @('background.js','rules.js','modifiers.js','options.html','options.js','icon.svg')) { Copy-Item (Join-Path $PSScriptRoot $name) (Join-Path $destination $name) }
Copy-Item (Join-Path $PSScriptRoot "manifest.$Browser.json") (Join-Path $destination 'manifest.json')
Write-Output "Load the extension from $destination"
