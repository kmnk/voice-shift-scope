# voice-shift-scope

A Windows app that changes the pitch and the formants of your microphone voice separately, sends it to an output device, and shows F0, F1–F4, and the harmonics of the original and shifted voice side by side on one screen. It is made for voice research: you can hold the display as a target and compare how close you get while changing the way you use your voice.

![Spectrograms of the original (left) and shifted (right) voice, the spectrum, and the values table](docs/images/screen.png)

## Requirements

- Windows 10 or 11, x64
- Microsoft Edge WebView2 Runtime. It is included in Windows 11 and installed on most Windows 10 machines. If the app does not start, install it from https://developer.microsoft.com/microsoft-edge/webview2/
- A recording device (microphone) and a playback device for the voice
- Only to pass the shifted voice to another app as a microphone: a virtual audio device such as VB-CABLE (https://vb-audio.com/Cable/ )

## Download and start

1. Download `voice-shift-scope-<version>-windows-x64.zip` from https://github.com/kmnk/voice-shift-scope/releases
2. Unpack it anywhere and start `voice-shift-scope.exe` inside. There is no installer

Versions starting with `0.` are development versions for trying out.

To check that the download is not damaged, compare the value in `SHA256SUMS.txt` of the release with the output of `Get-FileHash <path to the zip>` in PowerShell.

Settings (the chosen devices, language, view, pitch and formant values, and so on) are kept in `%APPDATA%\dev.kmnk.voice-shift-scope\preferences.json`. To stop using the app, delete the unpacked folder and this folder.

## Warnings when downloading and starting

The current release packages are not code-signed, so you may see the following.

- When downloading in a browser, a warning that the file is not commonly downloaded may appear. Follow the browser's prompts to keep the file
- When starting, a "Windows protected your PC" screen may appear. Press "More info" and then "Run anyway" to start
- On PCs with Windows 11 Smart App Control turned on, unsigned apps are blocked and cannot start. Turning Smart App Control off lets the app start, but it may not be possible to turn it back on, so decide for yourself whether to turn it off

## Usage

1. In the left column, choose the input (microphone) in `IN` and the output (where the voice goes) in `OUT`
2. Press `START` on the top bar
3. Change the voice with `PITCH` and `FORMANT`, and compare the original and shifted voice in the plots and values. Hold the display with the `ORIGINAL` and `SHIFTED` switches, and save the held screen as a PNG with `SNAPSHOT`

The `?` button on the top bar lists the keyboard shortcuts.

## Passing the voice to another app

`OUT` can be any playback device, such as speakers or headphones. To pass the shifted voice to a call or recording app as a microphone, install a virtual audio device such as VB-CABLE (https://vb-audio.com/Cable/ ), choose its playback side in `OUT` (`CABLE Input` for VB-CABLE), and choose its recording side (`CABLE Output`) as the microphone in the other app. If VB-CABLE is installed and `OUT` has never been chosen, the app chooses it (the chosen `OUT` is kept for the next start).

The audio sent to `OUT` and the monitor (`MON`) can be switched with `SRC` between the shifted voice (`SHIFTED`) and the original voice (`ORIGINAL`).

## Network

The app sends nothing to and receives nothing from the network. It only changes the microphone sound on your PC and sends it to the chosen devices, without recording it. Settings are kept in the `preferences.json` above and saved images in the folder you choose, only on your PC.

## Development

How to build and test is in [AGENTS.md](AGENTS.md), and the names and pass conditions of the checks are in [docs/criteria.md](docs/criteria.md). The development tool `voice-cli.exe` is also in the releases as a separate zip.

## License

You may use it under either the MIT License ([LICENSE-MIT](LICENSE-MIT)) or the Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.

The release packages include the license texts of the third-party software used, as `THIRD-PARTY-LICENSES.txt`. Signalsmith Stretch and Signalsmith Linear in `third_party/` are under the MIT License.
