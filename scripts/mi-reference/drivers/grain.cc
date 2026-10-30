// Plaits' granular formant engine (grain_engine.cc) driven block by block with
// Operator's control mapping: exact pitch conversions in place of Plaits'
// quantised lookup tables, knobs in volts. Each block record is its inputs then
// its 12 `out` and 12 `aux` samples, before the output gain.

#include <cmath>
#include <cstdio>

#include "plaits/dsp/oscillator/grainlet_oscillator.h"
#include "plaits/dsp/oscillator/z_oscillator.h"
#include "stmlib/dsp/filter.h"

using namespace plaits;
using namespace stmlib;

const size_t kBlock = 12;
const float kC4 = 261.625565f;

static float knob(float volts) {
  float v = volts * 0.2f;
  return v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v);
}

int main(int argc, char** argv) {
  GrainletOscillator grainlet[2];
  ZOscillator z;
  OnePole dc_blocker[2];
  grainlet[0].Init();
  grainlet[1].Init();
  z.Init();
  dc_blocker[0].Init();
  dc_blocker[1].Init();

  FILE* f = fopen(argv[1], "wb");
  const int blocks = 600;
  for (int b = 0; b < blocks; ++b) {
    const float t = float(b) / blocks;
    const int segment = b / 100;
    // pitch in cycles per sample, from ~33 Hz to ~11 kHz
    float note = segment == 3 ? 2.0f + 3.5f * t : -3.0f + segment * 1.3f;
    if (segment == 5) note = -1.0f + ((b % 7) * 0.5f);
    float f0 = kC4 * powf(2.0f, note) * (1.0f / 48000.0f);
    float in[7] = {
        f0,
        -2.0f + 6.0f * sinf(float(b) * 0.05f),       // formant, V/Oct
        2.5f + 2.6f * sinf(float(b) * 0.031f + 1.0f), // ratio
        2.5f + 2.6f * sinf(float(b) * 0.023f + 2.0f), // width
        2.5f + 2.6f * sinf(float(b) * 0.041f),        // cutoff
        2.5f + 2.6f * sinf(float(b) * 0.017f + 0.5f), // shape
        2.5f + 2.6f * sinf(float(b) * 0.013f + 1.5f), // mode
    };

    const float f1 = kC4 * powf(2.0f, in[1]) * (1.0f / 48000.0f);
    const float harmonics = knob(in[2]);
    const float morph = knob(in[3]);
    const float ratio = exp2f(4.0f * harmonics - 2.0f);
    const float carrier_bleed = harmonics < 0.5f ? 1.0f - 2.0f * harmonics : 0.0f;
    const float carrier_bleed_fixed = carrier_bleed * (2.0f - carrier_bleed);
    const float narrowing = 1.0f - fabsf(f0) * 24.0f;
    const float carrier_shape =
        0.33f + (morph - 0.33f) * (narrowing > 0.0f ? narrowing : 0.0f);

    float out[kBlock], aux[kBlock];
    grainlet[0].Render(f0, f1, carrier_shape, carrier_bleed_fixed, out, kBlock);
    grainlet[1].Render(f0, f1 * ratio, carrier_shape, carrier_bleed_fixed, aux, kBlock);
    dc_blocker[0].set_f<FREQUENCY_DIRTY>(0.3f * fabsf(f0));
    for (size_t i = 0; i < kBlock; ++i) {
      out[i] = dc_blocker[0].Process<FILTER_MODE_HIGH_PASS>(out[i] + aux[i]);
    }

    const float cutoff = fabsf(f0) * exp2f(8.0f * knob(in[4]));
    z.Render(f0, cutoff, knob(in[5]), knob(in[6]), aux, kBlock);
    dc_blocker[1].set_f<FREQUENCY_DIRTY>(0.3f * fabsf(f0));
    dc_blocker[1].Process<FILTER_MODE_HIGH_PASS>(aux, kBlock);

    fwrite(in, sizeof(float), 7, f);
    fwrite(out, sizeof(float), kBlock, f);
    fwrite(aux, sizeof(float), kBlock, f);
  }
  fclose(f);
  return 0;
}
