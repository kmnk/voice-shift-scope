# voice-shift-scope

声の研究のための Windows のアプリ。マイクの声のピッチとフォルマントを別々に変えて VB-CABLE に出し、変える前と後の F0・F1〜F4・倍音を 1 つの画面に並べて表示する。Rust（Cargo の workspace）と Tauri（フェーズ 5 で入れる）で作る。

## 構成

依存は下の行から上の行へだけ向く。Tauri に依るのは `voice-tauri` だけにする。

| crate | 層 | 持つもの |
| --- | --- | --- |
| `crates/voice-domain` | domain | value object、`VoiceSession`、port の trait。外の技術に依らない |
| `crates/voice-app` | application | use case。port を generics で受け取る |
| `crates/voice-dsp` | 出る側の adapter | 分析の推定と、声を変える処理（Signalsmith Stretch） |
| `crates/voice-runtime` | 出る側の adapter | cpal での機器の列挙、音声のスレッド、分析のスレッド |
| `crates/voice-tauri` | 入る側の adapter と組み立て | Tauri の command と channel |
| `crates/voice-cli` | 確かめる道具 | 開発で確かめるための CLI。アプリの機能ではない |
| `frontend/` | 画面 | TypeScript の描画と、表示を止める状態 |

音声のスレッドの中では、メモリの確保・ロック・drop をしない。

## build と test

```
cargo test --workspace
cargo run -p voice-cli -- devices      # 録音と再生の機器を JSON で出す（機器は開かない）
cd frontend && npm ci && npm test && npm run typecheck
```

Praat（Parselmouth）と突き合わせる:

```
cargo run -p voice-cli -- synth vowel --f0 100 --out /tmp/v100.wav
uv run scripts/compare_praat.py /tmp/v100.wav --expect 700,1200,2600,3500 --formants 4 --ceiling 5000
```

- `scripts/compare_praat.py` はファイルを読むだけで書かない。`--expect` か `--compare` を渡すと、許す差（既定 F1〜F3 5%、F4 8%）を超えたとき終了コード 1
- 合成の母音は共鳴を F1〜F4 の 4 つしか持たないので、Praat の既定（5 個、5500 Hz）では F2 と F3 の間に無い共鳴を置く。合成の母音には `--formants 4 --ceiling 5000` を使う
- sandbox の中では `UV_CACHE_DIR=$TMPDIR/uv` を付ける

- 声を変える処理は、`third_party/` に取り込んだ Signalsmith Stretch 1.4.0（C++、MIT）を、`crates/voice-dsp/cpp/stretch_wrapper.cpp` と `crates/voice-dsp/build.rs`（`cc` crate）で build して呼ぶ。C++17 のコンパイラが要る（macOS は Xcode の Command Line Tools、Windows は MSVC）。crates.io の `signalsmith-stretch` 0.1.3 は、中の版が古くフォルマントの包絡が粗いことと、`set_formant_base` が `setFormantSemitones` を呼ぶ誤りがあるので使わない
- sandbox の中で動かすエージェントは、home ディレクトリの `.cargo` に書けないので、`CARGO_HOME=$PWD/.cargo-home` を付けて動かす（`.gitignore` 済み）。npm は `--cache "$TMPDIR/npm-cache"` を付ける
- Windows でしか動かないもの（WASAPI、VB-CABLE、Tauri の Windows の build）は、GitHub Actions の `windows-latest` で確かめる。`gh workflow run ci.yml` で動かし、`gh run watch` で待つ

## 検証

変更の種類ごとに、次で確かめる。Windows でしか動かないものは GitHub Actions の Windows のランナーで動かす（ランナーは毎回作り直されるので、利用者の機器や設定に触れない）。

- `cargo test --workspace` と `cd frontend && npm test`: 値の型と `VoiceSession` の規則、use case、分析の部品、試験用の信号、VB-CABLE を見つける処理、音声の実行の部品（時計のずれの補正など）、表示を止める状態（B5・B7・B8 の状態の移り方）
- `cargo run --release -p voice-cli -- check --criteria A1,A2,A3,A4,B1,B3 [--json] [--block-ms 70]`: spec の受け入れの基準を、試験用の信号で判定する。FAIL があれば終了コード 1。B1 は、同じ設定の Praat の値（`check.rs` の `PRAAT_REFERENCE`、作り直すときは `scripts/praat_reference.sh`）との差で判定する。今は A2 の F0 150 Hz・−12 半音と A3 の F0 150 Hz・0.9 倍が FAIL する（CI は通るものだけ `A1,A4,B1,B3` を動かす）
- `voice-cli analyze <wav>` の CSV を `scripts/compare_praat.py <wav> --compare <csv>` に渡すと、Praat との差を出す
- `windows-e2e.yml`（`gh workflow run windows-e2e.yml`、結果は `gh run download <id> -n e2e-report`）: VB-CABLE を入れたランナーで、`voice-cli loopback`（声を変える処理を通さない VB-CABLE の経路）と `voice-cli e2e`（音声の実行を wav の音源で通し、VB-CABLE から録り返す）を動かす。`e2e-*.json` に A1・A5（0 半音のときだけ）・A6 と、出力の ring buffer の量と補正が出る。ランナーでは A5 と A6 は FAIL する（ランナーの機器と時計の性質による。利用者の Windows で `docs/windows-checklist.md` のとおりに確かめる）。そのため workflow の合否は、A1 と分析と `errors` だけで決める（`scripts/e2e_gate.ps1`）。VB-CABLE が無いときに `loopback` と `e2e` が終了コード 2 で止まることは、同じ job で VB-CABLE を入れる前に確かめる
- `windows-ui.yml`: アプリを `--input-wav <wav> --ui-selftest <dir>` で起動し、画面の側（`frontend/src/selftest.ts`）が利用者と同じ操作を順に行って `<dir>/ui-report.json` を書く。全部 PASS なら終了コード 0。保存した PNG は `<dir>/saved/` にあり、artifact に上がる。1 つの job で、VB-CABLE を入れる前と後に 1 回ずつ動かし（`scripts/ui_selftest.ps1`）、入れる前の回で C4 を確かめる。結果は artifact の `ui-without-vb-cable` と `ui-with-vb-cable`
  - 録音の機器を開くために、workflow の中でマイクのプライバシーの設定を許している。消すと WASAPI が「Access is denied」を返す
  - 受け入れ基準の項目を判定するたびに、その時点の窓の client 領域を `<dir>/shots/<項目>.png` に書く（簡易表示の状態は `compact-view-compact.png`）。撮るのは Rust 側の `selftest_shot`（`crates/voice-tauri/src/shot.rs`、Windows の `PrintWindow` に `PW_RENDERFULLCONTENT` を付けて WebView2 の中身も描かせる）で、Windows でだけ撮る。他の OS では撮らずに理由を `<dir>/app-log.txt` に書く。撮れないことは試験の合否に入れない。`shots/` は `<dir>` ごと artifact に上がる。UI を変えたら、項目の画像を開いて変えた所を照らす
  - 窓の大きさ: 最後に、試験のときだけの command `selftest_resize`（`crates/voice-tauri/src/lib.rs`。`--ui-selftest` で起動したときだけ動き、960×640〜2560×1600 に限る）で窓を最小の 960×640 にし、`window-960x640` の項目と `shots/size-960x640.png` を書いてから 1280×860 に戻す（`window-1280x860`）。小さい窓での配置は、この画像と、項目を足すときはこの間に判定を置いて確かめる
  - 試験のときだけの入口: `--input-wav`（入力の機器の代わりに wav を流す。一覧に「試験の音源」が出る）、`--ui-selftest`、環境変数 `VOICE_SHIFT_SCOPE_CONFIG_DIR`（`preferences.json` の置き場）・`VOICE_SHIFT_SCOPE_TEST_SAVE_DIR`（保存先を選ぶ画面を出さずに画像を書く）・`VOICE_SHIFT_SCOPE_OUTPUT_BUFFER_MS`（出力の ring buffer の量）
- `voice-cli loopback` と `voice-cli e2e` を利用者の Windows で動かすと、VB-CABLE に音を流す。動いている他のソフトが CABLE Output を録っていると、その音が入る
- Tauri のアプリは、`voice-tauri` の既定の feature の `custom-protocol` で、`cargo build` だけでも build した画面（`frontend/dist`）を開く。先に `cd frontend && npm run build` が要る

## commit

短い命令形の英語（例 `Add device probing to voice-runtime`）。
