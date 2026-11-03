# Reads the voice-cli e2e JSON; exits with 0 if all the given checks PASS and errors is empty, otherwise 1.
# Used by windows-e2e.yml to decide the result from only the checks that can be judged on a runner.
# Usage: pwsh scripts/e2e_gate.ps1 <e2e JSON> <check> [<check> ...]   for example: ... e2e-latency.json A1 analysis
param(
  [Parameter(Mandatory)][string]$Report,
  [Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Criteria
)

if (-not (Test-Path $Report) -or (Get-Item $Report).Length -eq 0) {
  Write-Host "::error::$Report is missing (e2e stopped before writing its JSON)"
  exit 1
}
$r = Get-Content $Report -Raw | ConvertFrom-Json
$failed = @()
foreach ($c in $Criteria) {
  $result = $r.$c.result
  Write-Host "$c`t$result"
  if ($result -ne 'PASS') { $failed += $c }
}
if ($r.errors.Count -gt 0) {
  Write-Host "errors`t$($r.errors -join '; ')"
  $failed += 'errors'
}
if ($failed.Count -gt 0) {
  Write-Host "::error::did not pass: $($failed -join ', ')"
  exit 1
}
exit 0
