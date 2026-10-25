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
impl Wake {
    fn new() -> Option<Self> {
        // CALLBACK_EVENT requires a manual-reset event. A single worker shares
        // it for driver completions and note commands.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            None
        } else {
            Some(Self(event))
        }
    }
    fn begin_cycle(&self) -> bool {
        // Clear BEFORE inspecting commands/headers, never between inspecting
        // work and waiting. Notifications arriving during service must remain
        // signaled so the next wait cannot miss them.
        unsafe { ResetEvent(self.0) != 0 }
    }
}
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
        {
            let Some(event) = Wake::new() else {
                return Self { channel: None };
            };
            let wake = Arc::new(event);
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
// WAVEHDR is packed in the Windows bindings. Keep every header (including
// later array entries) aligned for pointer-sized fields and volatile flags.
#[repr(C, align(8))]
struct Buffer {
    header: WAVEHDR,
    samples: [i16; FRAMES],
    queued: bool,
    prepared: bool,
}
struct Device {
    handle: HWAVEOUT,
    buffers: Box<[Buffer; BUFFERS]>,
    running: bool,
    // Multimedia scheduling is scoped to an open stream, not the idle tray.
    _scheduling: AudioScheduling,
}
struct AudioScheduling(HANDLE);
impl AudioScheduling {
    unsafe fn register() -> Self {
        let mut index = 0;
        Self(AvSetMmThreadCharacteristicsW(
            super::wide("Audio").as_ptr(),
            &mut index,
        ))
    }
}
impl Drop for AudioScheduling {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                AvRevertMmThreadCharacteristics(self.0);
            }
        }
    }
}

trait Stream {
    unsafe fn pause(&mut self) -> Result<(), ()>;
    unsafe fn submit(&mut self, buffer: &mut Buffer) -> Result<(), ()>;
    unsafe fn restart(&mut self) -> Result<(), ()>;
}
struct WaveStream(HWAVEOUT);
impl Stream for WaveStream {
    unsafe fn pause(&mut self) -> Result<(), ()> {
        if waveOutPause(self.0) == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    unsafe fn submit(&mut self, buffer: &mut Buffer) -> Result<(), ()> {
        if waveOutWrite(self.0, &mut buffer.header, size_of::<WAVEHDR>() as u32) == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
    unsafe fn restart(&mut self) -> Result<(), ()> {
        if waveOutRestart(self.0) == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}
impl Device {
    fn empty() -> Self {
        Self {
            handle: null_mut(),
            buffers: Box::new(std::array::from_fn(|_| Buffer {
                header: unsafe { zeroed() },
                samples: [0; FRAMES],
                queued: false,
                prepared: false,
            })),
            running: false,
            _scheduling: AudioScheduling(null_mut()),
        }
    }
    unsafe fn open(wake: &Wake) -> Option<Self> {
        let mut device = Self::empty();
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
        // waveOut otherwise starts immediately on the FIRST write. Hold it
        // paused until the complete initial queue is ready, including after
        // an idle gap. Never reset/interrupt the tails of playing notes.
        if waveOutPause(device.handle) != 0 {
            return None;
        }
        device._scheduling = AudioScheduling::register();
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
        self.fill_using(mixer, &mut WaveStream(self.handle))
    }
    unsafe fn fill_using(
        &mut self,
        mixer: &mut Mixer,
        stream: &mut impl Stream,
    ) -> Result<bool, ()> {
        // Prepared headers and sample addresses stay stable for the whole
        // stream. Reuse only completed buffers; render and submit in the same
        // order so the PCM timeline stays continuous when completions coalesce.
        for b in self.buffers.iter_mut() {
            // The driver owns these flags while the header is queued.
            if b.queued && std::ptr::read_volatile(&raw const b.header.dwFlags) & WHDR_DONE != 0 {
                b.queued = false;
            }
        }
        if self.running && !self.buffers.iter().any(|b| b.queued) {
            stream.pause()?;
            self.running = false;
        }
        for b in self.buffers.iter_mut() {
            if !b.queued && mixer.active() > 0 {
                mixer.mix(&mut b.samples);
                stream.submit(b)?;
                b.queued = true;
            }
        }
        let queued = self.buffers.iter().any(|b| b.queued);
        if queued && !self.running {
            stream.restart()?;
            self.running = true;
        }
        Ok(queued)
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
            if !wake.begin_cycle() {
                break;
            }
            let mut disconnected = false;
            loop {
                match rx.try_recv() {
                    Ok(s) => {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        if s.enabled {
                            mixer.set_instrument(s.instrument);
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

#[cfg(test)]
mod queue_tests {
    use super::*;
    // Kernel-event checks only: no audio device, windows, input or playback.
    #[test]
    fn callback_wakeup_is_manual_reset_and_cleared_before_work() {
        let wake = Wake::new().unwrap();
        unsafe {
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_TIMEOUT);
            SetEvent(wake.0);
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_OBJECT_0);
            assert_eq!(
                WaitForSingleObject(wake.0, 0),
                WAIT_OBJECT_0,
                "callback notification must remain set until explicit reset"
            );
            assert!(wake.begin_cycle());
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_TIMEOUT);
        }
    }

    #[test]
    fn commands_and_completions_arriving_during_service_keep_next_wait_awake() {
        let wake = Wake::new().unwrap();
        let (tx, rx) = sync_channel(4);
        let sound = Sound::default();
        unsafe {
            // An old completion wakes the cycle, then a new command arrives
            // after its reset; both the command and callback state are observed.
            SetEvent(wake.0);
            assert!(wake.begin_cycle());
            tx.try_send(sound).unwrap();
            SetEvent(wake.0);
            assert_eq!(rx.try_recv().unwrap(), sound);
            SetEvent(wake.0); // Driver completes during the same service pass.
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_OBJECT_0);
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_OBJECT_0);
            assert!(wake.begin_cycle());
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_TIMEOUT);
            // A request after the empty check still wakes the worker.
            assert!(rx.try_recv().is_err());
            tx.try_send(sound).unwrap();
            SetEvent(wake.0);
            assert_eq!(WaitForSingleObject(wake.0, 0), WAIT_OBJECT_0);
            assert_eq!(rx.try_recv().unwrap(), sound);
        }
    }

    #[derive(Default)]
    struct RecordingStream {
        operations: String,
        samples: Vec<i16>,
    }

    #[test]
    fn rapid_wind_chime_bursts_keep_existing_tails_during_queue_reuse() {
        use crate::config::Instrument;
        let mut device = Device::empty();
        let (mut mixer, mut reference) = (
            Mixer::new(Instrument::WindChimes),
            Mixer::new(Instrument::WindChimes),
        );
        let mut stream = RecordingStream::default();
        let mut checked = 0;
        for cycle in 0..100 {
            if [0, 1, 4, 5, 8, 12, 13, 30, 31, 40].contains(&cycle) {
                assert!(mixer.start(cycle % 5, 0.18));
                assert!(reference.start(cycle % 5, 0.18));
            }
            // Alternate blocked, partial and coalesced driver completions.
            // New gestures can arrive before any queued block has finished.
            for (index, buffer) in device.buffers.iter_mut().enumerate() {
                if cycle % 3 == 2 || (cycle % 3 == 1 && index < 2) {
                    buffer.header.dwFlags |= WHDR_DONE;
                }
            }
            unsafe {
                device.fill_using(&mut mixer, &mut stream).unwrap();
            }
            let mut expected = vec![0; stream.samples.len() - checked];
            reference.mix(&mut expected);
            assert_eq!(
                &stream.samples[checked..],
                expected.as_slice(),
                "cycle {cycle}"
            );
            checked = stream.samples.len();
        }
        assert_eq!(mixer.active(), 0);
        assert_eq!(stream.samples.last(), Some(&0));
    }
    impl Stream for RecordingStream {
        unsafe fn pause(&mut self) -> Result<(), ()> {
            self.operations.push('P');
            Ok(())
        }
        unsafe fn submit(&mut self, b: &mut Buffer) -> Result<(), ()> {
            self.operations.push('W');
            self.samples.extend_from_slice(&b.samples);
            b.header.dwFlags &= !WHDR_DONE;
            Ok(())
        }
        unsafe fn restart(&mut self) -> Result<(), ()> {
            self.operations.push('R');
            Ok(())
        }
    }

    // A null-handle fixture uses only the recording backend: no device is
    // opened, no scheduling priority is changed, and no sound is played.
    #[test]
    fn all_instrument_tails_drain_in_order_without_a_device() {
        use crate::config::Instrument;
        for instrument in [
            Instrument::Mallet,
            Instrument::ClassicBell,
            Instrument::PluckedStrings,
            Instrument::WindChimes,
        ] {
            let mut device = Device::empty();
            let (mut mixer, mut reference) = (Mixer::new(instrument), Mixer::new(instrument));
            mixer.start(0, 0.18);
            reference.start(0, 0.18);
            let mut stream = RecordingStream::default();
            for _ in 0..100 {
                if !unsafe { device.fill_using(&mut mixer, &mut stream).unwrap() } {
                    break;
                }
                for b in &mut *device.buffers {
                    b.header.dwFlags |= WHDR_DONE;
                }
            }
            assert_eq!(mixer.active(), 0);
            assert!(!device.running);
            let mut expected = vec![0; stream.samples.len()];
            reference.mix(&mut expected);
            assert_eq!(stream.samples, expected);
            assert_eq!(stream.samples.last(), Some(&0));
        }
    }
    #[test]
    fn startup_and_exhausted_queue_are_primed_before_playback() {
        let mut device = Device::empty();
        let (mut mixer, mut reference) = (Mixer::default(), Mixer::default());
        mixer.start(3, 0.18);
        reference.start(3, 0.18);
        let mut stream = RecordingStream::default();
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        assert_eq!(stream.operations, "WWWWR");
        for b in &mut *device.buffers {
            b.header.dwFlags |= WHDR_DONE;
        }
        stream.operations.clear();
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        assert_eq!(stream.operations, "PWWWWR");
        let mut expected = vec![0; 8 * FRAMES];
        reference.mix(&mut expected);
        assert_eq!(
            stream.samples, expected,
            "Repriming cannot skip or duplicate PCM"
        );
    }

    #[test]
    fn partial_completions_do_not_pause_or_touch_pending_buffers() {
        let mut device = Device::empty();
        let (mut mixer, mut reference) = (Mixer::default(), Mixer::default());
        mixer.start(1, 0.18);
        reference.start(1, 0.18);
        let mut stream = RecordingStream::default();
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        // Rotate the FIFO until 3/0 can finish together across the physical
        // array boundary while 1/2 still belong to the driver.
        device.buffers[0].header.dwFlags |= WHDR_DONE;
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        device.buffers[1].header.dwFlags |= WHDR_DONE;
        device.buffers[2].header.dwFlags |= WHDR_DONE;
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        let pending = [device.buffers[1].samples, device.buffers[2].samples];
        device.buffers[0].header.dwFlags |= WHDR_DONE;
        device.buffers[3].header.dwFlags |= WHDR_DONE;
        stream.operations.clear();
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        assert_eq!(stream.operations, "WW");
        assert_eq!(
            pending,
            [device.buffers[1].samples, device.buffers[2].samples]
        );
        let mut expected = vec![0; 9 * FRAMES];
        reference.mix(&mut expected);
        assert_eq!(stream.samples, expected);
    }

    #[test]
    fn natural_tail_finishes_then_next_burst_restarts_from_a_primed_queue() {
        let mut device = Device::empty();
        let mut mixer = Mixer::default();
        mixer.start(0, 0.18);
        let mut stream = RecordingStream::default();
        let mut finished = false;
        for _ in 0..10 {
            let queued = unsafe { device.fill_using(&mut mixer, &mut stream).unwrap() };
            if !queued {
                finished = true;
                break;
            }
            for b in &mut *device.buffers {
                b.header.dwFlags |= WHDR_DONE;
            }
        }
        assert!(finished && mixer.active() == 0 && !device.running);
        assert_eq!(stream.samples.last(), Some(&0));
        stream.operations.clear();
        unsafe {
            assert!(!device.fill_using(&mut mixer, &mut stream).unwrap());
        }
        assert!(
            stream.operations.is_empty(),
            "No driver activity while silent"
        );
        mixer.start(4, 0.18);
        unsafe {
            device.fill_using(&mut mixer, &mut stream).unwrap();
        }
        assert_eq!(stream.operations, "WWWWR");
    }
}
