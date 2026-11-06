# Check names

The check names (A1, U3-settings, and so on) used by `voice-cli check`, `scripts/quality_check.py`, the UI self-test (`ui-report.json` from `--ui-selftest`), and `voice-cli e2e`, with what each one checks and when it passes. Names such as "A1" and "S8" in code comments refer to these. When you add a check or change a pass condition, update this document. Only checks that run without a person are listed here.

Abbreviations in "Checked by": `check` is `cargo run --release -p voice-cli -- check --criteria <name>`, `quality` is `uv run scripts/quality_check.py`, `selftest` is the UI self-test (`windows-ui.yml`), and `e2e` is `voice-cli e2e` (`windows-e2e.yml`).

## Test signals

Most A, B, and Q checks use a synthetic vowel whose answer is known. It is a pulse train at a fixed F0, passed through resonance filters at F1 700 Hz, F2 1200 Hz, F3 2600 Hz, and F4 3500 Hz (bandwidths 60–120 Hz) plus F5 4500 Hz (bandwidth 150 Hz), with F0 at 100 Hz, 150 Hz, or 220 Hz (`cargo run -p voice-cli -- synth vowel`). F5 is added because real voices have about 5 resonances below 5500 Hz, and with only F1–F4 the same default estimate as Praat places a resonance between F2 and F3 that is not there.

## A. Changing the voice

| Name | What it checks | Pass condition | Checked by |
| --- | --- | --- | --- |
| A1 | Pitch | At −12, −5, +5, and +12 semitones with formant ×1.0, the shifted F0 is within ±1% of 2^(semitones/12) times the original | `check`, `e2e` |
| A2 | Formants stay when the pitch moves | Under A1, the shifted F1–F3 are within ±5% of the original (F0 100 and 150 Hz) | `check` |
| A3 | Formant | At 0 semitones with formant ×0.8, ×0.9, ×1.1, and ×1.2, F1–F3 move by that factor within ±7%, and F0 stays within ±1%. F0 150 Hz at ×0.9 does not pass because F1 comes close to a harmonic | `check` |
| A4 | No change | At 0 semitones and ×1.0, the difference between the output and the input (after aligning the delay) is at least 30 dB below the input | `check` |
| A5 | Latency | 150 ms or less from the wav source to the output. Does not pass on GitHub Actions runners because of their devices, so the value is kept in the report and does not decide the workflow result | `e2e` |
| A6 | No dropouts | With the default settings, the output has 0 dropouts (buffer underruns). May not pass on runners, so the value is kept in the report and does not decide the workflow result | `e2e` |

## Q. Lowering the pitch (TD-PSOLA) and output level

Checks for the processing used below 0 semitones and for the level of the shifted voice. Q1 and Q10–Q13 use a synthetic vowel with variation, or the macOS text-to-speech voices (Kyoko and Daniel). How to make them is at the top of `scripts/quality_check.py`.

| Name | What it checks | Pass condition | Checked by |
| --- | --- | --- | --- |
| Q1 | Periodicity when lowered | A synthetic vowel with variation (F0 110 and 220 Hz, 5.5 Hz ±30 cent vibrato, jitter 0.5%, shimmer 3%, breath noise at −30 dB) shifted by −5 and −12 semitones with formant ×1.0 and ×0.8 has an HNR (Praat harmonicity, cc, 10 ms, mean with a 30 Hz floor) no more than 2.0 dB below that of the ideal output re-synthesised at the target F0 and formants | `quality` |
| Q2 | Resonances when lowered | In the lowering cases of A2 (F0 100 and 150 Hz, −5 and −12 semitones), F1–F3 are within ±5%. For −12 semitones with formant ×0.8 and ×1.2, F1–F3 move by that factor within ±7% and F0 is within ±1% of the target | `check` |
| Q3 | Switching between the two methods | With a synthetic vowel at F0 150 Hz, switching from −5 to +5 semitones at 1.0 s and back to −5 at 2.0 s, the RMS (20 ms windows every 5 ms) in the 100 ms around each switch is 0.5–2 times the median RMS of the 200 ms before the switch | `check` |
| Q4 | Level | Synthetic vowels at F0 100 and 150 Hz shifted by ±5 and ±12 semitones, formant ×0.8 and ×1.2, and −12 semitones with ×0.8 have an output RMS after 0.5 s within ±2 dB of the input | `check` |
| Q5 | Silence is not amplified | White noise alone at −60 dBFS shifted by ±12 semitones has an output RMS no more than 1 dB above the input | `check` |
| Q10 | No original pitch mixed in when lowered | Text-to-speech voices shifted by −12 semitones, and by −12 semitones with ×0.8: of the 10 ms frames where the input is voiced, at most 2% have an output F0 within ±8% of the input F0 | `quality --speech` |
| Q11 | No extra noise at word onsets | Same voices and shifts as Q10: the mean output HNR over the first 5 frames (50 ms) after voicing starts is no more than 0.5 dB below that of Signalsmith Stretch alone | `quality --speech` |
| Q12 | The original pitch is not heard in a lowered voice | Same voices and shifts as Q10: for each voiced run (0.2 s or longer), the ratio of the odd to even harmonics of the target F0 (up to 3 kHz, median over 10 ms frames) is −4 dB or more | `quality --speech` |
| Q13 | Consonants are not muffled | Same voices and shifts as Q10: in frames where the input is unvoiced, louder than −45 dBFS, and has a spectral centroid above 4 kHz, the median output centroid is within ±10% of the input centroid times the formant factor | `quality --speech` |

Q6–Q9 were checked once when TD-PSOLA was added (results for raising the pitch unchanged, latency still 3360 samples, processing cost, Windows workflows passing). There is no tool for them.

## B. Display

| Name | What it checks | Pass condition | Checked by |
| --- | --- | --- | --- |
| B1 | F1–F4 estimate | For synthetic vowels (F0 100, 150, and 220 Hz), the estimated F1–F4 are within ±2% of Praat's values for the same signal with the same settings (Burg method, same ceiling and number of poles). The difference from the true values is shown for reference | `check` (Praat's values are `PRAAT_REFERENCE` in `check.rs`) |
| B3 | Harmonics | For a signal at F0 150 Hz with only H1–H8, at least 20 dB above the noise floor, the number of harmonics seen is 8 and H1–H8 are within ±1% of the true frequencies | `check` |
| B4 | Original and shifted are aligned | For a synthetic vowel shifted by +5 semitones, the original and shifted values on screen are from the same time | `selftest` (`B4-shift`) |
| B5 | Holding everything | After holding, the original and shifted displays stop changing and the output voice does not break up. Doing it again brings both back to the latest | `selftest` (`B5-freeze`, `B5-resume`) |
| B6 | Display updates | At least 30 updates per second, and the display is at most 100 ms behind the sound | `selftest` |
| B7 | Holding one side | With only SHIFTED held, changing the voice leaves SHIFTED as it was when held, ORIGINAL keeps moving, and the time row of the values table shows the same time on both sides while live, and while held shows the held time (the same as in the held plot's label) in the held colour next to the live time, with no separate note. Doing it again brings only SHIFTED back to the latest. The same holds for ORIGINAL | `selftest` |
| B8 | ALL after holding one side | With only SHIFTED held, using ALL holds both, and using it again resumes both | `selftest` |
| B9 | F1–F4 do not swap on a steady voice | For steady vowels with variation (jitter 1%, shimmer 5%) and breath noise (HNR 20 and 10 dB) (F0 120, 183, and 250 Hz; the same 5 resonances as in "Test signals"), the difference of the scrolling plot's F2–F4 from the true values (root mean square of the log difference) is within 3%, F1 is no worse than the untracked value (`analyze --raw-formants`), and no voiced frame lacks a value | `formant_stability.py` |
| B10 | Moving voices are not dulled | For vowels whose F2 moves between 1000 and 2000 Hz in 150 ms and 40 ms, the difference of the scrolling plot's F2 from the true value is no worse than the untracked value. The scrolling plot's value is treated as 2 frames old when aligning times | `formant_stability.py` |
| B11 | Single-frame outliers | For a voice wav (`--speech`), the share of frames that are more than 15% away from both neighbours while the neighbours are within 7% of each other is 0.5% or less for each of F1–F4 | `formant_stability.py` |
| B12 | No extra jumps in a voice | For a voice wav, the share of frames that differ from the next frame by more than 15% does not rise by more than 1 point above the untracked value for any of F1–F4 (for a text-to-speech voice, 1 frame is about 0.8 points. Real fast movements also count, so a decrease is not required) | `formant_stability.py` |

## C. On Windows

| Name | What it checks | Pass condition | Checked by |
| --- | --- | --- | --- |
| C2 | Device lists | The input list does not have the VB-CABLE recording side, and the monitor cannot be the output device. Fetching the list again keeps the chosen devices that remain and clears removed ones | Unit tests (`voice-domain` `session.rs`, `voice-app`) |
| C4 | Cannot start until an output is chosen | While no output device is chosen, the start button cannot be pressed (on the first start on Windows without VB-CABLE, the output is empty) | `selftest` |
| C5 | Saving an image | Holding and saving writes a PNG that shows the same display and values as the held screen, the original and shifted times, whether each is held, the settings, and when it was saved. The save dialog opens in the folder last saved to, or in the Pictures folder if that folder no longer exists. Saving is not possible while both sides are live | `selftest` (`C5-save`), unit tests (the folder; `voice-tauri` `preferences.rs`) |
| C6 | Devices at start | On the first start, the input is the default recording device, the output is the VB-CABLE playback side (if installed; otherwise empty), and the monitor is the default playback device (empty if it is the same as the output). Remembered devices are used at start, and remembered devices that are no longer in the list are skipped, keeping the default | Unit tests (`voice-domain` `session.rs`, `voice-app`) |

A7 (offset between the output and the monitor), B2 (agreement with Praat on a real voice), C1 (other software receives the voice), and C3 (monitor) needed a person with real devices and were removed. The names are not reused.

## D. Devices section

| Name | What it checks | Checked by |
| --- | --- | --- |
| D1 | Start and stop share one button that switches between START and STOP. IN and OUT cannot be changed while running | `selftest` (`start-0st`, `run-toggle`) |
| D2 | The start button cannot be pressed while no output is chosen | `selftest` (`C4`) |
| D3 | The monitor is a button at least 24 px high, not a checkbox. Pressing it flips `aria-pressed` and switches the monitor | `selftest` (`monitor-button`, `monitor-toggle`) |
| D4 | The source sent to the output and monitor (SRC) is one button that switches between SHIFTED and ORIGINAL on each press | `selftest` (`routing`) |
| D5 | In both English and Japanese, each label in the devices section fits on one line and stays within the column | `selftest` (`devices-fit`) |

## S. Appearance and keyboard

| Name | What it checks | Checked by |
| --- | --- | --- |
| S1 | Outside `:root`, `style.css` has no colour values (hex, `rgb()`, `hsl()`); the TypeScript sources (including `render.ts` and `png.ts`) have none either; `:root` defines the spacing (`--space-1` to `3`), corner (`--radius-s`, `-m`), and type (`--text-s` to `l`) variables | Unit tests (`frontend/src/tokens.test.ts`) |
| S2 | No emoji or symbol icons in text or buttons; icons are SVG; every button is 24 px high (the same in English and Japanese) | `selftest` (`S2-icons`) |
| S3 | Both plot titles have an F1–F4 legend in the colours `--formant-1` to `4`. The F1–F4 rows of the values table start with a square of the same colour | `selftest` (`S3-legend`) |
| S4 | While there is voice, F1–F4 in the value column to the right of each spectrogram match the same side in the values table | `selftest` (`S4-values`) |
| S5 | Harmonic dots are drawn 1 px high in the `--harmonic` colour | `selftest` (`S5-harmonics`) |
| S6 | The spectrum has a dB scale from `0` to `−100` and a `dB` label on its left | `selftest` (`S6-db-axis`) |
| S7 | Hovering over a spectrogram or the spectrum shows a label with the frequency (and dB on the spectrum) that matches the value computed from the pointer position. It disappears when the pointer leaves, and saved PNGs do not include it | `selftest` (`S7-cursor`) |
| S8 | Holding one side gives that plot a 2 px `--hold` border, puts a `HOLD <time> s` label in its title, fills that side's switch, and sets its `aria-pressed` to true. Resuming undoes all of these. Holding does not change the plot size | `selftest` (`S8-hold`, `S8-resume`, `S8-one-side`) |
| S10 | ↑, ↓, →, and ← (with and without Shift) change the pitch and formant fields by fixed steps and send them to the shifter. Nothing changes while typing in a number field | `selftest` (`S10-arrows`) |
| S12 | `?` opens the list of keyboard shortcuts and Esc closes it. The first five rows of the list are `Space`, `Tab`, `A`, `S`, and `D`, there is no row for saving, and the note is shown. While the list is open, `Space`, `Tab`, `A`, `S`, `D`, and the arrows change nothing | `selftest` (`S12`) |
| S15 | `Space` turns monitoring on and off and leaves the hold switches alone. With no monitor device chosen, monitoring stays off and a message appears | `selftest` (`S15-monitor-key-rejected`, `S15-monitor-key`; the latter only when there is a device to choose for the monitor) |
| S16 | `Tab` and `Shift+Tab` switch SRC and back, and the `keydown` is `preventDefault`ed (focus does not move) | `selftest` (`S16-route-key`) |
| S17 | `A`, `S`, and `D` hold and resume both sides, the original side, and the shifted side. `S` while held does not save a PNG. `0`, `1`, and `2` change nothing | `selftest` (`S17-hold-keys`) |
| S18 | In the compact view, `Space` and `Tab` work, and `A`, `S`, `D`, and the arrows do nothing | `selftest` (`S18-compact-keys`) |
| S19 | With focus on the volume slider, the monitor select, or the monitor button, `Space` and `Tab` still work and are `preventDefault`ed, and the selected monitor device does not change. The arrows on the slider are not `preventDefault`ed and the pitch does not change | `selftest` (`S19-focus-keys`) |
| S20 | While typing in a number field, with Ctrl, or on key repeat, `Space`, `Tab`, and `A` change nothing | `selftest` (`S20-ignored-keys`) |

S9 (key hints on the hold switches) and S14 (length of the left column) were replaced by U1 and U2. S13 checked that the self-test results did not change during that change. S11 (`S` saves) was removed with the save key; `S` now holds the original side. A synthetic `keydown` does not trigger the browser's default actions (pressing a button, opening a select, moving focus), so S16 and S19 check that those are stopped by whether the event was `preventDefault`ed.

## U. Top bar, left column, settings panel, and compact view

| Name | What it checks | Checked by |
| --- | --- | --- |
| U1 | At 1280×860 and 960×640, in English and Japanese, the top bar has the lock, start/stop, the three hold switches, save, the status message, and the settings, ?, language, and view buttons; no part is wider than the window; the bar is 32 px high. The bar has no key hints, and there are 2 divider lines (1 px wide, 16 px high) between the groups of hold switches | `selftest` (`U1-bar`, `U1-bar-960x640`) |
| U2 | In the same 4 cases, the left column has only the `PITCH`, `FORMANT`, and `DEVICES` sections, and its `scrollHeight` does not exceed its `clientHeight` | `selftest` (`U2-column`, `U2-column-960x640`) |
| U3 | The settings button opens the panel; the analysis settings are in the panel; Esc and clicking outside close it; focus returns to the settings button | `selftest` (`U3-settings`) |
| U4 | The values table stays within its frame, and the two spectrograms have the same width and height | `selftest` (`U4-table`) |
| U5 | Before starting, the `ORIGINAL` plot shows a guide that disappears on start. Without VB-CABLE, the guide asks to choose an output, and the word `VB-CABLE` appears nowhere on screen | `selftest` (`U5-guide`, `cable-found`) |
| U6 | The spectrum title has an `ORIGINAL` and `SHIFTED` legend; the dashed sample is in `--before` and the solid sample in `--accent` | `selftest` (`U6-spectrum-legend`) |
| U7 | The pitch field shows values like `+5.0` and `0.0`, and the formant field like `1.00`. Typing `+5` and Enter gives `+5.0`; `abc` goes back to the previous value | `selftest` (`U7-digits`) |
| U8 | Each value has `−` and `+` buttons: pitch ±0.1 (±1 with Shift), formant ±0.01 (±0.05 with Shift). The wheel uses the same steps. At formant ×1.00 the slider is centred, and its ends are `0.50` and `2.00` | `selftest` (`U8-controls`) |
| U9 | Saved messages, dropouts, and errors appear in the status message on the top bar, and the left column has no message text. The saved message disappears after 5 s | `selftest` (`U9-status`) |
| U10 | With only SHIFTED held, the ORIGINAL plot shows dashed lines at the heights of SHIFTED's F1–F4 in the values table, and its title shows a `HOLD <time> s (SHIFTED)` legend. The same the other way round. With both held, neither shows them; resuming removes them | `selftest` (`U10-overlay`) |
| U11 | In a PNG saved with one side held, the pixels at the target heights in the live side's plot are in that formant's colour | `selftest` (`U11-overlay-png`) |
| U12 | In the compact view, the hold switches, save, settings, and ? buttons are hidden, the sections are in the order `PITCH`, `FORMANT`, `DEVICES`, and the left column is 250 px wide | `selftest` (`U12-compact`, `compact-view`) |

U13 checked that the self-test results did not change during that change.

## L. Lock

The lock button is the first part of the top bar. While locked, nothing but the lock button changes the state. The self-test sends synthetic events, which do not run the browser's default actions (focus moving with `Tab`, `Space` or `Enter` pressing a focused button, dragging a slider, opening a select). So for keys it checks that they are `preventDefault`ed, and for the mouse that the parts are `inert`.

| Name | What it checks | Checked by |
| --- | --- | --- |
| L1 | In the full and compact views, the bar starts with the lock button (24×24 px), then a divider (1 px wide, 16 px high, vertically centred on the button), then start/stop 8 px to the right of the lock. Locking does not move start/stop or any part to its right. In the compact view the lock, start/stop, language, and view buttons are on one row | `selftest` (`L1-lock-button`, `L1-lock-compact`) |
| L2 | Pressing the lock sets `aria-pressed` to `true`, the name (`title` and `aria-label`) to `Unlock controls` / `ロックを解除する`, and the icon to `lock`. Pressing it again gives `false`, `Lock controls` / `操作をロックする`, and `unlock` | `selftest` (`L2-lock-toggle`) |
| L3 | While locked, pressing start/stop, the three hold switches, save, settings, ?, language, view, MON, SRC, `−`, `+`, and reset changes nothing (running, the hold switches, MON, SRC, language, view, pitch, formant, the settings panel, the shortcut list, the status message), and the parts on the bar other than the lock, its divider, and the status message, and the left column, are `inert` | `selftest` (`L3-lock-buttons`) |
| L4 | While locked, the pitch, formant, and volume sliders, typing in the value fields and the ceiling field, the IN, OUT, and monitor selects, and the wheel on the value field and sliders change no value, and the controls show the current values again | `selftest` (`L4-lock-fields`) |
| L5 | While locked, `Space`, `Tab`, `Shift+Tab`, `A`, `S`, `D`, the arrows (also with `Shift`), `?`, and `Enter`, sent from the body, the volume slider, the monitor select, the MON and start/stop buttons, the pitch field, and the lock button, are all `preventDefault`ed and change nothing; the controls cannot take focus; the lock button has `tabindex="-1"` | `selftest` (`L5-lock-keys`) |
| L6 | While locked, the parts on the bar and the contents of the left column are drawn at opacity 0.5 or less, and the lock button, the status message, the section frames, and the plots, spectrum, and values table at 1. The lock button is filled with `--ink` with the icon in `--on-fill`. While running and locked, the number of analysis results received keeps growing and STOP does not stop | `selftest` (`L6-lock-look`; `L6-lock-running` runs only with VB-CABLE) |
| L7 | After unlocking, SRC switches by button and by `Tab`, and `−`, the slider, and reset change the pitch; nothing is `inert` or dimmed | `selftest` (`L7-lock-released`) |
| L8 | Every start is unlocked. The self-test ends locked, so running it twice with the same `VOICE_SHIFT_SCOPE_CONFIG_DIR` checks that the lock is not remembered | `selftest` (`L8-start-unlocked`) |

The self-test also writes `shots/L-unlocked-full.png`, `L-locked-full.png`, `L-unlocked-compact.png`, `L-locked-compact.png`, and (with VB-CABLE) `L-locked-running.png`.

## P. Remembering the pitch and formant

| Name | What it checks | Checked by |
| --- | --- | --- |
| P1 | When `preferences.json` has remembered values (−12 semitones, ×0.8), the fields show "−12.0" and "0.80" at start. Without them, "0.0" and "1.00" | `selftest` (`P1-shift-restored`; the expected values come from the environment variable `VOICE_SHIFT_SCOPE_TEST_EXPECT_SHIFT`) |
| P2 | After changing the values and closing, `preferences.json` has the last values | Unit tests |
| P3 | Remembered values out of range fall back to the defaults | Unit tests |
| P4 | Adding the remembered values keeps the other remembered values (devices, analysis settings, language, view) | Unit tests |
| P5 | `ci.yml` and `windows-ui.yml` succeed and `P1-shift-restored` passes | Workflows |

## Self-test items without a letter

| Item | What it checks |
| --- | --- |
| `language-toggle` | The language switch changes the text, and switching again changes it back |
| `fits-window` | The screen fits in the window without vertical scrolling |
| `held-survives-resize` | A held plot is still drawn after the plot size changes |
| `window-960x640`, `window-1280x860` | Shrinks the window to the minimum 960×640, takes a shot, and restores 1280×860 |
| `error` | Written when the self-test itself stops with an exception. FAIL if present |
