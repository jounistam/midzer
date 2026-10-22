//! Native host for the midzer engine: audio through cpal, MIDI through midir,
//! the microphone and live audio to play along with, for the desktop app (and
//! later mobile).
//!
//! The engine runs inside the cpal output callback, exactly as it runs in the
//! AudioWorklet on the web: commands come in through lock-free queues, events
//! go out through another, and nothing on the audio thread allocates or locks.
//! A forwarder thread hands events to the UI, plays MIDI out at the time each
//! event is heard, and watches for MIDI devices coming and going.

mod audio;
mod midi;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;
use midzer_core::{Command, Track, WORDS_PER_EVENT};
use midzer_synth::Synth;

pub use audio::LiveDevice;
pub use midi::{Port, Ports};

/// Engine events waiting for the forwarder; about a second of a busy band.
const EVENT_QUEUE: usize = 16_384;
const COMMAND_QUEUE: usize = 8_192;
/// Microphone (and live source) samples in flight between the input and output callbacks.
const MIC_QUEUE: usize = 16_384;
const FORWARD_EVERY: Duration = Duration::from_millis(2);
const PORT_SCAN_EVERY: Duration = Duration::from_secs(1);

/// What the host tells the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Engine events, `WORDS_PER_EVENT` words each with the absolute engine
    /// frame (low 32 bits) last. `heard_frame` is the frame reaching the
    /// speakers as this is sent, so the UI can tell when each event is heard.
    Events { words: Vec<u32>, heard_frame: f64 },
    /// Commands from MIDI input, already applied, for the UI to show and judge.
    Input { words: Vec<u32> },
    /// MIDI devices changed.
    Ports(Ports),
}

pub type UpdateFn = Box<dyn FnMut(Update) + Send>;

/// State shared between the audio callback, the forwarder and the API.
struct Shared {
    commands: ArrayQueue<[u32; 4]>,
    events: ArrayQueue<[u32; WORDS_PER_EVENT]>,
    /// MIDI input commands to report to the UI.
    inputs: ArrayQueue<[u32; 4]>,
    mic: ArrayQueue<f32>,
    mic_open: AtomicBool,
    /// The live source to play along with, like the microphone.
    live: ArrayQueue<f32>,
    live_open: AtomicBool,
    /// A freshly loaded synth for the audio thread, and old ones to drop elsewhere.
    synth_in: ArrayQueue<Box<Synth>>,
    synth_out: ArrayQueue<Box<Synth>>,
    /// The same for backing tracks; `None` unloads it.
    track_in: ArrayQueue<Option<Box<Track>>>,
    track_out: ArrayQueue<Box<Track>>,
    sample_rate: u32,
    epoch: Instant,
    /// The first frame of the latest callback and when it plays, in ns since `epoch`.
    played_frame: AtomicU64,
    played_at_ns: AtomicU64,
    /// Output latency in µs, measured by the audio callback.
    latency_us: AtomicU32,
    running: AtomicBool,
}

impl Shared {
    /// The engine frame reaching the speakers right now.
    fn heard_frame(&self) -> f64 {
        let frame = self.played_frame.load(Ordering::Acquire) as f64;
        let at = self.played_at_ns.load(Ordering::Acquire) as f64;
        let now = self.epoch.elapsed().as_nanos() as f64;
        frame + (now - at) / 1e9 * self.sample_rate as f64
    }

    /// When `frame` reaches the speakers.
    fn instant_of(&self, frame: u64) -> Instant {
        let ahead = (frame as f64 - self.heard_frame()) / self.sample_rate as f64;
        let now = Instant::now();
        if ahead > 0.0 { now + Duration::from_secs_f64(ahead) } else { now }
    }
}

enum Control {
    OpenMic(mpsc::Sender<Result<(), String>>),
    CloseMic,
    /// A device id from [`NativeEngine::live_devices`].
    OpenLive(String, mpsc::Sender<Result<(), String>>),
    CloseLive,
    Stop,
}

pub struct NativeEngine {
    shared: Arc<Shared>,
    control: mpsc::Sender<Control>,
    midi: Arc<Mutex<midi::Midi>>,
    threads: Vec<JoinHandle<()>>,
}

impl NativeEngine {
    /// Opens the default output device and starts the engine. `on_update`
    /// runs on the forwarder thread.
    pub fn start(on_update: UpdateFn) -> Result<Self, String> {
        let (control, control_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        // cpal streams are not `Send` on every platform, so one thread owns them.
        let audio_thread = std::thread::Builder::new()
            .name("midzer-audio-owner".into())
            .spawn(move || audio::own_streams(control_rx, ready_tx))
            .map_err(|e| e.to_string())?;
        let shared = ready_rx.recv().map_err(|_| "the audio thread stopped".to_string())??;

        let midi = Arc::new(Mutex::new(midi::Midi::new(shared.clone())));
        let forwarder = {
            let (shared, midi) = (shared.clone(), midi.clone());
            std::thread::Builder::new()
                .name("midzer-forwarder".into())
                .spawn(move || forward(shared, midi, on_update))
                .map_err(|e| e.to_string())?
        };
        // Give the first callback a moment to measure the latency.
        let deadline = Instant::now() + Duration::from_millis(500);
        while shared.latency_us.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(Self { shared, control, midi, threads: vec![audio_thread, forwarder] })
    }

    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate
    }

    pub fn output_latency_ms(&self) -> f32 {
        self.shared.latency_us.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// Queues an encoded command; false if it is invalid or the queue is full.
    pub fn send(&self, words: [u32; 4]) -> bool {
        Command::decode(words).is_some() && self.shared.commands.push(words).is_ok()
    }

    /// Parses an SF2 file on this thread and hands it to the audio thread.
    pub fn load_soundfont(&self, bytes: &[u8]) -> Result<(), String> {
        let mut synth = Box::new(Synth::new(self.shared.sample_rate as f32));
        synth.load_soundfont(bytes)?;
        self.shared.synth_in.force_push(synth);
        Ok(())
    }

    /// Hands a recording (decoded at [`NativeEngine::sample_rate`]) to the
    /// audio thread for songs to play, or unloads it with `None`.
    pub fn load_track(&self, track: Option<Track>) {
        self.shared.track_in.force_push(track.map(Box::new));
    }

    /// Opens the default input device as the microphone.
    pub fn open_mic(&self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.control.send(Control::OpenMic(tx)).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn close_mic(&self) {
        let _ = self.control.send(Control::CloseMic);
    }

    /// Inputs live audio can come from: system audio (loopback) where the
    /// platform offers it, and the other inputs.
    pub fn live_devices(&self) -> Vec<LiveDevice> {
        audio::live_devices()
    }

    /// Plays along with the device `id` from [`NativeEngine::live_devices`]:
    /// its audio goes to the engine's beat tracker.
    pub fn open_live(&self, id: &str) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.control.send(Control::OpenLive(id.to_string(), tx)).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    pub fn close_live(&self) {
        let _ = self.control.send(Control::CloseLive);
    }

    pub fn midi_ports(&self) -> Ports {
        self.midi.lock().map(|m| m.ports()).unwrap_or_default()
    }

    /// Sends the band to a MIDI output, or to none.
    pub fn set_midi_output(&self, id: Option<&str>) -> Result<(), String> {
        self.midi.lock().map_err(|e| e.to_string())?.set_output(id)
    }
}

impl Drop for NativeEngine {
    fn drop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        let _ = self.control.send(Control::Stop);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Moves events to the UI and MIDI out, drops replaced synths and rescans
/// MIDI devices, until the engine stops.
fn forward(shared: Arc<Shared>, midi: Arc<Mutex<midi::Midi>>, mut on_update: UpdateFn) {
    let mut last_scan = Instant::now() - PORT_SCAN_EVERY;
    let mut words = Vec::new();
    let mut inputs = Vec::new();
    while shared.running.load(Ordering::Acquire) {
        if last_scan.elapsed() >= PORT_SCAN_EVERY {
            last_scan = Instant::now();
            if let Some(ports) = midi.lock().ok().and_then(|mut m| m.rescan()) {
                on_update(Update::Ports(ports));
            }
        }
        while let Some(event) = shared.events.pop() {
            words.extend_from_slice(&event);
        }
        while let Some(input) = shared.inputs.pop() {
            inputs.extend_from_slice(&input);
        }
        if let Ok(mut m) = midi.lock() {
            m.schedule(&words);
            m.flush();
        }
        if !words.is_empty() {
            let heard_frame = shared.heard_frame();
            on_update(Update::Events { words: std::mem::take(&mut words), heard_frame });
        }
        if !inputs.is_empty() {
            on_update(Update::Input { words: std::mem::take(&mut inputs) });
        }
        while shared.synth_out.pop().is_some() {}
        while shared.track_out.pop().is_some() {}
        std::thread::sleep(FORWARD_EVERY);
    }
}
