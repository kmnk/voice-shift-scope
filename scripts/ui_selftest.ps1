# Runs the end-to-end UI test of windows-ui.yml once. Starts the app with --ui-selftest and waits for it to write ui-report.json and exit.
# The exit code is the app's (0 is success). If it does not finish in 180 seconds, it is stopped and the exit code is 1.
# Usage: pwsh scripts/ui_selftest.ps1 <output directory> [-Exe <app exe>]   The input is vowel150.wav (make it first with voice-cli synth)
# Without -Exe, starts target\release\voice-shift-scope.exe. release.yml passes the exe from the unpacked release package
param([Parameter(Mandatory)][string]$OutDir, [string]$Exe = 'target\release\voice-shift-scope.exe')

New-Item -ItemType Directory -Force "$OutDir\saved", "$OutDir\config" | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$env:VOICE_SHIFT_SCOPE_CONFIG_DIR = "$OutDir\config"
$env:VOICE_SHIFT_SCOPE_TEST_SAVE_DIR = "$OutDir\saved"
$p = Start-Process $Exe -ArgumentList '--input-wav', 'vowel150.wav', '--ui-selftest', $OutDir -PassThru
Start-Sleep -Seconds 20
# Take a screenshot after 20 seconds as a clue if it stops
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
