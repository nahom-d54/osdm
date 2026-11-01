# Windows PowerShell 5.1's ConvertFrom-Json emits a JSON array as one pipeline
# object. Return its entries explicitly; @(... | ConvertFrom-Json) nests it.
function Read-NativeRegistrationBackup {
    param([Parameter(Mandatory)][string]$Path)
    $entries = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    foreach ($entry in $entries) {
        if ($entry.path -notmatch '^Software\\(Google\\Chrome|Chromium|Mozilla)\\NativeMessagingHosts\\app\.osdm\.capture$') { throw 'Unexpected registry path in backup.' }
        if ($entry.hasValue -isnot [bool] -or $entry.manifest -isnot [string] -or $entry.kind -notin @('String','ExpandString')) { throw 'Invalid registration backup.' }
        if ($entry.hasValue -and $entry.value -isnot [string]) { throw 'Invalid original registration value.' }
        Write-Output $entry
    }
}
