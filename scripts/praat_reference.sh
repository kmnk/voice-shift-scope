#!/bin/sh
# Regenerates the B1 reference values (PRAAT_REFERENCE in crates/voice-cli/src/check.rs).
# Writes the test vowels and prints the F1–F4 medians from Praat (Parselmouth). Files are written only to a temporary place.
set -eu
dir=$(mktemp -d)
cargo build -q --release -p voice-cli
for f0 in 100 150 220; do
  ./target/release/voice-cli synth vowel --f0 "$f0" --seconds 1.5 --out "$dir/v$f0.wav" >/dev/null
  printf '%s Hz: ' "$f0"
  uv run -q scripts/compare_praat.py "$dir/v$f0.wav" --skip 0.2 | cut -f2 | paste -sd ' ' -
done
rm -rf "$dir"
