//! Small portable mallet mixer. Existing notes always finish; saturation drops
//! new requests rather than stealing a playing voice. No work while silent.
use crate::config::Instrument;
use std::sync::Arc;

pub const RATE: u32 = 48000;
pub const VOICES: usize = 32;
const NOTES: [f64; 5] = [261.625, 293.665, 329.625, 391.995, 440.];
const DURATION: f64 = 0.32;
struct Voice {
    samples: Arc<[f32]>,
    at: usize,
    volume: f32,
}
pub struct Mixer {
    notes: [Arc<[f32]>; 5],
    instrument: Instrument,
    voices: Vec<Voice>,
}
impl Default for Mixer {
    fn default() -> Self {
        Self::new(Instrument::default())
    }
}
impl Mixer {
    fn bank(instrument: Instrument) -> [Arc<[f32]>; 5] {
        match instrument {
            Instrument::Mallet => NOTES.map(|f| Arc::from(mallet(f))),
            Instrument::ClassicBell => NOTES.map(|f| Arc::from(classic_bell(f * 2.))),
            Instrument::PluckedStrings => NOTES.map(|f| Arc::from(plucked_string(f))),
            Instrument::WindChimes => NOTES.map(|f| Arc::from(wind_chime(f * 2.))),
        }
    }
    pub fn new(instrument: Instrument) -> Self {
        Self {
            notes: Self::bank(instrument),
            instrument,
            voices: Vec::with_capacity(VOICES),
        }
    }
    /// Generate only the selected bank, off the UI thread. Already-playing
    /// voices retain their old note until its natural end, even across changes.
    pub fn set_instrument(&mut self, instrument: Instrument) {
        if instrument != self.instrument {
            self.notes = Self::bank(instrument);
            self.instrument = instrument;
        }
    }
    pub fn start(&mut self, note: usize, volume: f64) -> bool {
        if note >= self.notes.len()
            || !volume.is_finite()
            || volume <= 0.
            || self.voices.len() >= VOICES
        {
            return false;
        }
        self.voices.push(Voice {
            samples: self.notes[note].clone(),
            at: 0,
            volume: volume.min(1.) as f32,
        });
        true
    }
    pub fn active(&self) -> usize {
        self.voices.len()
    }
    pub fn mix(&mut self, samples: &mut [i16]) {
        for sample in samples {
            let mut sum = 0.;
            for v in &mut self.voices {
                if let Some(value) = v.samples.get(v.at) {
                    sum += value * v.volume;
                    v.at += 1;
                }
            }
            // Preserve normal-level samples exactly. Limit only near full scale;
            // the previous rational curve compressed/distorted every sample.
            let magnitude = sum.abs();
            let output = if magnitude <= 0.95 {
                sum
            } else {
                sum.signum() * (0.95 + 0.05 * (1. - (-(magnitude - 0.95) / 0.05).exp()))
            };
            *sample = (32767. * output) as i16;
        }
        self.voices.retain(|v| v.at < v.samples.len());
    }
}
/// Historical 0.1.21 bell, with its original register and 160 ms envelope.
fn classic_bell(frequency: f64) -> Vec<f32> {
    let count = (RATE as f64 * 0.16) as usize;
    (0..count)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let fade = ((count - i) as f64 / (count as f64 * 0.15)).min(1.);
            let wave = (std::f64::consts::TAU * frequency * t).sin()
                + 0.25 * (std::f64::consts::TAU * frequency * 2.76 * t).sin() * (-35. * t).exp();
            (wave * (-27. * t).exp() * (t / 0.003).min(1.) * fade * 0.35) as f32
        })
        .collect()
}
/// Independently tuned struck-bar model: damped sinusoidal modes. See
/// docs/AUDIO-SYNTHESIS.md for research and design. All synthesis is performed
/// once when creating the bank; the output mixer still only adds cached PCM.
fn mallet(frequency: f64) -> Vec<f32> {
    // Frequency ratio, relative amplitude, decay time, attack time (seconds).
    // Upper modes die away faster, leaving a softer fundamental in the tail.
    const MODES: [(f64, f64, f64, f64); 4] = [
        (1.0, 1.0, 0.075, 0.004),
        (4.02, 0.22, 0.026, 0.002),
        (9.96, 0.065, 0.012, 0.0015),
        (18.08, 0.018, 0.006, 0.001),
    ];
    modal_tone(frequency, &MODES, DURATION, 0.29, 0.025)
}

fn wind_chime(frequency: f64) -> Vec<f32> {
    // Free tube bending-mode ratios, with a quieter, long metallic tail.
    const MODES: [(f64, f64, f64, f64); 5] = [
        (1., 1., 0.18, 0.004),
        (2.76, 0.35, 0.11, 0.0025),
        (5.40, 0.14, 0.075, 0.002),
        (8.93, 0.06, 0.045, 0.0015),
        (13.34, 0.02, 0.025, 0.0015),
    ];
    modal_tone_recursive(frequency, &MODES, 0.72, 0.145, 0.060)
}

fn modal_tone(
    frequency: f64,
    modes: &[(f64, f64, f64, f64)],
    duration: f64,
    gain: f64,
    release: f64,
) -> Vec<f32> {
    let count = (RATE as f64 * duration) as usize;
    (0..count)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let mut sum = 0.;
            for &(ratio, amplitude, decay, attack) in modes {
                let partial = frequency * ratio;
                if partial >= RATE as f64 * 0.5 {
                    continue;
                }
                let rise = 0.5 - 0.5 * (std::f64::consts::PI * (t / attack).min(1.)).cos();
                sum += amplitude
                    * rise
                    * (-t / decay).exp()
                    * (std::f64::consts::TAU * partial * t).sin();
            }
            // Raised-cosine release reaches exact zero with a flat endpoint.
            let remaining = (count - 1 - i) as f64 / RATE as f64;
            let fade = 0.5 - 0.5 * (std::f64::consts::PI * (remaining / release).min(1.)).cos();
            (gain * sum * fade) as f32
        })
        .collect()
}

/// Same damped sine model as modal_tone, evaluated with complex rotation.
/// Trigonometry/exponentials occur once per mode, except at attack/release edges.
fn modal_tone_recursive(
    frequency: f64,
    modes: &[(f64, f64, f64, f64)],
    duration: f64,
    gain: f64,
    release: f64,
) -> Vec<f32> {
    struct Mode {
        real: f64,
        imaginary: f64,
        rotation_real: f64,
        rotation_imaginary: f64,
        amplitude: f64,
        attack: f64,
    }
    let mut states: Vec<_> = modes
        .iter()
        .filter_map(|&(ratio, amplitude, decay, attack)| {
            let partial = frequency * ratio;
            if partial >= RATE as f64 * 0.5 {
                return None;
            }
            let angle = std::f64::consts::TAU * partial / RATE as f64;
            let radius = (-1. / (RATE as f64 * decay)).exp();
            Some(Mode {
                real: 1.,
                imaginary: 0.,
                rotation_real: radius * angle.cos(),
                rotation_imaginary: radius * angle.sin(),
                amplitude,
                attack,
            })
        })
        .collect();
    let count = (RATE as f64 * duration) as usize;
    (0..count)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let mut sum = 0.;
            for mode in &mut states {
                let rise = if t >= mode.attack {
                    1.
                } else {
                    0.5 - 0.5 * (std::f64::consts::PI * t / mode.attack).cos()
                };
                sum += mode.amplitude * mode.imaginary * rise;
                let real =
                    mode.real * mode.rotation_real - mode.imaginary * mode.rotation_imaginary;
                mode.imaginary =
                    mode.imaginary * mode.rotation_real + mode.real * mode.rotation_imaginary;
                mode.real = real;
            }
            let remaining = (count - 1 - i) as f64 / RATE as f64;
            let fade = if remaining >= release {
                1.
            } else {
                0.5 - 0.5 * (std::f64::consts::PI * remaining / release).cos()
            };
            (gain * sum * fade) as f32
        })
        .collect()
}

/// Fractional-delay Karplus-Strong string, rendered once into the note bank.
/// The half-sample loss-filter delay is included in the pitch calculation.
fn plucked_string(frequency: f64) -> Vec<f32> {
    let delay = RATE as f64 / frequency - 0.5;
    let whole = delay.floor() as usize;
    let fraction = delay - whole as f64;
    let length = whole + 3;
    let mut seed = frequency.to_bits() ^ 0x9e3779b97f4a7c15;
    let mut line: Vec<f64> = (0..length)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 32) as f64 / u32::MAX as f64 * 2. - 1.
        })
        .collect();
    // A soft pluck: smooth its initial excitation, and remove DC.
    for _ in 0..3 {
        let source = line.clone();
        for i in 0..length {
            line[i] = 0.5 * (source[i] + source[(i + 1) % length]);
        }
    }
    let mean = line.iter().sum::<f64>() / length as f64;
    for value in &mut line {
        *value -= mean;
    }
    let loss = (-1. / (frequency * 0.11)).exp();
    let (mut write, mut previous, mut body) = (0usize, 0., 0.);
    let count = (RATE as f64 * 0.48) as usize;
    let mut samples: Vec<f32> = (0..count)
        .map(|i| {
            let a = (write + length - whole) % length;
            let b = (a + length - 1) % length;
            let value = line[a] * (1. - fraction) + line[b] * fraction;
            line[write] = 0.5 * (value + previous) * loss;
            previous = value;
            write = (write + 1) % length;
            body = 0.35 * value + 0.65 * body;
            let t = i as f64 / RATE as f64;
            let rise = 0.5 - 0.5 * (std::f64::consts::PI * (t / 0.003).min(1.)).cos();
            let remaining = (count - 1 - i) as f64 / RATE as f64;
            let fade = 0.5 - 0.5 * (std::f64::consts::PI * (remaining / 0.04).min(1.)).cos();
            (body * rise * fade) as f32
        })
        .collect();
    let peak = samples.iter().map(|v| v.abs()).fold(0f32, f32::max);
    if peak > 0. {
        for value in &mut samples {
            *value *= 0.28 / peak;
        }
    }
    samples
}

#[cfg(test)]
mod synthesis_tests {
    use super::*;
    #[test]
    fn recursive_modes_match_the_closed_form_below_pcm_resolution() {
        let modes = [
            (1., 1., 0.18, 0.004),
            (2.76, 0.35, 0.11, 0.0025),
            (5.4, 0.14, 0.075, 0.002),
        ];
        let a = modal_tone(523.25, &modes, 0.72, 0.145, 0.06);
        let b = modal_tone_recursive(523.25, &modes, 0.72, 0.145, 0.06);
        assert_eq!(a.len(), b.len());
        assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.000001));
    }
    #[test]
    fn new_banks_are_bounded_deterministic_quiet_at_edges_and_decay() {
        for (instrument, payload) in [
            (Instrument::PluckedStrings, 460800),
            (Instrument::WindChimes, 691200),
        ] {
            let a = Mixer::new(instrument);
            let b = Mixer::new(instrument);
            assert_eq!(
                a.notes
                    .iter()
                    .map(|n| n.len() * size_of::<f32>())
                    .sum::<usize>(),
                payload
            );
            for (a, b) in a.notes.iter().zip(&b.notes) {
                assert_eq!(a, b);
                assert_eq!(a[0], 0.);
                assert_eq!(*a.last().unwrap(), 0.);
                assert!(a.iter().all(|v| v.is_finite() && v.abs() < 0.4));
                let energy =
                    |s: &[f32]| s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64;
                assert!(energy(&a[480..2400]) > energy(&a[a.len() - 2400..a.len() - 480]) * 5.);
                assert!(a[a.len() - 16..].iter().all(|v| v.abs() < 0.00001));
            }
        }
    }
    #[test]
    fn fractional_string_delay_preserves_the_requested_pitch() {
        for (frequency, note) in NOTES
            .into_iter()
            .zip(Mixer::new(Instrument::PluckedStrings).notes)
        {
            let period = RATE as f64 / frequency;
            let correlation = |lag: usize| {
                let (mut xy, mut xx, mut yy) = (0., 0., 0.);
                for i in 5760..10560 {
                    let (x, y) = (note[i] as f64, note[i + lag] as f64);
                    xy += x * y;
                    xx += x * x;
                    yy += y * y;
                }
                xy / (xx * yy).sqrt()
            };
            let best = ((period * 0.8) as usize..=(period * 1.2) as usize)
                .max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)))
                .unwrap();
            let actual = RATE as f64 / best as f64;
            assert!(
                (actual / frequency - 1.).abs() < 0.01,
                "{actual} vs {frequency}"
            );
        }
    }
    #[test]
    fn bank_changes_reuse_current_selection_and_release_old_data_after_tails() {
        let mut mixer = Mixer::default();
        let old = Arc::downgrade(&mixer.notes[0]);
        mixer.set_instrument(Instrument::Mallet);
        assert!(Arc::ptr_eq(&old.upgrade().unwrap(), &mixer.notes[0]));
        mixer.start(0, 0.2);
        mixer.set_instrument(Instrument::ClassicBell);
        assert!(old.upgrade().is_some());
        assert_eq!(
            mixer
                .notes
                .iter()
                .map(|n| n.len() * size_of::<f32>())
                .sum::<usize>(),
            153600
        );
        mixer.mix(&mut vec![0; RATE as usize / 2]);
        assert!(old.upgrade().is_none());
    }
    #[test]
    fn bank_is_bounded_finite_and_releases_without_discontinuities() {
        let mixer = Mixer::default();
        assert_eq!(
            mixer
                .notes
                .iter()
                .map(|n| n.len() * size_of::<f32>())
                .sum::<usize>(),
            307200
        );
        for note in &mixer.notes {
            assert_eq!(note.len(), (RATE as f64 * DURATION) as usize);
            assert_eq!(note[0], 0.);
            assert_eq!(*note.last().unwrap(), 0.);
            assert!(note.iter().all(|s| s.is_finite() && s.abs() < 0.45));
            // Check the boundaries rather than penalizing valid fast oscillations
            // in the struck-bar attack as though they were discontinuities.
            assert!(note[..16].iter().all(|s| s.abs() < 0.015));
            assert!(note[note.len() - 16..].iter().all(|s| s.abs() < 0.00001));
            let rms = |slice: &[f32]| {
                (slice.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / slice.len() as f64).sqrt()
            };
            assert!(rms(&note[480..2400]) > rms(&note[9600..12000]) * 8.);
        }
    }
    #[test]
    fn fundamental_remains_tuned_to_the_pentatonic_bank() {
        for (frequency, note) in NOTES.into_iter().zip(Mixer::default().notes) {
            // Once the short upper modes decay, the remaining waveform must
            // cross upwards at the requested fundamental, within 1%.
            let crossings: Vec<_> = (4801..9600)
                .filter(|i| note[i - 1] <= 0. && note[*i] > 0.)
                .collect();
            let period = (crossings.last().unwrap() - crossings.first().unwrap()) as f64
                / (crossings.len() - 1) as f64;
            let actual = RATE as f64 / period;
            assert!((actual / frequency - 1.).abs() < 0.01);
        }
    }
}
