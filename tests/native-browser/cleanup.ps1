param([Parameter(Mandatory)][string]$Root)
$ErrorActionPreference = 'Stop'
$directory = [IO.Path]::GetFullPath($Root)
$parent = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Temp\opencode'))
if ([IO.Path]::GetDirectoryName($directory) -ne $parent -or [IO.Path]::GetFileName($directory) -notlike 'osdm-native-browser-*') { throw 'Not an owned native-browser test directory.' }
$marker = Join-Path $directory 'app-data\.osdm-native-test'
if (!(Test-Path -LiteralPath $marker) -or [IO.File]::ReadAllText($marker) -ne "OSDM native test v1`n") { throw 'Test ownership marker missing.' }
foreach ($process in Get-CimInstance Win32_Process) {
    if ($process.Name -in @('chrome.exe','firefox.exe') -and $process.CommandLine -and $process.CommandLine.IndexOf($directory, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
        taskkill.exe /PID $process.ProcessId /T /F | Out-Null
    }
}
# Profiles and fixtures are retained as local evidence, never recursively deleted.
