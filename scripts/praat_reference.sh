#!/bin/sh
# B1 の参照の値（crates/voice-cli/src/check.rs の PRAAT_REFERENCE）を作り直す。
# 試験用の母音を書き、Praat（Parselmouth）で F1〜F4 の中央値を出す。ファイルは一時的な場所にだけ書く。
set -eu
dir=$(mktemp -d)
cargo build -q --release -p voice-cli
for f0 in 100 150 220; do
  ./target/release/voice-cli synth vowel --f0 "$f0" --seconds 1.5 --out "$dir/v$f0.wav" >/dev/null
  printf '%s Hz: ' "$f0"
  uv run -q scripts/compare_praat.py "$dir/v$f0.wav" --skip 0.2 | cut -f2 | paste -sd ' ' -
done
rm -rf "$dir"
