//! Offline A/B renders only: writes WAV files, never opens an audio device.
use constellation::sound::{Mixer, RATE};
use std::{fs, io::Write, path::Path};
const VOLUME: f64 = 0.28;
fn wav(path: &Path, samples: &[i16]) {
    let mut f = fs::File::create(path).unwrap();
    let length = samples.len() as u32 * 2;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(length + 36).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&RATE.to_le_bytes()).unwrap();
    f.write_all(&(RATE * 2).to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&length.to_le_bytes()).unwrap();
    for sample in samples {
        f.write_all(&sample.to_le_bytes()).unwrap();
    }
    let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
    println!(
        "{}: {:.2}s, peak {:.3}",
        path.display(),
        samples.len() as f64 / RATE as f64,
        peak as f64 / 32767.
    );
}
fn main() {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".local/audio-review".into());
    let output = Path::new(&output);
    fs::create_dir_all(output).unwrap();
    // Same pitches by scale degree, spacing and volume; register differs as in app.
    let events = [
        (0.1, 0),
        (0.5, 1),
        (0.9, 2),
        (1.3, 3),
        (1.7, 4),
        (2.3, 0),
        (2.35, 2),
        (2.4, 4),
        (2.45, 3),
        (2.5, 1),
        (2.55, 2),
        (2.6, 0),
    ];
    let count = (RATE as f64 * 3.2) as usize;
    let mut current = vec![0i16; count];
    let mut mixer = Mixer::default();
    let mut at = 0;
    for (time, note) in events {
        let next = (time * RATE as f64) as usize;
        mixer.mix(&mut current[at..next]);
        mixer.start(note, VOLUME);
        at = next;
    }
    mixer.mix(&mut current[at..]);
    wav(&output.join("02-mallet-novo.wav"), &current);
    let mut old = vec![0f64; count];
    // Historical 0.1.21 formula, reproduced only for the listening comparison.
    let frequencies = [523.25, 587.33, 659.25, 783.99, 880.];
    let length = (RATE as f64 * 0.16) as usize;
    for (time, note) in events {
        let start = (time * RATE as f64) as usize;
        for i in 0..length {
            let t = i as f64 / RATE as f64;
            let f = frequencies[note];
            let wave = (std::f64::consts::TAU * f * t).sin()
                + 0.25 * (std::f64::consts::TAU * f * 2.76 * t).sin() * (-35. * t).exp();
            old[start + i] += wave
                * (-27. * t).exp()
                * (t / 0.003).min(1.)
                * ((length - i) as f64 / (length as f64 * 0.15)).min(1.)
                * 0.35
                * VOLUME;
        }
    }
    // Preview levels stay below the existing mixer's limiter knee.
    assert!(old.iter().all(|s| s.abs() <= 0.95));
    let old: Vec<i16> = old.into_iter().map(|s| (s * 32767.) as i16).collect();
    wav(&output.join("01-sino-anterior.wav"), &old);
}
