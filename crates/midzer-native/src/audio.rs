//! The cpal streams: the engine renders inside the output callback, and the
//! input callbacks feed it the microphone and the live source.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, SampleFormat, Stream, StreamConfig};
use crossbeam_queue::ArrayQueue;
use midzer_core::{Command, Engine};
use midzer_synth::Synth;

use crate::{COMMAND_QUEUE, Control, EVENT_QUEUE, MIC_QUEUE, Shared};

/// An input live audio can come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveDevice {
    /// cpal's device id, host included.
    pub id: String,
    pub name: String,
    /// It records what the computer plays (a monitor or loopback), not a microphone or line in.
    pub loopback: bool,
}

/// Frames the engine renders at a time, as in the AudioWorklet.
const BLOCK: usize = 128;
/// Mic samples allowed to pile up (input and output clocks drift) before the oldest are skipped.
const MIC_BACKLOG: usize = 2048;

/// Runs on its own thread: opens the output, then serves microphone requests
/// until told to stop. Streams close when dropped here.
pub(crate) fn own_streams(control: mpsc::Receiver<Control>, ready: mpsc::Sender<Result<Arc<Shared>, String>>) {
    let host = output_host();
    let opened = open_output(&host);
    let (shared, _output) = match opened {
        Ok(ok) => ok,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(shared.clone()));

    let mut mic: Option<Stream> = None;
    let mut live: Option<Stream> = None;
    while let Ok(msg) = control.recv() {
        match msg {
            Control::OpenMic(reply) => {
                if mic.is_none() {
                    match open_input(&host, &shared) {
                        Ok(stream) => mic = Some(stream),
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            continue;
                        }
                    }
                }
                shared.mic_open.store(true, Ordering::Release);
                let _ = reply.send(Ok(()));
            }
            Control::CloseMic => {
                shared.mic_open.store(false, Ordering::Release);
                mic = None;
                while shared.mic.pop().is_some() {}
            }
            Control::OpenLive(id, reply) => {
                shared.live_open.store(false, Ordering::Release);
                shared.live_generation.fetch_add(1, Ordering::AcqRel);
                // The old stream closes first: some devices take one stream at a time.
                drop(live.take());
                while shared.live.pop().is_some() {}
                match open_live(&id, &shared) {
                    Ok(stream) => {
                        live.replace(stream);
                        shared.live_open.store(true, Ordering::Release);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Control::CloseLive => {
                shared.live_open.store(false, Ordering::Release);
                shared.live_generation.fetch_add(1, Ordering::AcqRel);
                drop(live.take());
                while shared.live.pop().is_some() {}
            }
            Control::Stop => break,
        }
    }
}

/// An f32 config at the device's default rate, or the default config if it is f32 already.
fn f32_config(
    default: cpal::SupportedStreamConfig,
    supported: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
    rate: u32,
) -> Option<StreamConfig> {
    if default.sample_format() == SampleFormat::F32 && default.sample_rate() == rate {
        return Some(default.into());
    }
    supported
        .filter(|r| r.sample_format() == SampleFormat::F32)
        .find_map(|r| r.try_with_sample_rate(rate))
        .map(Into::into)
}

fn open_output(host: &cpal::Host) -> Result<(Arc<Shared>, Stream), String> {
    let device = host.default_output_device().ok_or("No audio output device was found.")?;
    let default = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = default.sample_rate();
    let supported = device.supported_output_configs().map_err(|e| e.to_string())?;
    let config = f32_config(default, supported, rate).ok_or("The audio output does not support 32-bit float samples.")?;
    let channels = config.channels as usize;

    let shared = Arc::new(Shared {
        commands: ArrayQueue::new(COMMAND_QUEUE),
        events: ArrayQueue::new(EVENT_QUEUE),
        inputs: ArrayQueue::new(1024),
        mic: ArrayQueue::new(MIC_QUEUE),
        mic_open: AtomicBool::new(false),
        live: ArrayQueue::new(MIC_QUEUE),
        live_open: AtomicBool::new(false),
        tap: ArrayQueue::new(crate::hear::TAP_BLOCKS),
        live_generation: AtomicU32::new(0),
        song_finish: AtomicBool::new(false),
        live_song: Mutex::new(None),
        synth_in: ArrayQueue::new(1),
        synth_out: ArrayQueue::new(4),
        track_in: ArrayQueue::new(1),
        track_out: ArrayQueue::new(4),
        sample_rate: rate,
        epoch: Instant::now(),
        played_frame: AtomicU64::new(0),
        played_at_ns: AtomicU64::new(0),
        latency_us: AtomicU32::new(0),
        running: AtomicBool::new(true),
    });

    let mut render = Renderer::new(shared.clone());
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                let ts = info.timestamp();
                let ahead = ts.playback.duration_since(ts.callback);
                render.run(data, channels, ahead);
            },
            |e| eprintln!("midzer: audio output: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((shared, stream))
}

/// ALSA on Linux even where cpal would pick PulseAudio: the output has only been tuned on ALSA.
fn output_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if let Ok(host) = cpal::host_from_id(cpal::HostId::Alsa) {
        return host;
    }
    cpal::default_host()
}

/// Where live audio comes from: PulseAudio (or PipeWire's stand-in) on Linux,
/// whose monitor sources record what the computer plays.
fn live_host() -> cpal::Host {
    #[cfg(target_os = "linux")]
    if let Ok(host) = cpal::host_from_id(cpal::HostId::PulseAudio) {
        return host;
    }
    cpal::default_host()
}

pub(crate) fn live_devices() -> Vec<LiveDevice> {
    let host = live_host();
    let describe = |device: &cpal::Device, loopback: bool| {
        let id = device.id().ok()?.to_string();
        let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| id.clone());
        // PulseAudio monitors record a sink.
        let loopback = loopback || id.ends_with(".monitor");
        Some(LiveDevice { id, name, loopback })
    };
    let mut out: Vec<LiveDevice> = Vec::new();
    // WASAPI records an output device in loopback when asked for input from it.
    if cfg!(target_os = "windows")
        && let Ok(outputs) = host.output_devices()
    {
        out.extend(outputs.filter_map(|d| describe(&d, true)));
    }
    if let Ok(inputs) = host.input_devices() {
        for device in inputs.filter_map(|d| describe(&d, false)) {
            if !out.iter().any(|o| o.id == device.id) {
                out.push(device);
            }
        }
    }
    // System audio first.
    out.sort_by_key(|d| !d.loopback);
    out
}

fn open_input(host: &cpal::Host, shared: &Arc<Shared>) -> Result<Stream, String> {
    let device = host.default_input_device().ok_or("No microphone was found.")?;
    // The engine analyses the mic at the output rate, so the input must match it.
    input_stream(&device, shared, "The microphone cannot record at the output's sample rate.", |s| (&s.mic, &s.mic_open))
}

fn open_live(id: &str, shared: &Arc<Shared>) -> Result<Stream, String> {
    let id: DeviceId = id.parse().map_err(|_| "That audio source is not known.".to_string())?;
    let host = cpal::host_from_id(id.host()).map_err(|e| e.to_string())?;
    let device = host.device_by_id(&id).ok_or("That audio source is gone.")?;
    input_stream(&device, shared, "That audio source cannot record at the output's sample rate.", |s| (&s.live, &s.live_open))
}

/// An f32 input stream at the output's rate, mixed to mono into the queue `pick` chooses while its flag is set.
fn input_stream(
    device: &cpal::Device,
    shared: &Arc<Shared>,
    wrong_rate: &str,
    pick: fn(&Shared) -> (&ArrayQueue<f32>, &AtomicBool),
) -> Result<Stream, String> {
    let default = device.default_input_config().or_else(|_| device.default_output_config()).map_err(|e| e.to_string())?;
    let supported: Vec<_> = match device.supported_input_configs() {
        Ok(configs) => configs.collect(),
        Err(_) => device.supported_output_configs().map_err(|e| e.to_string())?.collect(),
    };
    let config = f32_config(default, supported.into_iter(), shared.sample_rate).ok_or(wrong_rate)?;
    let channels = config.channels as usize;
    let shared = shared.clone();
    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let (queue, open) = pick(&shared);
                if !open.load(Ordering::Acquire) {
                    return;
                }
                for frame in data.chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / channels as f32;
                    // Full means the output side stalled; old input is worth less than new.
                    queue.force_push(mono);
                }
            },
            |e| eprintln!("midzer: audio input: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

/// Everything the output callback owns. Allocated once, before the stream starts.
struct Renderer {
    shared: Arc<Shared>,
    engine: Box<Engine>,
    synth: Box<Synth>,
    gain: f32,
    kit_sound: bool,
    left: [f32; BLOCK],
    right: [f32; BLOCK],
    input: [f32; BLOCK],
    live: [f32; BLOCK],
    frame: u64,
}

impl Renderer {
    fn new(shared: Arc<Shared>) -> Self {
        let rate = shared.sample_rate as f32;
        Self {
            engine: Box::new(Engine::new(rate)),
            synth: Box::new(Synth::new(rate)),
            gain: 0.6,
            kit_sound: false,
            left: [0.0; BLOCK],
            right: [0.0; BLOCK],
            input: [0.0; BLOCK],
            live: [0.0; BLOCK],
            frame: 0,
            shared,
        }
    }

    fn run(&mut self, data: &mut [f32], channels: usize, ahead: std::time::Duration) {
        let shared = &*self.shared;
        let now_ns = shared.epoch.elapsed().as_nanos() as u64;
        shared.played_frame.store(self.frame, Ordering::Release);
        shared.played_at_ns.store(now_ns + ahead.as_nanos() as u64, Ordering::Release);
        let frames = data.len() / channels.max(1);
        let buffer_us = frames as u64 * 1_000_000 / shared.sample_rate as u64;
        shared.latency_us.store((ahead.as_micros() as u64).max(buffer_us).max(1) as u32, Ordering::Relaxed);

        if let Some(mut synth) = shared.synth_in.pop() {
            synth.set_gain(self.gain);
            synth.set_kit_sound(self.kit_sound);
            std::mem::swap(&mut self.synth, &mut synth);
            // Dropping frees memory; the forwarder thread does that.
            let _ = shared.synth_out.push(synth);
        }
        if let Some(track) = shared.track_in.pop()
            && let Some(old) = self.engine.set_track(track)
        {
            // Freed by the forwarder; if it is behind, freeing here beats keeping both.
            let _ = shared.track_out.push(old);
        }
        while let Some(words) = shared.commands.pop() {
            match Command::decode(words) {
                Some(cmd @ Command::SetGain { gain }) => {
                    self.gain = gain;
                    self.synth.set_gain(gain);
                    self.engine.handle(cmd);
                }
                Some(Command::SetKitSound { on }) => {
                    self.kit_sound = on;
                    self.synth.set_kit_sound(on);
                }
                Some(cmd) => self.engine.handle(cmd),
                None => {}
            }
        }

        let listening = shared.mic_open.load(Ordering::Acquire);
        let following = shared.live_open.load(Ordering::Acquire);
        for chunk in data.chunks_mut(BLOCK * channels) {
            let n = chunk.len() / channels;
            if listening {
                let got = take_input(&shared.mic, &mut self.input[..n]);
                if got > 0 {
                    self.engine.listen(&self.input[..got]);
                }
            }
            if following {
                let got = take_input(&shared.live, &mut self.live[..n]);
                if got > 0 {
                    // The listener gets it too; if it is behind, it misses a block.
                    let _ = shared.tap.push(crate::hear::TapBlock::new(self.engine.now(), &self.live[..got]));
                    self.engine.listen_live(&self.live[..got]);
                }
            }
            let events = self.engine.process(n as u32);
            self.synth.render(events, &mut self.left[..n], &mut self.right[..n]);
            for timed in events {
                let frame = (self.frame + timed.frame as u64) as u32;
                // A full queue means the UI stalled; dropping beats blocking audio.
                let _ = shared.events.push(timed.event.encode(frame));
            }
            self.engine.mix_track(&mut self.left[..n], &mut self.right[..n]);
            self.engine.end_block();
            for (i, out) in chunk.chunks_mut(channels).enumerate() {
                out[0] = self.left[i];
                if channels > 1 {
                    out[1] = self.right[i];
                    out[2..].fill(0.0);
                }
            }
            self.frame += n as u64;
        }
    }
}

/// Fills `into` from an input queue as far as it goes, skipping the oldest
/// samples once they pile up; returns how many it got.
fn take_input(queue: &ArrayQueue<f32>, into: &mut [f32]) -> usize {
    while queue.len() > MIC_BACKLOG + into.len() {
        queue.pop();
    }
    let mut got = 0;
    while got < into.len() {
        let Some(x) = queue.pop() else { break };
        into[got] = x;
        got += 1;
    }
    got
}
