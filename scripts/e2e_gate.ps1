# voice-cli e2e の JSON を読み、渡した基準がすべて PASS で errors が空なら終了コード 0、そうでなければ 1。
# windows-e2e.yml が、ランナーで判定できる基準だけで合否を決めるために使う。
# 使い方: pwsh scripts/e2e_gate.ps1 <e2e の JSON> <基準> [<基準> ...]   例: ... e2e-latency.json A1 analysis
param(
  [Parameter(Mandatory)][string]$Report,
  [Parameter(Mandatory, ValueFromRemainingArguments)][string[]]$Criteria
)

if (-not (Test-Path $Report) -or (Get-Item $Report).Length -eq 0) {
  Write-Host "::error::$Report がありません（e2e が JSON を出す前に止まった）"
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
  Write-Host "::error::通らなかったもの: $($failed -join ', ')"
  exit 1
}
exit 0
