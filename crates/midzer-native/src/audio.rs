//! The cpal streams: the engine renders inside the output callback, and the
//! input callback feeds the microphone to it.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use crossbeam_queue::ArrayQueue;
use midzer_core::{Command, Engine};
use midzer_synth::Synth;

use crate::{COMMAND_QUEUE, Control, EVENT_QUEUE, MIC_QUEUE, Shared};

/// Frames the engine renders at a time, as in the AudioWorklet.
const BLOCK: usize = 128;
/// Mic samples allowed to pile up (input and output clocks drift) before the oldest are skipped.
const MIC_BACKLOG: usize = 2048;

/// Runs on its own thread: opens the output, then serves microphone requests
/// until told to stop. Streams close when dropped here.
pub(crate) fn own_streams(control: mpsc::Receiver<Control>, ready: mpsc::Sender<Result<Arc<Shared>, String>>) {
    let host = cpal::default_host();
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
        synth_in: ArrayQueue::new(1),
        synth_out: ArrayQueue::new(4),
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

fn open_input(host: &cpal::Host, shared: &Arc<Shared>) -> Result<Stream, String> {
    let device = host.default_input_device().ok_or("No microphone was found.")?;
    let default = device.default_input_config().map_err(|e| e.to_string())?;
    let supported = device.supported_input_configs().map_err(|e| e.to_string())?;
    // The engine analyses the mic at the output rate, so the input must match it.
    let config = f32_config(default, supported, shared.sample_rate)
        .ok_or("The microphone cannot record at the output's sample rate.")?;
    let channels = config.channels as usize;
    let shared = shared.clone();
    let stream = device
        .build_input_stream(
            config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !shared.mic_open.load(Ordering::Acquire) {
                    return;
                }
                for frame in data.chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / channels as f32;
                    // Full means the output side stalled; old input is worth less than new.
                    shared.mic.force_push(mono);
                }
            },
            |e| eprintln!("midzer: microphone: {e}"),
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
    left: [f32; BLOCK],
    right: [f32; BLOCK],
    input: [f32; BLOCK],
    frame: u64,
}

impl Renderer {
    fn new(shared: Arc<Shared>) -> Self {
        let rate = shared.sample_rate as f32;
        Self {
            engine: Box::new(Engine::new(rate)),
            synth: Box::new(Synth::new(rate)),
            gain: 0.6,
            left: [0.0; BLOCK],
            right: [0.0; BLOCK],
            input: [0.0; BLOCK],
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
            std::mem::swap(&mut self.synth, &mut synth);
            // Dropping frees memory; the forwarder thread does that.
            let _ = shared.synth_out.push(synth);
        }
        while let Some(words) = shared.commands.pop() {
            match Command::decode(words) {
                Some(Command::SetGain { gain }) => {
                    self.gain = gain;
                    self.synth.set_gain(gain);
                }
                Some(cmd) => self.engine.handle(cmd),
                None => {}
            }
        }

        let listening = shared.mic_open.load(Ordering::Acquire);
        for chunk in data.chunks_mut(BLOCK * channels) {
            let n = chunk.len() / channels;
            if listening {
                while shared.mic.len() > MIC_BACKLOG + n {
                    shared.mic.pop();
                }
                let mut got = 0;
                while got < n {
                    let Some(x) = shared.mic.pop() else { break };
                    self.input[got] = x;
                    got += 1;
                }
                if got > 0 {
                    self.engine.listen(&self.input[..got]);
                }
            }
            let events = self.engine.process(n as u32);
            self.synth.render(events, &mut self.left[..n], &mut self.right[..n]);
            for timed in events {
                let frame = (self.frame + timed.frame as u64) as u32;
                // A full queue means the UI stalled; dropping beats blocking audio.
                let _ = shared.events.push(timed.event.encode(frame));
            }
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
