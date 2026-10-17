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

- Signalsmith Stretch の wrapper（`signalsmith-stretch`）は build の時に `bindgen` を使うので、libclang が要る。macOS は Xcode の Command Line Tools、Windows は LLVM（GitHub Actions のランナーには入っている。`LIBCLANG_PATH` を `.github/workflows/ci.yml` で設定している）
- sandbox の中で動かすエージェントは、home ディレクトリの `.cargo` に書けないので、`CARGO_HOME=$PWD/.cargo-home` を付けて動かす（`.gitignore` 済み）。npm は `--cache "$TMPDIR/npm-cache"` を付ける
- Windows でしか動かないもの（WASAPI、VB-CABLE、Tauri の Windows の build）は、GitHub Actions の `windows-latest` で確かめる。`gh workflow run ci.yml` で動かし、`gh run watch` で待つ

## 検証

作りかけ。wav を通す CLI（`voice-cli check`）、Windows のランナーで VB-CABLE を通す試験（`windows-e2e.yml`）、画面の試験（`windows-ui.yml`）を足していく。それまでの確かめは上の「build と test」と `ci.yml` で行う。

## commit

短い命令形の英語（例 `Add device probing to voice-runtime`）。
