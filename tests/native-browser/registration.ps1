param(
    [Parameter(Mandatory)][ValidateSet('Install','Restore')][string]$Mode,
    [Parameter(Mandatory)][string]$StateFile,
    [string]$ChromiumManifest,
    [string]$FirefoxManifest
)
$ErrorActionPreference = 'Stop'
$root = [Microsoft.Win32.Registry]::CurrentUser
$options = [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames

if ($Mode -eq 'Install') {
    if (Test-Path -LiteralPath $StateFile) { throw 'Refusing to replace an existing registration backup.' }
    $registrations = @(
        @{ path='Software\Google\Chrome\NativeMessagingHosts\app.osdm.capture'; manifest=$ChromiumManifest },
        @{ path='Software\Chromium\NativeMessagingHosts\app.osdm.capture'; manifest=$ChromiumManifest },
        @{ path='Software\Mozilla\NativeMessagingHosts\app.osdm.capture'; manifest=$FirefoxManifest }
    )
    $snapshot = @()
    foreach ($registration in $registrations) {
        $missing = @(); $prefix = ''
        foreach ($part in $registration.path.Split('\')) {
            $prefix = if ($prefix) { "$prefix\$part" } else { $part }
            $probe = $root.OpenSubKey($prefix)
            if ($null -eq $probe) { $missing += $prefix } else { $probe.Dispose() }
        }
        $key = $root.OpenSubKey($registration.path)
        $hasValue = $false; $value = $null; $kind = 'String'
        if ($null -ne $key) {
            $hasValue = $key.GetValueNames() -contains ''
            if ($hasValue) {
                $value=$key.GetValue('', $null, $options); $kind=$key.GetValueKind('').ToString()
                if ($kind -notin @('String','ExpandString')) { $key.Dispose(); throw 'Existing native-host value has an unsupported type; no registration was changed.' }
            }
            $key.Dispose()
        }
        $snapshot += @{ path=$registration.path; manifest=$registration.manifest; hasValue=$hasValue; value=$value; kind=$kind; missing=$missing }
    }
    # Persist the complete original state before touching any key.
    $stream = [IO.File]::Open($StateFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $bytes=[Text.UTF8Encoding]::new($false).GetBytes((ConvertTo-Json -InputObject @($snapshot) -Depth 5)); $stream.Write($bytes,0,$bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    foreach ($entry in $snapshot) {
        $key=$root.CreateSubKey($entry.path)
        try { $key.SetValue('', $entry.manifest, [Microsoft.Win32.RegistryValueKind]::String) } finally { $key.Dispose() }
    }
    Write-Output 'Temporary current-user native-host registration installed.'
} else {
    if (!(Test-Path -LiteralPath $StateFile)) { throw 'Registration backup missing; refusing to guess the previous state.' }
    $snapshot = @(Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json)
    foreach ($entry in $snapshot) {
        if ($entry.path -notmatch '^Software\\(Google\\Chrome|Chromium|Mozilla)\\NativeMessagingHosts\\app\.osdm\.capture$') { throw 'Unexpected registry path in backup.' }
        $key=$root.OpenSubKey($entry.path, $true)
        if ($null -eq $key) { continue }
        try {
            $current=$key.GetValue('', $null, $options)
            if ($current -ne $entry.manifest) {
                # A later user/installer change wins. Never discard it.
                Write-Warning 'Native-host registration changed during the test; preserved the newer value.'
                continue
            }
            if ($entry.hasValue) { $key.SetValue('', $entry.value, ([Microsoft.Win32.RegistryValueKind]$entry.kind)) }
            else { $key.DeleteValue('', $false) }
        } finally { $key.Dispose() }
        $missing=@($entry.missing)
        [array]::Reverse($missing)
        foreach ($path in $missing) {
            $candidate=$root.OpenSubKey($path)
            if ($null -eq $candidate) { continue }
            $empty=$candidate.ValueCount -eq 0 -and $candidate.SubKeyCount -eq 0
            $candidate.Dispose()
            if ($empty) { $root.DeleteSubKey($path, $false) }
        }
    }
    Write-Output 'Matching temporary native-host registrations restored.'
}
