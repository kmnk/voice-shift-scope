# Checks on real Windows hardware

A checklist for checking, on the user's Windows, what cannot be checked on GitHub Actions runners (a real microphone and headphones, plugging and unplugging USB devices, your own voice, and the standard Windows screens). Fill in the result columns and paste them into a GitHub issue or put them in a shared folder.

The check names are the same as in [criteria.md](criteria.md).

## Preparation

- Install VB-CABLE (https://vb-audio.com/Cable/ )
- Unpack the release zips. If an earlier version is there, delete its folder before unpacking
- `voice-shift-scope.exe` inside is the app, and `voice-cli.exe` is the tool for checks

## 1. Checks with the tool (commands)

Open PowerShell in the folder where the zip was unpacked.

1. `.\voice-cli.exe devices > devices.json`
   - Check: `devices.json` has 2 VB-CABLE devices, both with `"driver": "VB-Audio Virtual Cable"`
   - Result:
2. `.\voice-cli.exe e2e --seconds 10 --semitones 0 > e2e-latency.json`
   - This plays a test sound into VB-CABLE. Stop any other software recording CABLE Output first
   - Check: `latency_ms` of `A5` (the delay without the microphone), and whether `output_underruns` of `A6` is 0
   - Result:
3. `.\voice-cli.exe e2e --seconds 1800 --semitones 5 > e2e-30min.json` (takes 30 minutes)
   - Check: `output_underruns` of `A6` is 0 (A6)
   - Result:

Please send `devices.json`, `e2e-latency.json`, and `e2e-30min.json`.

## 2. Checks on screen

| Check | What to do | What to check | Result |
| --- | --- | --- | --- |
| C6 | Start the app | The input is the Windows default recording device, and the monitor is the default playback device | |
| C6 | Set the default playback device to CABLE Input in the Windows sound settings, then start the app (change it back afterwards) | The monitor is `(none)` | |
| C2 | Open the input and monitor lists | The input list has no CABLE Output and the monitor list has no CABLE Input. Other devices are listed by name | |
| C2 | Plug in a USB device and restart the app. Unplug it and restart | It appears in the list when plugged in and disappears when unplugged | |
| C1 | Start, set the pitch to +5, and speak. Record CABLE Output with Voice Recorder or Audacity | The shifted voice is recorded | |
| C3 | Turn the monitor on, choose headphones, and change the volume | The shifted voice is heard. Changing the monitor volume does not change the level recorded in C1 | |
| A7 | Run for 30 minutes with the monitor on, then compare the output and the monitor by ear | No offset can be heard | |
| A5 | Clap in front of the microphone, and record the microphone (a recording separate from CABLE Output) and the headphone sound at the same time in Audacity | The peaks of the waveforms are 150 ms or less apart | |
| C5 | Hold ALL and press SNAPSHOT. Choose where to save and save | A PNG is saved, and in the standard Windows image viewer the three displays and the values, the original and shifted times and settings, and when it was saved can be read | |
| C5 | Close and restart the app, hold, and save | The save dialog opens in the previous folder | |
| C5 | Delete that folder, then save | The dialog opens in the Pictures folder | |
| C5 | Set both sides to LIVE | SNAPSHOT cannot be pressed | |

## 3. Your own voice and Praat (B2)

1. Record a, i, u, e, and o, each held for 2 seconds, as separate 48 kHz wavs (with Audacity or similar)
2. Send the wavs. The recordings do not go into the repository; only the check results are kept
