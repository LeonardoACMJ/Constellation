use crate::config::Sound;
use std::sync::mpsc::{SyncSender, sync_channel};
use windows_sys::Win32::Media::Audio::*;

pub struct Chimes {
    tx: SyncSender<Sound>,
}
impl Default for Chimes {
    fn default() -> Self {
        let (tx, rx) = sync_channel::<Sound>(2);
        std::thread::spawn(move || {
            let frequencies = [523.25, 587.33, 659.25, 783.99, 880.];
            let mut seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64;
            // One bounded voice. Keep the current buffer alive for asynchronous playback;
            // each new call stops the previous sound before its buffer is dropped.
            let mut current: Option<Vec<u8>> = None;
            while let Ok(sound) = rx.recv() {
                if !sound.enabled || sound.volume <= 0. {
                    continue;
                }
                let mut latest = sound;
                while let Ok(s) = rx.try_recv() {
                    latest = s;
                }
                if !latest.enabled || latest.volume <= 0. {
                    continue;
                }
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let wav = bell(frequencies[(seed % 5) as usize], latest.volume);
                unsafe {
                    PlaySoundW(
                        wav.as_ptr() as *const u16,
                        std::ptr::null_mut(),
                        SND_MEMORY | SND_NODEFAULT | SND_ASYNC,
                    );
                }
                current.replace(wav);
            }
            unsafe {
                PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
            }
            drop(current);
        });
        Self { tx }
    }
}
impl Chimes {
    pub fn play(&self, s: Sound) {
        let _ = self.tx.try_send(s);
    }
}
pub fn bell(frequency: f64, volume: f64) -> Vec<u8> {
    let rate = 22050u32;
    let count = (rate as f64 * 0.11) as usize;
    let mut out = vec![];
    out.extend(b"RIFF");
    out.extend((36 + count as u32 * 2).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((count as u32 * 2).to_le_bytes());
    for i in 0..count {
        let t = i as f64 / rate as f64;
        let fade = ((count - i) as f64 / (count as f64 * 0.15)).min(1.);
        let wave = (std::f64::consts::TAU * frequency * t).sin()
            + 0.25 * (std::f64::consts::TAU * frequency * 2.76 * t).sin() * (-35. * t).exp();
        let amplitude =
            wave * (-27. * t).exp() * (t / 0.003).min(1.) * fade * volume.clamp(0., 1.) * 20000.;
        out.extend((amplitude as i16).to_le_bytes());
    }
    out
}
