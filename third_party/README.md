# third_party

- `signalsmith-stretch/`: Signalsmith Stretch 1.4.0 (MIT; `signalsmith-stretch.h` and `LICENSE.txt` from tag 1.4.0 of https://github.com/Signalsmith-Audio/signalsmith-stretch )
- `signalsmith-linear/`: Signalsmith Linear 0.6.4 (MIT; tag 0.6.4 of https://github.com/Signalsmith-Audio/linear , without `tests/`)

Neither is modified. `crates/voice-dsp/build.rs` builds them together with `crates/voice-dsp/cpp/stretch_wrapper.cpp`.
