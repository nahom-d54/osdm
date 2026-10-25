param([Parameter(Mandatory)][string]$HostExecutable, [Parameter(Mandatory)][string]$ChromiumExtensionId, [string]$BrowserExecutable, [string]$BrowserRegistryPath, [switch]$LaunchBrowser)
$ErrorActionPreference = 'Stop'
if ($ChromiumExtensionId -notmatch '^[a-p]{32}$') { throw 'Supply the installed Chromium extension ID (32 letters a-p).' }
$exe = (Resolve-Path $HostExecutable).Path
if ([IO.Path]::GetExtension($exe) -ne '.exe') { throw 'Choose the native osdm-native-host.exe executable.' }
if ($BrowserExecutable) {
    $browser = (Resolve-Path $BrowserExecutable).Path
    if ([IO.Path]::GetExtension($browser) -ne '.exe') { throw 'Choose a native browser executable.' }
    if ($BrowserRegistryPath -and $BrowserRegistryPath -notmatch '^HKCU:\\Software\\[A-Za-z0-9 ._\\-]+\\NativeMessagingHosts\\app\.osdm\.capture$') { throw 'Custom registry path must be a per-user vendor NativeMessagingHosts\app.osdm.capture key.' }
} elseif ($BrowserRegistryPath -or $LaunchBrowser) { throw 'Supply -BrowserExecutable for custom registration or launch.' }
$folder = Join-Path $env:APPDATA 'app.osdm.desktop\native-hosts'
New-Item -ItemType Directory -Force -Path $folder | Out-Null
$common = @{ name='app.osdm.capture'; description='OSDM local download capture'; path=$exe; type='stdio' }
$chromium = $common.Clone(); $chromium.allowed_origins = @("chrome-extension://$ChromiumExtensionId/")
$firefox = $common.Clone(); $firefox.allowed_extensions = @('osdm@osdm.app')
$chromePath = Join-Path $folder 'chromium.json'; $firefoxPath = Join-Path $folder 'firefox.json'
[IO.File]::WriteAllText($chromePath, ($chromium | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText($firefoxPath, ($firefox | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
foreach ($key in @('HKCU:\Software\Google\Chrome\NativeMessagingHosts\app.osdm.capture','HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\app.osdm.capture')) { New-Item -Force $key | Out-Null; Set-Item $key -Value $chromePath }
if ($BrowserRegistryPath) { New-Item -Force $BrowserRegistryPath | Out-Null; Set-Item $BrowserRegistryPath -Value $chromePath }
$key = 'HKCU:\Software\Mozilla\NativeMessagingHosts\app.osdm.capture'; New-Item -Force $key | Out-Null; Set-Item $key -Value $firefoxPath
Write-Output 'Registered for the current user. Enable browser integration in OSDM; restart the browser.'
if ($BrowserExecutable) {
    [IO.File]::WriteAllText((Join-Path $folder 'custom-browser.json'), (@{ executable=$browser; registry_path=$BrowserRegistryPath } | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    if ($LaunchBrowser) { Start-Process -FilePath $browser }
}
