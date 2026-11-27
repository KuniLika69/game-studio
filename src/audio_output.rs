//! Windows waveform output. The mixer and project format do not depend on this worker.
use std::{
    ffi::{c_char, c_void},
    mem::size_of,
    ptr::{self, null_mut},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::Duration,
};

use crate::audio::{AmbientLevels, Clip, Listener, Mixer, OUTPUT_RATE, VoiceSettings};

const FRAMES: usize = OUTPUT_RATE as usize / 100;
const SLOTS: usize = 3;
const POLL: Duration = Duration::from_millis(4);
const HEADER_BYTES: u32 = size_of::<WaveHeader>() as u32;
const STILL_PLAYING: u32 = 33;

struct Playback {
    generation: u64,
    dialogue: Option<u64>,
    clip: Arc<Clip>,
    settings: VoiceSettings,
}

#[derive(Default)]
struct State {
    generation: AtomicU64,
    dialogue_generation: AtomicU64,
    listener: Mutex<Listener>,
    ambience: Mutex<AmbientLevels>,
}

/// Opened on the first Play request. Headless/offline paths can omit this type entirely.
pub struct Output {
    playback: SyncSender<Playback>,
    state: Arc<State>,
    errors: Receiver<String>,
}

impl Output {
    pub fn spawn() -> Result<Self, String> {
        let (playback, requests) = mpsc::sync_channel(32);
        let (error_sender, errors) = mpsc::sync_channel(1);
        let state = Arc::new(State::default());
        let worker_state = state.clone();
        thread::Builder::new()
            .name("robot-studio-audio".into())
            .spawn(move || {
                if let Err(error) = run(&requests, &worker_state) {
                    let _ = error_sender.try_send(error);
                }
            })
            .map_err(|error| format!("Не удалось запустить звук ({error})."))?;
        Ok(Self {
            playback,
            state,
            errors,
        })
    }

    pub fn listener(&self, listener: Listener) {
        if let Ok(mut current) = self.state.listener.lock() {
            *current = listener;
        }
    }

    pub fn ambience(&self, levels: AmbientLevels) {
        if let Ok(mut current) = self.state.ambience.lock() {
            *current = levels;
        }
    }

    pub fn play(&self, clip: Arc<Clip>, settings: VoiceSettings) -> bool {
        self.submit(clip, settings, None)
    }

    pub fn play_dialogue(&self, clip: Arc<Clip>, settings: VoiceSettings) -> bool {
        let dialogue = self.state.dialogue_generation.load(Ordering::Acquire);
        self.submit(clip, settings, Some(dialogue))
    }

    pub fn stop_dialogue(&self) {
        self.state.dialogue_generation.fetch_add(1, Ordering::AcqRel);
    }

    fn submit(&self, clip: Arc<Clip>, settings: VoiceSettings, dialogue: Option<u64>) -> bool {
        self.playback
            .try_send(Playback {
                generation: self.state.generation.load(Ordering::Acquire),
                dialogue,
                clip,
                settings,
            })
            .is_ok()
    }

    pub fn clear(&self) {
        self.state.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn take_error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }
}

fn run(requests: &Receiver<Playback>, state: &State) -> Result<(), String> {
    let mut mixer = Mixer::new(OUTPUT_RATE)?;
    let mut device: Option<Device> = None;
    let mut generation = state.generation.load(Ordering::Acquire);
    let mut dialogue_generation = state.dialogue_generation.load(Ordering::Acquire);
    let mut buffer = [[0.0; 2]; FRAMES];
    loop {
        let playing = mixer.is_active() || device.as_ref().is_some_and(Device::pending);
        let poll = if playing { POLL } else { Duration::from_millis(24) };
        let request = match requests.recv_timeout(poll) {
            Ok(request) => Some(request),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        let next_generation = state.generation.load(Ordering::Acquire);
        if next_generation != generation {
            generation = next_generation;
            mixer.clear();
            if let Some(device) = &mut device {
                device.reset()?;
            }
        }
        let next_dialogue = state.dialogue_generation.load(Ordering::Acquire);
        if next_dialogue != dialogue_generation {
            dialogue_generation = next_dialogue;
            mixer.stop_dialogue();
        }
        for request in request.into_iter().chain(requests.try_iter().take(32)) {
            if request.generation == generation {
                match request.dialogue {
                    Some(id) if id == dialogue_generation => {
                        mixer.play_dialogue(request.clip, request.settings);
                    }
                    None => {
                        mixer.play(request.clip, request.settings);
                    }
                    _ => {}
                }
            }
        }
        if let Ok(level) = state.ambience.lock() {
            mixer.ambience(*level);
        }
        if mixer.is_active() && device.is_none() {
            device = Some(Device::open()?);
        }
        let Some(device) = &mut device else { continue };
        let listener = state
            .listener
            .lock()
            .map(|value| *value)
            .unwrap_or_default();
        for slot in &mut device.slots {
            slot.reclaim(device.handle)?;
            if !slot.prepared && mixer.is_active() {
                mixer.render(&mut buffer, listener);
                slot.submit(device.handle, &buffer)?;
            }
        }
    }
}

#[repr(C, packed(1))]
struct WaveFormat {
    format_tag: u16,
    channels: u16,
    samples_per_sec: u32,
    avg_bytes_per_sec: u32,
    block_align: u16,
    bits_per_sample: u16,
    extra_size: u16,
}

#[repr(C)]
#[derive(Default)]
struct WaveHeader {
    data: *mut c_char,
    len: u32,
    bytes_recorded: u32,
    user: usize,
    flags: u32,
    loops: u32,
    next: *mut WaveHeader,
    reserved: usize,
}

const _: () = assert!(size_of::<WaveFormat>() == 18);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<WaveHeader>() == 48);

#[link(name = "winmm")]
unsafe extern "system" {
    fn waveOutOpen(
        handle: *mut *mut c_void,
        device_id: u32,
        format: *const WaveFormat,
        callback: usize,
        instance: usize,
        flags: u32,
    ) -> u32;
    fn waveOutPrepareHeader(handle: *mut c_void, header: *mut WaveHeader, bytes: u32) -> u32;
    fn waveOutWrite(handle: *mut c_void, header: *mut WaveHeader, bytes: u32) -> u32;
    fn waveOutUnprepareHeader(handle: *mut c_void, header: *mut WaveHeader, bytes: u32) -> u32;
    fn waveOutReset(handle: *mut c_void) -> u32;
    fn waveOutClose(handle: *mut c_void) -> u32;
}

struct Device {
    handle: *mut c_void,
    slots: [Slot; SLOTS],
}

impl Device {
    fn open() -> Result<Self, String> {
        let mut handle = null_mut();
        let format = WaveFormat {
            format_tag: 1,
            channels: 2,
            samples_per_sec: OUTPUT_RATE,
            avg_bytes_per_sec: 4 * OUTPUT_RATE,
            block_align: 4,
            bits_per_sample: 16,
            extra_size: 0,
        };
        // The format is a valid WAVEFORMATEX. WinMM copies it before returning.
        check("открытие", unsafe {
            waveOutOpen(&mut handle, u32::MAX, &format, 0, 0, 0)
        })?;
        Ok(Self {
            handle,
            slots: std::array::from_fn(|_| Slot::default()),
        })
    }

    fn pending(&self) -> bool {
        self.slots.iter().any(|slot| slot.prepared)
    }

    fn reset(&mut self) -> Result<(), String> {
        // The handle stays uniquely owned by this worker.
        check("сброс", unsafe { waveOutReset(self.handle) })?;
        for slot in &mut self.slots {
            slot.reclaim(self.handle)?;
        }
        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.reset();
        for slot in &mut self.slots {
            let _ = slot.reclaim(self.handle);
            // A driver that refuses to unprepare may still hold the allocation.
            if slot.prepared
                && let Some(memory) = slot.memory.take()
            {
                std::mem::forget(memory);
            }
        }
        // The handle was opened by this worker. Still-playing errors remain harmless at exit.
        let _ = unsafe { waveOutClose(self.handle) };
    }
}

struct SlotMemory {
    samples: [[i16; 2]; FRAMES],
    header: WaveHeader,
}

struct Slot {
    // WinMM writes WaveHeader while a prepared slot belongs to the driver.
    memory: Option<Box<std::cell::UnsafeCell<SlotMemory>>>,
    prepared: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            memory: Some(Box::new(std::cell::UnsafeCell::new(SlotMemory {
                samples: [[0; 2]; FRAMES],
                header: WaveHeader::default(),
            }))),
            prepared: false,
        }
    }
}

impl Slot {
    fn reclaim(&mut self, handle: *mut c_void) -> Result<(), String> {
        if !self.prepared {
            return Ok(());
        }
        let memory = self.memory.as_ref().expect("allocated waveform slot").get();
        // The header has a fixed allocation. Unprepare reports STILLPLAYING while
        // the driver still owns its sample buffer, so Rust does not inspect either.
        let result = unsafe {
            waveOutUnprepareHeader(handle, ptr::addr_of_mut!((*memory).header), HEADER_BYTES)
        };
        if result != STILL_PLAYING {
            check("освобождение буфера", result)?;
            self.prepared = false;
        }
        Ok(())
    }

    fn submit(&mut self, handle: *mut c_void, buffer: &[[f32; 2]; FRAMES]) -> Result<(), String> {
        debug_assert!(!self.prepared);
        let memory = self.memory.as_ref().expect("allocated waveform slot").get();
        // Successful unprepare (or the first submission) gives exclusive access to this slot.
        let memory = unsafe { &mut *memory };
        for (dst, frame) in memory.samples.iter_mut().zip(buffer) {
            *dst = frame.map(|x| (x.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16);
        }
        memory.header = WaveHeader {
            data: memory.samples.as_mut_ptr().cast(),
            len: size_of::<[[i16; 2]; FRAMES]>() as u32,
            ..WaveHeader::default()
        };
        let header = ptr::addr_of_mut!(memory.header);
        // WinMM owns the header and buffer from successful prepare through unprepare.
        check("подготовка буфера", unsafe {
            waveOutPrepareHeader(handle, header, HEADER_BYTES)
        })?;
        self.prepared = true;
        check("вывод звука", unsafe {
            waveOutWrite(handle, header, HEADER_BYTES)
        })
    }
}

fn check(operation: &str, result: u32) -> Result<(), String> {
    if result == 0 {
        Ok(())
    } else {
        Err(format!("Устройство звука: {operation}, WinMM {result}."))
    }
}

#[cfg(test)]
mod tests {
    use super::Output;

    #[test]
    fn idle_worker_can_exit_without_requesting_hardware() {
        let output = Output::spawn().unwrap();
        output.clear();
        assert!(output.take_error().is_none());
        drop(output);
    }
}
