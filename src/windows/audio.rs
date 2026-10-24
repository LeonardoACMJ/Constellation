#![allow(unsafe_op_in_unsafe_fn)]
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
const FRAMES: usize = 480; // 10 ms at the native 48 kHz stream rate.
const BUFFERS: usize = 4; // Scheduling headroom; the previous stream had only 23 ms.
struct Buffer {
    header: WAVEHDR,
    samples: [i16; FRAMES],
    queued: bool,
    prepared: bool,
}
struct Device {
    handle: HWAVEOUT,
    buffers: Box<[Buffer; BUFFERS]>,
}
impl Device {
    unsafe fn open(wake: &Wake) -> Option<Self> {
        let mut device = Self {
            handle: null_mut(),
            buffers: Box::new(std::array::from_fn(|_| Buffer {
                header: zeroed(),
                samples: [0; FRAMES],
                queued: false,
                prepared: false,
            })),
        };
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 1,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        if unsafe {
            waveOutOpen(
                &mut device.handle,
                WAVE_MAPPER,
                &format,
                wake.0 as usize,
                0,
                CALLBACK_EVENT,
            )
        } != 0
        {
            return None;
        }
        for b in device.buffers.iter_mut() {
            b.header.lpData = b.samples.as_mut_ptr().cast();
            b.header.dwBufferLength = (FRAMES * 2) as u32;
            if unsafe {
                waveOutPrepareHeader(device.handle, &mut b.header, size_of::<WAVEHDR>() as u32)
            } != 0
            {
                return None;
            }
            b.prepared = true;
        }
        Some(device)
    }
    unsafe fn fill(&mut self, mixer: &mut Mixer) -> Result<bool, ()> {
        // Prepared headers and sample addresses stay stable for the whole
        // stream. Reuse only completed buffers; render and submit in the same
        // order so the PCM timeline stays continuous when completions coalesce.
        for b in self.buffers.iter_mut() {
            if b.queued && b.header.dwFlags & WHDR_DONE != 0 {
                b.queued = false;
            }
            if !b.queued && mixer.active() > 0 {
                mixer.mix(&mut b.samples);
                if unsafe { waveOutWrite(self.handle, &mut b.header, size_of::<WAVEHDR>() as u32) }
                    != 0
                {
                    return Err(());
                }
                b.queued = true;
            }
        }
        Ok(self.buffers.iter().any(|b| b.queued))
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            if self.handle.is_null() {
                return;
            }
            waveOutReset(self.handle);
            for b in self.buffers.iter_mut().filter(|b| b.prepared) {
                waveOutUnprepareHeader(self.handle, &mut b.header, size_of::<WAVEHDR>() as u32);
            }
            waveOutClose(self.handle);
        }
    }
}
fn output(rx: Receiver<Sound>, wake: Arc<Wake>) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
        let mut mixer = Mixer::default();
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64
            | 1;
        let mut device: Option<Device> = None;
        let mut idle_deadline: Option<std::time::Instant> = None;
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
            if device.is_none() && mixer.active() > 0 {
                device = Device::open(&wake);
                if device.is_none() {
                    break;
                }
            }
            if let Some(d) = &mut device {
                match d.fill(&mut mixer) {
                    Ok(false) => {
                        idle_deadline.get_or_insert_with(|| {
                            std::time::Instant::now() + std::time::Duration::from_millis(750)
                        });
                    }
                    Ok(true) => idle_deadline = None,
                    Err(()) => break,
                }
            }
            let wait = if let Some(deadline) = idle_deadline {
                let now = std::time::Instant::now();
                if now >= deadline {
                    device = None;
                    idle_deadline = None;
                    continue;
                }
                (deadline - now).as_millis().max(1) as u32
            } else {
                INFINITE
            };
            // Events wake for commands/completions. After one short idle grace
            // period, release the device and wait indefinitely with no polling.
            WaitForSingleObject(wake.0, wait);
        }
    }
}
