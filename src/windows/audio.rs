//! Event-driven PCM output. Only the audio worker touches driver buffers.
use crate::{
    config::Sound,
    sound::{Mixer, RATE},
};
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{
        Arc,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};
use windows_sys::Win32::{Foundation::*, Media::Audio::*, System::Threading::*};
struct Wake(HANDLE);
// The event handle is thread-safe, and is closed only after both owners drop.
unsafe impl Send for Wake {}
unsafe impl Sync for Wake {}
impl Drop for Wake {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Chimes {
    channel: Option<(SyncSender<Sound>, Arc<Wake>)>,
}
impl Default for Chimes {
    fn default() -> Self {
        unsafe {
            let event = CreateEventW(null(), 0, 0, null());
            if event.is_null() {
                return Self { channel: None };
            }
            let wake = Arc::new(Wake(event));
            let worker_wake = wake.clone();
            let (tx, rx) = sync_channel(64);
            if std::thread::Builder::new()
                .name("constellation-audio".into())
                .stack_size(512 * 1024)
                .spawn(move || output(rx, worker_wake))
                .is_err()
            {
                return Self { channel: None };
            }
            Self {
                channel: Some((tx, wake)),
            }
        }
    }
}
impl Chimes {
    pub fn play(&self, s: Sound) {
        if !s.enabled || s.volume <= 0. {
            return;
        }
        if let Some((tx, wake)) = &self.channel {
            if tx.try_send(s).is_ok() {
                unsafe {
                    SetEvent(wake.0);
                }
            }
        }
    }
}
impl Drop for Chimes {
    fn drop(&mut self) {
        if let Some((tx, wake)) = self.channel.take() {
            drop(tx);
            unsafe {
                SetEvent(wake.0);
            }
        }
    }
}
struct Buffer {
    header: WAVEHDR,
    samples: [i16; 256],
    queued: bool,
}
fn output(rx: Receiver<Sound>, wake: Arc<Wake>) {
    unsafe {
        let mut mixer = Mixer::default();
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64
            | 1;
        // Heap addresses stay stable from PrepareHeader until UnprepareHeader.
        let mut buffers: Box<[Buffer; 2]> = Box::new(std::array::from_fn(|_| Buffer {
            header: zeroed(),
            samples: [0; 256],
            queued: false,
        }));
        let mut device: HWAVEOUT = null_mut();
        loop {
            let mut disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(s) => {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        if s.enabled {
                            mixer.start((seed % 5) as usize, s.volume);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
            if disconnected {
                break;
            }
            if device.is_null() && mixer.active() > 0 {
                let format = WAVEFORMATEX {
                    wFormatTag: 1,
                    nChannels: 1,
                    nSamplesPerSec: RATE,
                    nAvgBytesPerSec: RATE * 2,
                    nBlockAlign: 2,
                    wBitsPerSample: 16,
                    cbSize: 0,
                };
                if waveOutOpen(
                    &mut device,
                    WAVE_MAPPER,
                    &format,
                    wake.0 as usize,
                    0,
                    CALLBACK_EVENT,
                ) != 0
                {
                    break;
                }
            }
            if !device.is_null() {
                let mut failed = false;
                for b in buffers.iter_mut() {
                    if b.queued && b.header.dwFlags & WHDR_DONE != 0 {
                        if waveOutUnprepareHeader(
                            device,
                            &mut b.header,
                            size_of::<WAVEHDR>() as u32,
                        ) != 0
                        {
                            failed = true;
                            break;
                        }
                        b.queued = false;
                    }
                    if !b.queued && mixer.active() > 0 {
                        mixer.mix(&mut b.samples);
                        b.header = zeroed();
                        b.header.lpData = b.samples.as_mut_ptr().cast();
                        b.header.dwBufferLength = (b.samples.len() * 2) as u32;
                        if waveOutPrepareHeader(device, &mut b.header, size_of::<WAVEHDR>() as u32)
                            != 0
                        {
                            failed = true;
                            break;
                        }
                        if waveOutWrite(device, &mut b.header, size_of::<WAVEHDR>() as u32) != 0 {
                            waveOutUnprepareHeader(
                                device,
                                &mut b.header,
                                size_of::<WAVEHDR>() as u32,
                            );
                            failed = true;
                            break;
                        }
                        b.queued = true;
                    }
                }
                if failed {
                    break;
                }
                if mixer.active() == 0 && buffers.iter().all(|b| !b.queued) {
                    waveOutClose(device);
                    device = null_mut();
                }
            }
            // Commands and driver completion share an auto-reset event. No timer,
            // polling or silence stream runs while the launcher waits in the tray.
            WaitForSingleObject(wake.0, INFINITE);
        }
        if !device.is_null() {
            waveOutReset(device);
            for b in buffers.iter_mut() {
                waveOutUnprepareHeader(device, &mut b.header, size_of::<WAVEHDR>() as u32);
            }
            waveOutClose(device);
        }
    }
}
