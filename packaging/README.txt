voice-shift-scope

A Windows app that changes the pitch and the formants of your microphone
voice separately, sends it to an output device, and shows F0, F1-F4, and
the harmonics of the original and shifted voice side by side on one screen.

The full description, usage, and source are on GitHub:
https://github.com/kmnk/voice-shift-scope


Requirements

- Windows 10 or 11, x64
- Microsoft Edge WebView2 Runtime (included in Windows 11; if the app does
  not start, install it from
  https://developer.microsoft.com/microsoft-edge/webview2/ )
- Only to pass the shifted voice to another app as a microphone: a virtual
  audio device such as VB-CABLE


Start

Start voice-shift-scope.exe in this folder. There is no installer.
Settings are kept in %APPDATA%\dev.kmnk.voice-shift-scope\preferences.json


Warnings when starting

This version is not code-signed.
- If "Windows protected your PC" appears, press "More info" and then
  "Run anyway" to start
- On PCs with Windows 11 Smart App Control turned on, it cannot start


License

voice-shift-scope may be used under either the MIT License (LICENSE-MIT) or
the Apache License 2.0 (LICENSE-APACHE), at your option.
The licenses of the third-party software used are in
THIRD-PARTY-LICENSES.txt.
