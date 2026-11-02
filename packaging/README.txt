voice-shift-scope

マイクの声のピッチとフォルマントを別々に変えて出力の機器に流し、変える前と後の
F0・F1〜F4・倍音を 1 つの画面に並べて表示する Windows のアプリ。

詳しい説明、使い方、ソースは GitHub にある。
https://github.com/kmnk/voice-shift-scope


動く環境

- Windows 10・11 の x64
- Microsoft Edge WebView2 Runtime（Windows 11 には含まれる。起動しないときは
  https://developer.microsoft.com/microsoft-edge/webview2/ から入れる）
- 変えた声を他のアプリにマイクとして渡すときだけ、VB-CABLE などの仮想の音声機器


起動

このフォルダーの voice-shift-scope.exe を起動する。インストールはしない。
設定は %APPDATA%\dev.kmnk.voice-shift-scope\preferences.json に残る。


起動するときに出る警告

この版にはコードの署名が無い。
- 「Windows によって PC が保護されました」と出たら、「詳細情報」を押してから
  「実行」を押すと起動する
- Windows 11 の Smart App Control が有効な PC では起動できない


ライセンス

voice-shift-scope は MIT License（LICENSE-MIT）と Apache License 2.0
（LICENSE-APACHE）の、どちらかを選んで使える。
使っている第三者のソフトウェアのライセンスは THIRD-PARTY-LICENSES.txt にある。
