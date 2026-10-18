// Signalsmith Stretch（third_party/signalsmith-stretch、MIT）を Rust から呼ぶための C の口。mono だけを扱う。
#include "signalsmith-stretch/signalsmith-stretch.h"

using Stretch = signalsmith::stretch::SignalsmithStretch<float>;

extern "C" {

Stretch *vss_stretch_create(float sample_rate) {
    auto *s = new Stretch();
    s->presetDefault(1, sample_rate);
    return s;
}

Stretch *vss_stretch_create_custom(int block_samples, int interval_samples) {
    auto *s = new Stretch();
    s->configure(1, block_samples, interval_samples);
    return s;
}

void vss_stretch_destroy(Stretch *s) { delete s; }

void vss_stretch_reset(Stretch *s) { s->reset(); }

int vss_stretch_input_latency(const Stretch *s) { return s->inputLatency(); }

int vss_stretch_output_latency(const Stretch *s) { return s->outputLatency(); }

void vss_stretch_set_transpose_semitones(Stretch *s, float semitones, float tonality_limit) {
    s->setTransposeSemitones(semitones, tonality_limit);
}

void vss_stretch_set_formant_factor(Stretch *s, float multiplier, int compensate_pitch) {
    s->setFormantFactor(multiplier, compensate_pitch != 0);
}

// F0 の目安。Nyquist ではなく標本化の速さに対する比（Hz / sample_rate）。
void vss_stretch_set_formant_base(Stretch *s, float relative_frequency) {
    s->setFormantBase(relative_frequency);
}

void vss_stretch_process(Stretch *s, const float *input, int input_len, float *output, int output_len) {
    const float *in[1] = {input};
    float *out[1] = {output};
    s->process(in, input_len, out, output_len);
}

}
