//! Small portable mallet mixer. Existing notes always finish; saturation drops
//! new requests rather than stealing a playing voice. No work while silent.
pub const RATE: u32 = 48000;
pub const VOICES: usize = 32;
const NOTES: [f64; 5] = [261.625, 293.665, 329.625, 391.995, 440.];
const DURATION: f64 = 0.32;
struct Voice {
    note: usize,
    at: usize,
    volume: f32,
}
pub struct Mixer {
    notes: [Vec<f32>; 5],
    voices: Vec<Voice>,
}
impl Default for Mixer {
    fn default() -> Self {
        Self {
            notes: NOTES.map(mallet),
            voices: Vec::with_capacity(VOICES),
        }
    }
}
impl Mixer {
    pub fn start(&mut self, note: usize, volume: f64) -> bool {
        if note >= self.notes.len()
            || !volume.is_finite()
            || volume <= 0.
            || self.voices.len() >= VOICES
        {
            return false;
        }
        self.voices.push(Voice {
            note,
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
                if let Some(value) = self.notes[v.note].get(v.at) {
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
        self.voices.retain(|v| v.at < self.notes[v.note].len());
    }
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
    let count = (RATE as f64 * DURATION) as usize;
    (0..count)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let mut sum = 0.;
            for (ratio, amplitude, decay, attack) in MODES {
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
            let fade = 0.5 - 0.5 * (std::f64::consts::PI * (remaining / 0.025).min(1.)).cos();
            (0.29 * sum * fade) as f32
        })
        .collect()
}

#[cfg(test)]
mod synthesis_tests {
    use super::*;
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
