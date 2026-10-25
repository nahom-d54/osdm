param([string]$BrowserRegistryPath)
$ErrorActionPreference = 'Stop'
if ($BrowserRegistryPath -and $BrowserRegistryPath -notmatch '^HKCU:\\Software\\[A-Za-z0-9 ._\\-]+\\NativeMessagingHosts\\app\.osdm\.capture$') { throw 'Invalid custom native-host key.' }
$folder = Join-Path $env:APPDATA 'app.osdm.desktop\native-hosts'
$chromePath = Join-Path $folder 'chromium.json'; $firefoxPath = Join-Path $folder 'firefox.json'
$keys = @('HKCU:\Software\Google\Chrome\NativeMessagingHosts\app.osdm.capture','HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\app.osdm.capture','HKCU:\Software\Mozilla\NativeMessagingHosts\app.osdm.capture')
if ($BrowserRegistryPath) { $keys += $BrowserRegistryPath }
foreach ($key in $keys) {
    if (Test-Path -LiteralPath $key) {
        $value = (Get-Item -LiteralPath $key).GetValue('')
        if ($value -eq $chromePath -or $value -eq $firefoxPath) { Remove-Item -LiteralPath $key }
    }
}
Write-Output 'Removed matching OSDM native-host registrations. Download data and settings were not deleted.'
