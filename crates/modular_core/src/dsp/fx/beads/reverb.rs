//! Beads' reverb: four input allpass diffusers into a two-delay loop, with a
//! modulated first diffuser and slowly modulated long delays, stored at
//! 16 bits. The hardware's delay lengths scale with the ratio of the rate
//! the reverb runs at to the hardware's, so the reverb keeps its timing, and
//! stretch further with its size.

use super::primitives::{CosineOscillator, Frame, Ramp, clip16};

/// Lengths of ap1–ap4, dap1a, dap1b, del1, dap2a, dap2b, del2, in hardware
/// samples.
const LENGTHS: [i32; 10] = [150, 214, 319, 527, 2182, 2690, 4501, 2525, 2197, 6312];

/// Free samples after each line, so a modulated read near a line's end stays
/// clear of the next line after rounding.
const GUARD: i32 = 2;

/// Room each line takes in a buffer for up to `max_scale` times the
/// hardware lengths.
fn spaced(length: i32, max_scale: f32) -> i32 {
    (length as f32 * max_scale).ceil() as i32 + GUARD
}

/// Buffer length holding every line at up to `max_scale` times the hardware
/// lengths.
fn buffer_len(max_scale: f32) -> usize {
    let total: i32 = LENGTHS.iter().map(|&l| spaced(l, max_scale)).sum();
    (total as usize).next_power_of_two()
}

/// Delay layout and modulation depths at one rate, in samples at that rate
/// and size 1.
#[derive(Clone, Copy, Default, PartialEq)]
struct Layout {
    mask: i32,
    lengths: [f32; 10],
    bases: [i32; 10],
    /// Centre and depth of ap1's smear, and the offset it writes back to.
    smear: [f32; 2],
    smear_write: f32,
    /// Centre and depth of the modulated reads of del2 and del1.
    del2_read: [f32; 2],
    del1_read: [f32; 2],
}

impl Layout {
    fn new(scale: f32, max_scale: f32, buffer_len: usize) -> Self {
        let mut bases = [0; 10];
        for i in 1..10 {
            bases[i] = bases[i - 1] + spaced(LENGTHS[i - 1], max_scale);
        }
        Self {
            mask: buffer_len as i32 - 1,
            lengths: LENGTHS.map(|l| l as f32 * scale),
            bases,
            smear: [10.0 * scale, 80.0 * scale],
            smear_write: 100.0 * scale,
            del2_read: [6261.0 * scale, 50.0 * scale],
            del1_read: [4460.0 * scale, 40.0 * scale],
        }
    }
}

const AP1: usize = 0;
const AP2: usize = 1;
const AP3: usize = 2;
const AP4: usize = 3;
const DAP1A: usize = 4;
const DAP1B: usize = 5;
const DEL1: usize = 6;
const DAP2A: usize = 7;
const DAP2B: usize = 8;
const DEL2: usize = 9;

const KAP: f32 = 0.625;

#[inline]
fn compress(x: f32) -> i16 {
    clip16((x * 32768.0) as i32) as i16
}

#[inline]
fn decompress(v: i16) -> f32 {
    v as f32 / 32768.0
}

/// Accumulator-style context for one sample of the reverb program.
struct Context<'a> {
    buffer: &'a mut [i16],
    layout: &'a Layout,
    write_ptr: i32,
    accumulator: f32,
    previous_read: f32,
    lfo: [f32; 2],
    /// Multiplier on every delay length and modulation depth.
    size: f32,
}

impl Context<'_> {
    #[inline]
    fn index(&self, line: usize, offset: i32) -> usize {
        ((self.write_ptr + self.layout.bases[line] + offset) & self.layout.mask) as usize
    }

    /// Read the oldest sample of `line` at the current size.
    #[inline]
    fn read_tail(&mut self, line: usize, scale: f32) {
        self.read_at(line, self.layout.lengths[line] * self.size - 1.0, scale);
    }

    #[inline]
    fn write(&mut self, line: usize, offset: i32, scale: f32) {
        let i = self.index(line, offset);
        self.buffer[i] = compress(self.accumulator);
        self.accumulator *= scale;
    }

    #[inline]
    fn write_all_pass(&mut self, line: usize, scale: f32) {
        self.write(line, 0, scale);
        self.accumulator += self.previous_read;
    }

    #[inline]
    fn interpolate(&mut self, line: usize, [offset, amplitude]: [f32; 2], lfo: usize, scale: f32) {
        self.read_at(
            line,
            (offset + amplitude * self.lfo[lfo]) * self.size,
            scale,
        );
    }

    #[inline]
    fn read_at(&mut self, line: usize, offset: f32, scale: f32) {
        let integral = offset as i32;
        let fractional = offset - integral as f32;
        let a = decompress(self.buffer[self.index(line, integral)]);
        let b = decompress(self.buffer[self.index(line, integral + 1)]);
        let x = a + (b - a) * fractional;
        self.previous_read = x;
        self.accumulator += x * scale;
    }

    #[inline]
    fn lp(&mut self, state: &mut f32, coefficient: f32) {
        *state += coefficient * (self.accumulator - *state);
        self.accumulator = *state;
    }
}

#[derive(Default)]
pub struct Reverb {
    buffer: Box<[i16]>,
    /// The largest multiple of the hardware lengths the buffer holds.
    max_scale: f32,
    /// The largest size at the running rate.
    max_size: f32,
    layout: Layout,
    write_ptr: i32,
    lfo: [CosineOscillator; 2],
    lp_1: f32,
    lp_2: f32,
    dry: Ramp,
    wet: Ramp,
    send: Ramp,
    size: Ramp,
}

/// Dry gain and send into the tank (summed left plus right) for a reverb
/// `mix` at loop gain `time`.
#[inline]
fn mix_gains(mix: f32, time: f32) -> (f32, f32) {
    let input_gain = 0.2 * (1.0 - 0.333 * time);
    let send_sqrt = mix * (2.0 - mix);
    let send = send_sqrt * (2.0 - send_sqrt);
    ((1.0 - mix) * (1.0 + mix), send * input_gain)
}

/// Reverb settings for one control block.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReverbSettings {
    /// Send/dry balance, 0 to 0.9.
    pub mix: f32,
    /// Loop gain.
    pub time: f32,
    /// Loop low-pass coefficient.
    pub lp: f32,
    /// Multiplier on the delay lengths, from 1.
    pub size: f32,
}

impl Reverb {
    /// Allocate a tank for rates up to `max_scale` times the hardware's.
    /// Main thread only.
    pub fn allocate(&mut self, max_scale: f32) {
        self.buffer = vec![0; buffer_len(max_scale)].into_boxed_slice();
        self.max_scale = max_scale;
    }

    /// Run at `sample_rate`, `scale` times the hardware's rate, at size 1. A
    /// new scale moves the lines, so it clears the tank.
    pub fn set_sample_rate(&mut self, sample_rate: f32, scale: f32) {
        // The LFOs advance once every 32 samples.
        self.lfo[0].init_approximate(0.5 / sample_rate * 32.0);
        self.lfo[1].init_approximate(0.3 / sample_rate * 32.0);
        debug_assert!(scale <= self.max_scale);
        let layout = Layout::new(scale, self.max_scale, self.buffer.len());
        if layout != self.layout {
            self.clear();
        }
        self.layout = layout;
        self.max_size = self.max_scale / scale;
        self.size.reset(1.0);
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0);
        self.write_ptr = 0;
    }

    pub fn start_block(&mut self, s: &ReverbSettings, block_size: usize) {
        let input_gain = 0.2 * (1.0 - 0.333 * s.time);
        let output_gain = 0.4 / input_gain;
        let (dry, send) = mix_gains(s.mix, s.time);
        self.wet.start(output_gain, block_size);
        self.dry.start(dry, block_size);
        self.send.start(send, block_size);
        self.size
            .start(s.size.clamp(1.0, self.max_size), block_size);
    }

    pub fn process(&mut self, input: Frame, s: &ReverbSettings) -> Frame {
        let dry = self.dry.next();
        let wet_gain = self.wet.next();
        let send = self.send.next();
        let size = self.size.next();
        let (wet_l, wet_r) = self.tank((input.l + input.r) * send, size, s);
        Frame::new(
            input.l * dry + wet_l * wet_gain,
            input.r * dry + wet_r * wet_gain,
        )
    }

    /// Run the tank for one sample at `size` with `send` added to its input.
    #[inline]
    fn tank(&mut self, send: f32, size: f32, s: &ReverbSettings) -> (f32, f32) {
        let klp = s.lp;
        let krt = s.time;

        self.write_ptr -= 1;
        if self.write_ptr < 0 {
            self.write_ptr += self.buffer.len() as i32;
        }
        let lfo = if self.write_ptr & 31 == 0 {
            [self.lfo[0].next(), self.lfo[1].next()]
        } else {
            [self.lfo[0].value(), self.lfo[1].value()]
        };

        let mut c = Context {
            buffer: &mut self.buffer,
            write_ptr: self.write_ptr,
            accumulator: 0.0,
            previous_read: 0.0,
            layout: &self.layout,
            lfo,
            size,
        };
        let l = self.layout;

        // Smear AP1 inside the loop.
        c.interpolate(AP1, l.smear, 0, 1.0);
        c.write(AP1, (l.smear_write * size).round() as i32, 0.0);

        c.accumulator += send;

        for line in [AP1, AP2, AP3, AP4] {
            c.read_tail(line, KAP);
            c.write_all_pass(line, -KAP);
        }
        let apout = c.accumulator;

        c.interpolate(DEL2, l.del2_read, 1, krt);
        c.lp(&mut self.lp_1, klp);
        c.read_tail(DAP1A, -KAP);
        c.write_all_pass(DAP1A, KAP);
        c.read_tail(DAP1B, KAP);
        c.write_all_pass(DAP1B, -KAP);
        c.write(DEL1, 0, 1.0);
        let wet_l = c.accumulator;

        c.accumulator = apout;
        c.interpolate(DEL1, l.del1_read, 0, krt);
        c.lp(&mut self.lp_2, klp);
        c.read_tail(DAP2A, KAP);
        c.write_all_pass(DAP2A, -KAP);
        c.read_tail(DAP2B, -KAP);
        c.write_all_pass(DAP2B, KAP);
        c.write(DEL2, 0, 1.0);
        let wet_r = c.accumulator;
        (wet_l, wet_r)
    }
}
