//! Offline renderer for the cuelume sounds MonoCode plays. The TypeScript
//! app used the cuelume package (MIT, Daniel Belyi), which synthesizes each
//! sound live with Web Audio. This ports its recipe format and the six
//! recipes the app's cues use, and renders them to mono PCM with the same
//! node graph: tone and filtered-noise layers with exponential envelopes,
//! a feedback delay "shimmer", an output gain of 4, and a limiter.
//!
//! The limiter is a plain feed-forward compressor with the Web Audio
//! defaults the engine set (threshold -8 dB, knee 6 dB, ratio 12, attack
//! 2 ms, release 80 ms) and Chromium's automatic makeup gain. It is close to,
//! not identical with, the browser's `DynamicsCompressorNode`.

use std::f64::consts::PI;

/// The render rate.
pub const SAMPLE_RATE: u32 = 48_000;

const SOURCE_STOP_PADDING: f64 = 0.05;
const CLEANUP_MARGIN: f64 = 0.05;
const INAUDIBLE_GAIN: f64 = 0.001;
const OUTPUT_GAIN: f64 = 4.0;
const ENVELOPE_FLOOR: f64 = 0.0001;

/// The cuelume sound names the app's cues map to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundName {
    Success,
    Bloom,
    Chime,
    Arrival,
    Toggle,
    Scan,
}

impl SoundName {
    pub const fn as_str(self) -> &'static str {
        match self {
            SoundName::Success => "success",
            SoundName::Bloom => "bloom",
            SoundName::Chime => "chime",
            SoundName::Arrival => "arrival",
            SoundName::Toggle => "toggle",
            SoundName::Scan => "scan",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FilterType {
    Lowpass,
    Bandpass,
}

#[derive(Debug, Clone, Copy)]
struct Tone {
    frequency: f64,
    detune: f64,
    glide_to: Option<f64>,
    glide_time: Option<f64>,
    offset: f64,
    attack: f64,
    decay: f64,
    peak: f64,
}

#[derive(Debug, Clone, Copy)]
struct Noise {
    filter_type: FilterType,
    filter_frequency: f64,
    filter_q: f64,
    offset: f64,
    attack: f64,
    decay: f64,
    peak: f64,
}

#[derive(Debug, Clone, Copy)]
enum Layer {
    Tone(Tone),
    Noise(Noise),
}

impl Layer {
    fn offset(&self) -> f64 {
        match self {
            Layer::Tone(tone) => tone.offset,
            Layer::Noise(noise) => noise.offset,
        }
    }

    fn length(&self) -> f64 {
        match self {
            Layer::Tone(tone) => tone.attack + tone.decay,
            Layer::Noise(noise) => noise.attack + noise.decay,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Shimmer {
    delay: f64,
    feedback: f64,
    wet: f64,
    lowpass: f64,
}

struct Recipe {
    master_gain: f64,
    layers: Vec<Layer>,
    shimmer: Option<Shimmer>,
}

/// A sine tone layer. The six recipes use no other waveform.
fn sine(frequency: f64, offset: f64, attack: f64, decay: f64, peak: f64) -> Layer {
    detuned(frequency, 0.0, offset, attack, decay, peak)
}

fn detuned(frequency: f64, detune: f64, offset: f64, attack: f64, decay: f64, peak: f64) -> Layer {
    Layer::Tone(Tone {
        frequency,
        detune,
        glide_to: None,
        glide_time: None,
        offset,
        attack,
        decay,
        peak,
    })
}

fn band(
    filter_frequency: f64,
    filter_q: f64,
    offset: f64,
    attack: f64,
    decay: f64,
    peak: f64,
) -> Layer {
    Layer::Noise(Noise {
        filter_type: FilterType::Bandpass,
        filter_frequency,
        filter_q,
        offset,
        attack,
        decay,
        peak,
    })
}

fn recipe(name: SoundName) -> Recipe {
    match name {
        // A soft two-note ascending bell.
        SoundName::Chime => Recipe {
            master_gain: 0.5,
            layers: vec![
                sine(1046.5, 0.0, 0.006, 0.22, 0.09),
                sine(1568.0, 0.09, 0.006, 0.26, 0.08),
            ],
            shimmer: Some(Shimmer {
                delay: 0.12,
                feedback: 0.25,
                wet: 0.18,
                lowpass: 4000.0,
            }),
        },
        // A warm, slow-swelling pad from two gently detuned sines.
        SoundName::Bloom => Recipe {
            master_gain: 0.5,
            layers: vec![
                sine(528.0, 0.0, 0.06, 0.32, 0.06),
                detuned(528.0, 12.0, 0.0, 0.06, 0.34, 0.05),
            ],
            shimmer: Some(Shimmer {
                delay: 0.15,
                feedback: 0.2,
                wet: 0.12,
                lowpass: 2500.0,
            }),
        },
        // A two-part click-clack, like a mechanical switch.
        SoundName::Toggle => Recipe {
            master_gain: 0.4,
            layers: vec![
                band(2200.0, 1.6, 0.0, 0.001, 0.016, 0.12),
                band(3800.0, 1.6, 0.024, 0.001, 0.02, 0.1),
            ],
            shimmer: None,
        },
        // A short, warm three-note ascending confirmation.
        SoundName::Success => Recipe {
            master_gain: 0.5,
            layers: vec![
                sine(880.0, 0.0, 0.004, 0.09, 0.06),
                sine(1108.73, 0.06, 0.004, 0.1, 0.06),
                sine(1318.51, 0.12, 0.004, 0.18, 0.07),
            ],
            shimmer: Some(Shimmer {
                delay: 0.1,
                feedback: 0.22,
                wet: 0.16,
                lowpass: 4500.0,
            }),
        },
        // A fast three-step locator signal.
        SoundName::Scan => Recipe {
            master_gain: 0.4,
            layers: vec![
                sine(740.0, 0.0, 0.002, 0.055, 0.05),
                sine(1110.0, 0.045, 0.002, 0.055, 0.045),
                sine(1665.0, 0.09, 0.002, 0.07, 0.04),
            ],
            shimmer: Some(Shimmer {
                delay: 0.065,
                feedback: 0.16,
                wet: 0.1,
                lowpass: 4200.0,
            }),
        },
        // A rising harmonic portal with a soft tail.
        SoundName::Arrival => Recipe {
            master_gain: 0.44,
            layers: vec![
                Layer::Noise(Noise {
                    filter_type: FilterType::Lowpass,
                    filter_frequency: 900.0,
                    filter_q: 0.8,
                    offset: 0.0,
                    attack: 0.05,
                    decay: 0.24,
                    peak: 0.035,
                }),
                Layer::Tone(Tone {
                    frequency: 220.0,
                    detune: 0.0,
                    glide_to: Some(440.0),
                    glide_time: Some(0.32),
                    offset: 0.0,
                    attack: 0.04,
                    decay: 0.34,
                    peak: 0.055,
                }),
                sine(659.25, 0.12, 0.045, 0.32, 0.04),
                sine(987.77, 0.19, 0.045, 0.34, 0.032),
            ],
            shimmer: Some(Shimmer {
                delay: 0.16,
                feedback: 0.28,
                wet: 0.18,
                lowpass: 3200.0,
            }),
        },
    }
}

/// `exponentialRampToValueAtTime` from `from` to `to` over `[start, end]`.
fn exp_ramp(from: f64, to: f64, start: f64, end: f64, t: f64) -> f64 {
    if end <= start {
        return to;
    }
    from * (to / from).powf(((t - start) / (end - start)).clamp(0.0, 1.0))
}

/// The gain envelope every layer uses: up to `peak` over `attack`, down to
/// the floor over `decay`, then held at the floor until the source stops.
fn envelope(t: f64, attack: f64, decay: f64, peak: f64) -> f64 {
    if t < attack {
        exp_ramp(ENVELOPE_FLOOR, peak, 0.0, attack, t)
    } else if t < attack + decay {
        exp_ramp(peak, ENVELOPE_FLOOR, attack, attack + decay, t)
    } else {
        ENVELOPE_FLOOR
    }
}

/// A Web Audio `BiquadFilterNode`, with the spec's coefficient formulas.
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(filter_type: FilterType, frequency: f64, q: f64) -> Self {
        let rate = SAMPLE_RATE as f64;
        let w0 = 2.0 * PI * (frequency / rate).clamp(0.0, 0.5);
        let cos = w0.cos();
        let (b0, b1, b2, a0, a1, a2) = match filter_type {
            FilterType::Lowpass => {
                // Lowpass Q is a resonance in dB.
                let alpha = w0.sin() / (2.0 * 10f64.powf(q / 20.0));
                (
                    (1.0 - cos) / 2.0,
                    1.0 - cos,
                    (1.0 - cos) / 2.0,
                    1.0 + alpha,
                    -2.0 * cos,
                    1.0 - alpha,
                )
            }
            FilterType::Bandpass => {
                let alpha = w0.sin() / (2.0 * q.max(1e-4));
                (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// A small xorshift generator, so a render is deterministic.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn render_tone(tone: &Tone, out: &mut [f64]) {
    let rate = SAMPLE_RATE as f64;
    let start = tone.offset;
    let stop = start + tone.attack + tone.decay + SOURCE_STOP_PADDING;
    let detune = 2f64.powf(tone.detune / 1200.0);
    let glide_time = tone.glide_time.unwrap_or(tone.attack + tone.decay);
    let mut phase = 0.0;
    let first = (start * rate).ceil() as usize;
    let last = ((stop * rate).ceil() as usize).min(out.len());
    for (index, sample) in out.iter_mut().enumerate().take(last).skip(first) {
        let t = index as f64 / rate - start;
        let frequency = match tone.glide_to {
            Some(to) => exp_ramp(tone.frequency, to, 0.0, glide_time, t),
            None => tone.frequency,
        } * detune;
        let wave = (2.0 * PI * phase).sin();
        *sample += wave * envelope(t, tone.attack, tone.decay, tone.peak);
        phase = (phase + frequency / rate).fract();
    }
}

fn render_noise(noise: &Noise, rng: &mut Rng, out: &mut [f64]) {
    let rate = SAMPLE_RATE as f64;
    let start = noise.offset;
    let duration = noise.attack + noise.decay + SOURCE_STOP_PADDING;
    let mut filter = Biquad::new(noise.filter_type, noise.filter_frequency, noise.filter_q);
    let first = (start * rate).ceil() as usize;
    let last = (((start + duration) * rate).ceil() as usize).min(out.len());
    for (index, sample) in out.iter_mut().enumerate().take(last).skip(first) {
        let t = index as f64 / rate - start;
        let filtered = filter.process(2.0 * rng.next() - 1.0);
        *sample += filtered * envelope(t, noise.attack, noise.decay, noise.peak);
    }
}

/// `shimmerTail`: how long the echo stays audible.
fn shimmer_tail(shimmer: Option<&Shimmer>) -> f64 {
    let Some(shimmer) = shimmer.filter(|shimmer| shimmer.feedback > 0.0) else {
        return 0.0;
    };
    if shimmer.feedback >= 1.0 {
        return shimmer.delay;
    }
    shimmer.delay * (1.0 + (INAUDIBLE_GAIN.ln() / shimmer.feedback.ln()).ceil())
}

fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The static curve: output level in dB for an input level in dB.
fn compress_db(input: f64, threshold: f64, knee: f64, ratio: f64) -> f64 {
    let over = input - threshold;
    if 2.0 * over < -knee {
        input
    } else if 2.0 * over.abs() <= knee {
        input + (1.0 / ratio - 1.0) * (over + knee / 2.0).powi(2) / (2.0 * knee)
    } else {
        threshold + over / ratio
    }
}

fn limit(samples: &mut [f64]) {
    const THRESHOLD: f64 = -8.0;
    const KNEE: f64 = 6.0;
    const RATIO: f64 = 12.0;
    let rate = SAMPLE_RATE as f64;
    let attack = (-1.0 / (0.002 * rate)).exp();
    let release = (-1.0 / (0.08 * rate)).exp();
    // Chromium's automatic makeup gain: the inverse of the curve's gain at
    // full scale, raised to 0.6.
    let makeup = (1.0 / db_to_gain(compress_db(0.0, THRESHOLD, KNEE, RATIO))).powf(0.6);
    let mut reduction_db = 0.0;
    for sample in samples {
        let level = 20.0 * sample.abs().max(1e-9).log10();
        let target = compress_db(level, THRESHOLD, KNEE, RATIO) - level;
        let coefficient = if target < reduction_db {
            attack
        } else {
            release
        };
        reduction_db = target + coefficient * (reduction_db - target);
        *sample = (*sample * db_to_gain(reduction_db) * makeup).clamp(-1.0, 1.0);
    }
}

/// Render `name` at `volume` (0 to 1) to mono samples at [`SAMPLE_RATE`].
pub fn render(name: SoundName, volume: f64) -> Vec<f32> {
    let recipe = recipe(name);
    let rate = SAMPLE_RATE as f64;
    let source_end = recipe
        .layers
        .iter()
        .map(|layer| layer.offset() + layer.length() + SOURCE_STOP_PADDING)
        .fold(0.0, f64::max);
    let total = source_end + shimmer_tail(recipe.shimmer.as_ref()) + CLEANUP_MARGIN;
    let length = (total * rate).ceil() as usize;

    let mut master = vec![0.0; length];
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for layer in &recipe.layers {
        match layer {
            Layer::Tone(tone) => render_tone(tone, &mut master),
            Layer::Noise(noise) => render_noise(noise, &mut rng, &mut master),
        }
    }
    let master_gain = recipe.master_gain * volume.clamp(0.0, 1.0);
    for sample in &mut master {
        *sample *= master_gain;
    }

    let mut output = master.clone();
    if let Some(shimmer) = recipe.shimmer {
        let delay = ((shimmer.delay * rate).round() as usize).max(1);
        let mut filter = Biquad::new(FilterType::Lowpass, shimmer.lowpass, 1.0);
        let mut line = vec![0.0; length];
        for index in 0..length {
            let delayed = if index >= delay {
                line[index - delay]
            } else {
                0.0
            };
            let filtered = filter.process(delayed);
            line[index] = master[index] + shimmer.feedback * filtered;
            output[index] += shimmer.wet * filtered;
        }
    }
    for sample in &mut output {
        *sample *= OUTPUT_GAIN;
    }
    limit(&mut output);
    output.into_iter().map(|sample| sample as f32).collect()
}

/// A 16-bit mono PCM WAV file for `samples`.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SoundName; 6] = [
        SoundName::Success,
        SoundName::Bloom,
        SoundName::Chime,
        SoundName::Arrival,
        SoundName::Toggle,
        SoundName::Scan,
    ];

    #[test]
    fn renders_every_cue_as_a_short_audible_clip() {
        for name in ALL {
            let samples = render(name, 0.55);
            let seconds = samples.len() as f64 / SAMPLE_RATE as f64;
            assert!(seconds > 0.05 && seconds < 2.5, "{name:?} lasts {seconds}s");
            let peak = samples
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            assert!(peak > 0.05 && peak <= 1.0, "{name:?} peaks at {peak}");
        }
    }

    #[test]
    fn renders_deterministically_and_silences_at_zero_volume() {
        assert_eq!(
            render(SoundName::Toggle, 0.55),
            render(SoundName::Toggle, 0.55)
        );
        assert!(
            render(SoundName::Chime, 0.0)
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn shimmer_tail_follows_the_feedback_decay() {
        let shimmer = Shimmer {
            delay: 0.1,
            feedback: 0.22,
            wet: 0.16,
            lowpass: 4500.0,
        };
        // ln(0.001) / ln(0.22) is 4.56, so five echoes after the first.
        assert!((shimmer_tail(Some(&shimmer)) - 0.6).abs() < 1e-9);
        assert_eq!(shimmer_tail(None), 0.0);
    }

    #[test]
    fn writes_a_wav_header() {
        let bytes = wav_bytes(&[0.0, 0.5, -0.5]);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + 6);
    }
}
