//! Small portable bell mixer. Existing notes always finish; saturation drops
//! new requests rather than stealing a playing voice. No work while silent.
pub const RATE: u32 = 48000;
pub const VOICES: usize = 32;
const NOTES: [f64; 5] = [523.25, 587.33, 659.25, 783.99, 880.];
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
            notes: NOTES.map(bell),
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
fn bell(frequency: f64) -> Vec<f32> {
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
