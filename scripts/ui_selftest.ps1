# windows-ui.yml の画面の通しの試験を 1 回動かす。アプリを --ui-selftest で起動し、ui-report.json が書かれて終わるのを待つ。
# 終了コードはアプリのもの（0 が成功）。180 秒で終わらなければ止めて 1。
# 使い方: pwsh scripts/ui_selftest.ps1 <出力のディレクトリ> [-Exe <アプリの exe>]   入力は vowel150.wav（先に voice-cli synth で作る）
# -Exe を省くと target\release\voice-shift-scope.exe を起動する。release.yml は展開した配布物の exe を渡す
param([Parameter(Mandatory)][string]$OutDir, [string]$Exe = 'target\release\voice-shift-scope.exe')

New-Item -ItemType Directory -Force "$OutDir\saved", "$OutDir\config" | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$env:VOICE_SHIFT_SCOPE_CONFIG_DIR = "$OutDir\config"
$env:VOICE_SHIFT_SCOPE_TEST_SAVE_DIR = "$OutDir\saved"
$p = Start-Process $Exe -ArgumentList '--input-wav', 'vowel150.wav', '--ui-selftest', $OutDir -PassThru
Start-Sleep -Seconds 20
# 止まったときの手掛かりに、20 秒後の画面を撮る
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
[System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$bmp.Save("$OutDir\screen-20s.png")
Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Format-Table -AutoSize Id, StartTime | Out-String | Set-Content "$OutDir\webview-processes.txt"
if (-not $p.WaitForExit(160000)) {
  Stop-Process -Id $p.Id -Force
  Get-Content "$OutDir\app-log.txt" -ErrorAction SilentlyContinue
  Write-Host "::error::self-test did not finish in 180 s ($OutDir)"
  exit 1
}
Get-Content "$OutDir\ui-report.json"
exit $p.ExitCode
