import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

test("Windows PowerShell decodes each backup entry rather than silently skipping a nested array", { skip: process.platform !== "win32" }, () => {
  const helper = fileURLToPath(new URL("../tests/native-browser/backup.ps1", import.meta.url)).replaceAll("'", "''");
  const script = `
    $ErrorActionPreference = 'Stop'
    . '${helper}'
    $file = [IO.Path]::GetTempFileName()
    try {
      $entries = @('Google\\Chrome','Chromium','Mozilla') | ForEach-Object {
        @{ path="Software\\$_\\NativeMessagingHosts\\app.osdm.capture"; manifest='C:\\test.json'; hasValue=$false; value=$null; kind='String'; missing=@() }
      }
      ConvertTo-Json -InputObject @($entries) -Depth 5 | Set-Content -LiteralPath $file
      $actual = @(Read-NativeRegistrationBackup -Path $file)
      if ($actual.Count -ne 3) { throw 'Backup entries were nested' }
      for ($i=0; $i -lt 3; $i++) { if ($actual[$i].path -ne $entries[$i].path) { throw 'Backup entry lost' } }
      $entries[0].path = 'Software\\Unrelated'
      ConvertTo-Json -InputObject @($entries) -Depth 5 | Set-Content -LiteralPath $file
      $rejected = $false
      try { $null = @(Read-NativeRegistrationBackup -Path $file) } catch { $rejected = $true }
      if (!$rejected) { throw 'Foreign registry path accepted' }
    } finally { Remove-Item -LiteralPath $file }
  `;
  const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
