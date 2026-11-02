# voice-shift-scope

声の研究のための Windows のアプリ。マイクの声のピッチとフォルマントを別々に変えて、選んだ出力の機器（声を他のアプリに渡すときは VB-CABLE などの仮想の機器。いつも使う前提にはしない）に出し、変える前と後の F0・F1〜F4・倍音を 1 つの画面に並べて表示する。Rust（Cargo の workspace）と Tauri（フェーズ 5 で入れる）で作る。

## 構成

依存は下の行から上の行へだけ向く。Tauri に依るのは `voice-tauri` だけにする。

| crate | 層 | 持つもの |
| --- | --- | --- |
| `crates/voice-domain` | domain | value object、`VoiceSession`、port の trait。外の技術に依らない |
| `crates/voice-app` | application | use case。port を generics で受け取る |
| `crates/voice-dsp` | 出る側の adapter | 分析の推定と、声を変える処理（`app_shifter`: ピッチを下げるときは TD-PSOLA、それ以外は Signalsmith Stretch。どちらも出力の音量を入力にそろえる） |
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

- 声を変える処理は `voice_dsp::app_shifter` で組み立てる（`crates/voice-dsp/src/hybrid.rs`）。ピッチが 0 半音より下なら `psola.rs` の TD-PSOLA、0 半音以上なら Signalsmith Stretch を使い、境目をまたいだら 30 ms で混ぜる。2 つは毎ブロックどちらも動かし、PSOLA は Signalsmith Stretch と同じ遅れ（3360 標本）で出す。PSOLA が無声と判定した区間は、周期性の無い雑音（子音）なら音程を動かさずに通し、それ以外は Signalsmith Stretch の出力で埋める（有声の取りこぼしで元の高さの声が混ざらないように）。どちらの出力も `level.rs` の `LevelMatch` で、同じだけ遅らせた入力の音量にそろえる（0 半音・1.0 倍のときはそろえない）。アプリ（`voice-tauri`）と試験の道具（`voice-cli` の `check`・`e2e`・`shift`）は、どれも `app_shifter` を使う
- Signalsmith Stretch は、`third_party/` に取り込んだ Signalsmith Stretch 1.4.0（C++、MIT）を、`crates/voice-dsp/cpp/stretch_wrapper.cpp` と `crates/voice-dsp/build.rs`（`cc` crate）で build して呼ぶ。C++17 のコンパイラが要る（macOS は Xcode の Command Line Tools、Windows は MSVC）。crates.io の `signalsmith-stretch` 0.1.3 は、中の版が古くフォルマントの包絡が粗いことと、`set_formant_base` が `setFormantSemitones` を呼ぶ誤りがあるので使わない
- sandbox の中で動かすエージェントは、home ディレクトリの `.cargo` に書けないので、`CARGO_HOME=$PWD/.cargo-home` を付けて動かす（`.gitignore` 済み）。npm は `--cache "$TMPDIR/npm-cache"` を付ける
- Windows でしか動かないもの（WASAPI、VB-CABLE、Tauri の Windows の build）は、GitHub Actions の `windows-latest` で確かめる。`gh workflow run ci.yml` で動かし、`gh run watch` で待つ

## 検証

変更の種類ごとに、次で確かめる。判定の名前（A1、U3-settings など）の意味と合格の条件は [docs/criteria.md](docs/criteria.md) にある。判定を足すときや条件を変えるときは、そこを直す。Windows でしか動かないものは GitHub Actions の Windows のランナーで動かす（ランナーは毎回作り直されるので、利用者の機器や設定に触れない）。

- `cargo test --workspace` と `cd frontend && npm test`: 値の型と `VoiceSession` の規則、use case、分析の部品、試験用の信号、VB-CABLE を見つける処理、音声の実行の部品（時計のずれの補正など）、表示を止める状態（B5・B7・B8 の状態の移り方）
- `cargo run --release -p voice-cli -- check --criteria A1,A2,A3,A4,B1,B3,Q2,Q3,Q4,Q5 [--json] [--block-ms 70] [--shifter app|signalsmith]`: [docs/criteria.md](docs/criteria.md) の判定を、試験用の信号で行う。FAIL があれば終了コード 1。B1 は、同じ設定の Praat の値（`check.rs` の `PRAAT_REFERENCE`、作り直すときは `scripts/praat_reference.sh`）との差で判定する。今は A3 の F0 150 Hz・0.9 倍だけが FAIL する（CI は通るものだけ `A1,A4,B1,B3` を動かす）。Q2〜Q5 は、ピッチを下げたときの響き、2 つの処理の切り替え、変えた後の音量、無音を持ち上げないこと。`--shifter signalsmith` は変更の前の処理（Signalsmith Stretch だけ）で判定する。判定が違いを捕まえるかを確かめるのに使う
- `uv run scripts/quality_check.py [--shifter app|signalsmith] [--keep <dir>] [--speech <wav>...]`: Q1（ピッチを下げたときの周期性）。揺れのある合成の母音を `voice-cli shift` で変え、Praat の HNR を理想の出力と比べる。`--speech` に声の wav（48 kHz、mono。macOS なら `say` と `afconvert` で作れる。作り方は script の先頭）を渡すと、Q10（下げたとき元の高さの声が混ざらない）、Q11（言葉の頭で変更の前の処理より雑音が増えない）、Q12（下げた声で元の高さが聞こえない。狙いの F0 の奇数番目と偶数番目の倍音の強さの比）、Q13（さしすせそなどの子音がこもらない。子音の枠のスペクトルの重心）も判定する。合成の母音では PSOLA の有声の取りこぼしが起きないので、声の wav を渡して確かめる。先に `cargo build --release -p voice-cli` が要る
- `cargo run --release -p voice-cli -- shift <in.wav> <out.wav> --pitch <半音> --formant <倍率> [--shifter app|signalsmith] [--timing]`: 48 kHz の wav を、アプリと同じ声を変える処理に 10 ms ずつ通して書く（機器は開かない）。`--timing` は 1 ブロックの処理時間の中央値と最大を JSON で出す。アプリの分析のスレッドと同じく、直近 4096 標本の F0 を 10 ms ごとに目安（`set_pitch_hint`）として渡す（`--no-hint` で渡さない）。目安を渡さないと、0 半音以上の音（Signalsmith Stretch）がアプリと違う音になる
- `voice-cli analyze <wav>` の CSV を `scripts/compare_praat.py <wav> --compare <csv>` に渡すと、Praat との差を出す
- `windows-e2e.yml`（`gh workflow run windows-e2e.yml`、結果は `gh run download <id> -n e2e-report`）: VB-CABLE を入れたランナーで、`voice-cli loopback`（声を変える処理を通さない VB-CABLE の経路）と `voice-cli e2e`（音声の実行を wav の音源で通し、VB-CABLE から録り返す）を動かす。`e2e-*.json` に A1・A5（0 半音のときだけ）・A6 と、出力の ring buffer の量と補正が出る。0 半音（`e2e-latency.json`）、+5 半音（`e2e-runtime.json`、Signalsmith Stretch の側）、−12 半音（`e2e-runtime-lowered.json`、PSOLA の側）の 3 つを動かす。ランナーでは A5 と A6 は FAIL する（ランナーの機器と時計の性質による。利用者の Windows で `docs/windows-checklist.md` のとおりに確かめる）。そのため workflow の合否は、A1 と分析と `errors` だけで決める（`scripts/e2e_gate.ps1`）。VB-CABLE が無いときに `loopback` と `e2e` が終了コード 2 で止まることは、同じ job で VB-CABLE を入れる前に確かめる
- `windows-ui.yml`: アプリを `--input-wav <wav> --ui-selftest <dir>` で起動し、画面の側（`frontend/src/selftest.ts`）が利用者と同じ操作を順に行って `<dir>/ui-report.json` を書く。全部 PASS なら終了コード 0。保存した PNG は `<dir>/saved/` にあり、artifact に上がる。1 つの job で、VB-CABLE を入れる前と後に 1 回ずつ動かし（`scripts/ui_selftest.ps1`）、入れる前の回で C4 を確かめる。結果は artifact の `ui-without-vb-cable` と `ui-with-vb-cable`
  - 録音の機器を開くために、workflow の中でマイクのプライバシーの設定を許している。消すと WASAPI が「Access is denied」を返す
  - 受け入れ基準の項目を判定するたびに、その時点の窓の client 領域を `<dir>/shots/<項目>.png` に書く（簡易表示の状態は `compact-view-compact.png`）。撮るのは Rust 側の `selftest_shot`（`crates/voice-tauri/src/shot.rs`、Windows の `PrintWindow` に `PW_RENDERFULLCONTENT` を付けて WebView2 の中身も描かせる）で、Windows でだけ撮る。他の OS では撮らずに理由を `<dir>/app-log.txt` に書く。撮れないことは試験の合否に入れない。`shots/` は `<dir>` ごと artifact に上がる。UI を変えたら、項目の画像を開いて変えた所を照らす
  - 窓の大きさ: 最後に、試験のときだけの command `selftest_resize`（`crates/voice-tauri/src/lib.rs`。`--ui-selftest` で起動したときだけ動き、960×640〜2560×1600 に限る）で窓を最小の 960×640 にし、`window-960x640` の項目と `shots/size-960x640.png` を書いてから 1280×860 に戻す（`window-1280x860`）。小さい窓での配置は、この画像と、項目を足すときはこの間に判定を置いて確かめる
  - Mac の debug build（`cargo build -p voice-tauri` の `target/debug/voice-shift-scope`）でも `--input-wav <wav> --ui-selftest <dir>` で動く。VB-CABLE が無いので、音を流す項目（B4〜B8・S4・S5・S7・S8・U10・U11 など）は動かず、画像も撮れない。`compact-view` は Mac でだけ FAIL する（窓の高さの読み方。変更に依らない）。`monitor-button` の判定に 2 分ほどかかるので、時間の上限は 5 分以上にする
  - 試験のときだけの入口: `--input-wav`（入力の機器の代わりに wav を流す。一覧に「試験の音源」が出る）、`--ui-selftest`、環境変数 `VOICE_SHIFT_SCOPE_CONFIG_DIR`（`preferences.json` の置き場）・`VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT`（"半音,倍率"。起動したときに欄に出ているはずのピッチとフォルマントで、自己試験の `P1-shift-restored` が比べる。無ければ既定の 0 半音・1.0 倍）・`VOICE_SHIFT_SCOPE_TEST_SAVE_DIR`（保存先を選ぶ画面を出さずに画像を書く）・`VOICE_SHIFT_SCOPE_OUTPUT_BUFFER_MS`（出力の ring buffer の量）
- `voice-cli loopback` と `voice-cli e2e` を利用者の Windows で動かすと、VB-CABLE に音を流す。動いている他のソフトが CABLE Output を録っていると、その音が入る
- `preferences.json` は、画像の保存先・言葉・表示・機器・分析の設定・ピッチとフォルマントを覚える（`crates/voice-tauri/src/preferences.rs`）。ピッチとフォルマントは、値を変えたら 0.5 秒に 1 回まで書き、アプリを閉じるとき（`RunEvent::Exit`）にも書く。範囲の外の値が入っていたら既定のまま起動する。覚えた値で起動するのを Mac で確かめるには、`preferences.json` に `{"shift":{"semitones":-12.0,"formant":0.8}}` を置き、`VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT=-12,0.8` を付けて自己試験を動かす
- Tauri のアプリは、`voice-tauri` の既定の feature の `custom-protocol` で、`cargo build` だけでも build した画面（`frontend/dist`）を開く。先に `cd frontend && npm run build` が要る

## commit

短い命令形の英語（例 `Add device probing to voice-runtime`）。
