# voice-shift-scope

A Windows app for voice research. It changes the pitch and the formants of the microphone voice separately, sends the result to a chosen output device (a virtual device such as VB-CABLE when passing the voice to another app; this is not assumed), and shows F0, F1–F4, and the harmonics of the original and shifted voice side by side on one screen. Built with Rust (a Cargo workspace) and Tauri (added in phase 5).

## Language

Write documents, comments, doc comments, test names, commit messages, PR descriptions, and messages from the app, `voice-cli`, scripts, and workflows in English, so that people who do not read Japanese (including reviewers of applications such as code signing) can read the project. The only Japanese in the repository is the Japanese UI text in `frontend/src/i18n.ts` (the screen can switch between English and Japanese), the tests of that text, and the Japanese sentences given to the Kyoko text-to-speech voice in `scripts/quality_check.py` and `scripts/formant_stability.py`. When adding a UI string, add both the English and the Japanese text to `i18n.ts`.

## Structure

Dependencies point only from lower rows to upper rows. Only `voice-tauri` depends on Tauri.

| crate | Layer | Holds |
| --- | --- | --- |
| `crates/voice-domain` | domain | Value objects, `VoiceSession`, and port traits. Does not depend on outside technology |
| `crates/voice-app` | application | Use cases. Takes the ports as generics |
| `crates/voice-dsp` | outbound adapter | Analysis estimates (F1–F4 are tracked by assigning LPC roots by their continuity with the previous frame, returning the median of the last 5 frames 2 frames late; `formant_track.rs`) and the voice shifter (`app_shifter`: TD-PSOLA when lowering the pitch, Signalsmith Stretch otherwise; both match the output level to the input) |
| `crates/voice-runtime` | outbound adapter | Device enumeration with cpal, the audio thread, and the analysis thread |
| `crates/voice-tauri` | inbound adapter and assembly | Tauri commands and channels |
| `crates/voice-cli` | checking tool | A CLI for checks during development. Not a feature of the app |
| `frontend/` | screen | Drawing in TypeScript and the hold state of the display |

On the audio thread, do not allocate memory, take locks, or drop values.

## Build and test

```
cargo test --workspace
cargo run -p voice-cli -- devices      # print the recording and playback devices as JSON (does not open them)
cd frontend && npm ci && npm test && npm run typecheck
```

Compare with Praat (Parselmouth):

```
cargo run -p voice-cli -- synth vowel --f0 100 --out /tmp/v100.wav
uv run scripts/compare_praat.py /tmp/v100.wav --expect 700,1200,2600,3500 --formants 4 --ceiling 5000
```

- `scripts/compare_praat.py` only reads files and writes nothing. With `--expect` or `--compare`, it exits with 1 when a difference exceeds the allowed one (by default 5% for F1–F3 and 8% for F4)
- The synthetic vowel has only 4 resonances (F1–F4), so Praat's default (5 formants, 5500 Hz) places a resonance between F2 and F3 that is not there. Use `--formants 4 --ceiling 5000` for synthetic vowels
- In a sandbox, add `UV_CACHE_DIR=$TMPDIR/uv`

- The voice shifter is assembled in `voice_dsp::app_shifter` (`crates/voice-dsp/src/hybrid.rs`). Below 0 semitones it uses TD-PSOLA in `psola.rs`; at 0 semitones and above, Signalsmith Stretch, crossfading over 30 ms when the setting crosses the boundary. Both run every block, and PSOLA outputs with the same delay as Signalsmith Stretch (3360 samples). Parts PSOLA judges unvoiced pass through with the pitch unchanged if they are aperiodic noise (consonants), and are otherwise filled with the Signalsmith Stretch output (so that missed voicing does not let the original pitch through). Both outputs are matched by `LevelMatch` in `level.rs` to the level of the input delayed by the same amount (not at 0 semitones and ×1.0). The app (`voice-tauri`) and the test tools (`check`, `e2e`, and `shift` of `voice-cli`) all use `app_shifter`
- Signalsmith Stretch is the Signalsmith Stretch 1.4.0 (C++, MIT) bundled in `third_party/`, built and called through `crates/voice-dsp/cpp/stretch_wrapper.cpp` and `crates/voice-dsp/build.rs` (the `cc` crate). It needs a C++17 compiler (the Xcode Command Line Tools on macOS, MSVC on Windows). The crates.io `signalsmith-stretch` 0.1.3 is not used, because its bundled version is old with a coarse formant envelope, and its `set_formant_base` wrongly calls `setFormantSemitones`
- Agents running in a sandbox cannot write to `.cargo` in the home directory, so run with `CARGO_HOME=$PWD/.cargo-home` (in `.gitignore`). Give npm `--cache "$TMPDIR/npm-cache"`
- What runs only on Windows (WASAPI, VB-CABLE, the Windows build of Tauri) is checked on GitHub Actions `windows-latest`. Run with `gh workflow run ci.yml` and wait with `gh run watch`

## Verification

Check each kind of change as follows. The meaning and pass conditions of the check names (A1, U3-settings, and so on) are in [docs/criteria.md](docs/criteria.md). When adding a check or changing a condition, update that document. What runs only on Windows runs on GitHub Actions Windows runners (the runners are created fresh each time, so the user's devices and settings are not touched).

- `cargo test --workspace` and `cd frontend && npm test`: value types and the rules of `VoiceSession`, use cases, analysis parts, test signals, finding VB-CABLE, audio runtime parts (such as clock drift correction), the display hold state (the state transitions of B5, B7, and B8), and colours kept in the CSS variables (S1)
- `cargo run --release -p voice-cli -- check --criteria A1,A2,A3,A4,B1,B3,Q2,Q3,Q4,Q5 [--json] [--block-ms 70] [--shifter app|signalsmith]`: runs the checks of [docs/criteria.md](docs/criteria.md) on test signals. Exit code 1 if any FAIL. B1 is judged by the difference from Praat's values with the same settings (`PRAAT_REFERENCE` in `check.rs`; regenerate with `scripts/praat_reference.sh`). Now only A3 at F0 150 Hz with ×0.9 FAILs (CI runs only the passing `A1,A4,B1,B3`). Q2–Q5 are the resonances when lowering the pitch, switching between the two methods, the shifted level, and not amplifying silence. `--shifter signalsmith` checks with the processing before the change (Signalsmith Stretch only); use it to confirm that a check catches the difference
- `uv run scripts/quality_check.py [--shifter app|signalsmith] [--keep <dir>] [--speech <wav>...]`: Q1 (periodicity when lowering the pitch). Shifts synthetic vowels with variation through `voice-cli shift` and compares Praat's HNR with the ideal output. Given voice wavs with `--speech` (48 kHz, mono; on macOS they can be made with `say` and `afconvert`, as described at the top of the script), it also checks Q10 (no original pitch mixed in when lowered), Q11 (no more noise at word onsets than the processing before the change), Q12 (the original pitch is not heard in a lowered voice; the ratio of the odd to even harmonics of the target F0), and Q13 (consonants such as s and sh are not muffled; the spectral centroid of consonant frames). PSOLA does not miss voicing on synthetic vowels, so check with voice wavs. Needs `cargo build --release -p voice-cli` first
- `cargo run --release -p voice-cli -- shift <in.wav> <out.wav> --pitch <semitones> --formant <factor> [--shifter app|signalsmith] [--timing]`: runs a 48 kHz wav through the same shifter as the app in 10 ms steps and writes it (does not open devices). `--timing` prints the median and maximum processing time of one block as JSON. As in the app's analysis thread, it gives the F0 of the latest 4096 samples as a hint (`set_pitch_hint`) every 10 ms (`--no-hint` disables this). Without hints, sounds at 0 semitones and above (Signalsmith Stretch) differ from the app
- Passing the CSV from `voice-cli analyze <wav>` to `scripts/compare_praat.py <wav> --compare <csv>` prints the difference from Praat
- `uv run scripts/formant_stability.py [--keep <dir>] [--json] [--speech <wav>[:<ceiling Hz>]...]`: B9–B12 (stability of F1–F4 in the scrolling plots). Makes vowels with variation and breath noise and vowels whose F2 moves, and compares `voice-cli analyze` (values after the same tracking as the app) with `voice-cli analyze --raw-formants` (no tracking; the 4 lowest roots) and with the true values. The scrolling plot's values are 2 frames old (`voice_dsp::FORMANT_DELAY_FRAMES`) because of the median after tracking, so they are shifted by that much before comparing. Given voice wavs with `--speech`, it also checks single-frame outliers (B11) and jumps (B12) (made with `say` as for `quality_check.py`; to try another ceiling, use `kyoko.wav:6500`). Run it after changing the F1–F4 estimate (`lpc.rs`, `formant_track.rs`, `analyzer.rs`). It does not open devices, and without `--keep` it writes to a temporary directory and deletes it. Needs `cargo build --release -p voice-cli` first
- `voice-cli analyze --timing` writes the median and maximum processing time of one analysis as JSON to standard error. Comparing with and without tracking (`--raw-formants`) shows the cost of tracking
- `windows-e2e.yml` (`gh workflow run windows-e2e.yml`; results with `gh run download <id> -n e2e-report`): on a runner with VB-CABLE installed, runs `voice-cli loopback` (the VB-CABLE path without the shifter) and `voice-cli e2e` (the audio runtime with a wav source, recorded back from VB-CABLE). `e2e-*.json` has A1, A5 (only at 0 semitones), and A6, and the amount in the output ring buffer and its correction. It runs 0 semitones (`e2e-latency.json`), +5 semitones (`e2e-runtime.json`, the Signalsmith Stretch side), and −12 semitones (`e2e-runtime-lowered.json`, the PSOLA side). On runners A5 and A6 FAIL (because of the runners' devices and clocks). So the workflow result is decided only by A1, the analysis, and `errors` (`scripts/e2e_gate.ps1`). That `loopback` and `e2e` stop with exit code 2 without VB-CABLE is checked in the same job before installing VB-CABLE
- `windows-ui.yml`: starts the app with `--input-wav <wav> --ui-selftest <dir>`; the screen (`frontend/src/selftest.ts`) performs the same operations as a user in order and writes `<dir>/ui-report.json`. Exit code 0 if all PASS. Saved PNGs are in `<dir>/saved/` and uploaded as an artifact. One job runs it once before and once after installing VB-CABLE (`scripts/ui_selftest.ps1`), and the run before checks C4. The results are the artifacts `ui-without-vb-cable` and `ui-with-vb-cable`
  - To open the recording device, the workflow allows microphone access in the privacy settings. Without it, WASAPI returns "Access is denied"
  - Each time an acceptance item is checked, the window's client area at that moment is written to `<dir>/shots/<item>.png` (the compact view is `compact-view-compact.png`). The shots are taken by `selftest_shot` on the Rust side (`crates/voice-tauri/src/shot.rs`, Windows `PrintWindow` with `PW_RENDERFULLCONTENT` so the WebView2 content is drawn too), only on Windows. On other OSes it does not take them and writes the reason to `<dir>/app-log.txt`. Failing to take a shot does not count in the test results. `shots/` is uploaded with `<dir>` as an artifact. After changing the UI, open the item images and check the changed parts
  - Window size: at the end, the test-only command `selftest_resize` (`crates/voice-tauri/src/lib.rs`; it works only when started with `--ui-selftest`, and only from 960×640 to 2560×1600) shrinks the window to the minimum 960×640, writes the `window-960x640` item and `shots/size-960x640.png`, and restores 1280×860 (`window-1280x860`). Check the layout in a small window with this image, and when adding items, put checks between these
  - It also runs on a Mac debug build (`target/debug/voice-shift-scope` from `cargo build -p voice-tauri`) with `--input-wav <wav> --ui-selftest <dir>`. Without VB-CABLE, the items that play sound (B4–B8, S4, S5, S7, S8, U10, U11, and so on) do not run, and no images are taken. `compact-view` FAILs only on a Mac (how the window height is read; it does not depend on the change). The `monitor-button` check takes about 2 minutes, so set the time limit to 5 minutes or more
  - Test-only entry points: `--input-wav` (plays a wav instead of the input device; "Test source" appears in the list), `--ui-selftest`, and the environment variables `VOICE_SHIFT_SCOPE_CONFIG_DIR` (where `preferences.json` goes), `VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT` ("semitones,factor": the pitch and formant expected in the fields at start, compared by the self-test's `P1-shift-restored`; without it, the defaults of 0 semitones and ×1.0), `VOICE_SHIFT_SCOPE_TEST_SAVE_DIR` (writes images without the save dialog), and `VOICE_SHIFT_SCOPE_OUTPUT_BUFFER_MS` (the amount in the output ring buffer)
- Running `voice-cli loopback` and `voice-cli e2e` on the user's Windows plays sound into VB-CABLE. If other software is recording CABLE Output, it will pick up that sound
- `preferences.json` remembers the image folder, the language, the view, the devices, the analysis settings, and the pitch and formant (`crates/voice-tauri/src/preferences.rs`). The pitch and formant are written at most once every 0.5 seconds when changed, and also when the app closes (`RunEvent::Exit`). If a value is out of range, the app starts with the default. To check on a Mac that the app starts with remembered values, put `{"shift":{"semitones":-12.0,"formant":0.8}}` in `preferences.json` and run the self-test with `VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT=-12,0.8`
- With the default feature `custom-protocol` of `voice-tauri`, the Tauri app opens the built screen (`frontend/dist`) even with plain `cargo build`. Run `cd frontend && npm run build` first

## Commits

Short imperative English (for example `Add device probing to voice-runtime`).
