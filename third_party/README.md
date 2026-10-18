# third_party

- `signalsmith-stretch/`: Signalsmith Stretch 1.4.0（MIT、https://github.com/Signalsmith-Audio/signalsmith-stretch の tag 1.4.0 の `signalsmith-stretch.h` と `LICENSE.txt`）
- `signalsmith-linear/`: Signalsmith Linear 0.6.4（MIT、https://github.com/Signalsmith-Audio/linear の tag 0.6.4。`tests/` を除いた）

どちらも手を加えていない。`crates/voice-dsp/build.rs` が `crates/voice-dsp/cpp/stretch_wrapper.cpp` と一緒に build する。
