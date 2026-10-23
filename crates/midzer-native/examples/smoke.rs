//! Runs the band on the default audio device for a few seconds and reports
//! what came back. `cargo run -p midzer-native --example smoke [volume]`;
//! the volume defaults to 0, so it is silent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use midzer_core::{Command, EngineEvent};
use midzer_native::{NativeEngine, Update};

fn main() -> Result<(), String> {
    let volume: f32 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let beats = Arc::new(Mutex::new(Vec::new()));
    let seen = beats.clone();
    let engine = NativeEngine::start(Box::new(move |update| match update {
        Update::Events { words, heard_frame } => {
            for record in words.chunks_exact(4) {
                if let Some((EngineEvent::Beat { bar, beat }, frame)) = EngineEvent::decode([record[0], record[1], record[2], record[3]]) {
                    seen.lock().unwrap().push((bar, beat, frame as f64 - heard_frame));
                }
            }
        }
        Update::Ports(ports) => println!("MIDI: {ports:?}"),
        Update::Input { .. } | Update::Chords { .. } => {}
    }))?;
    println!("rate {} Hz, output latency {:.1} ms", engine.sample_rate(), engine.output_latency_ms());

    let font = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/web/public/soundfonts/GeneralUser-GS.sf2");
    match std::fs::read(font) {
        Ok(bytes) => engine.load_soundfont(&bytes)?,
        Err(_) => println!("no SoundFont; run `pnpm fetch:sounds` to hear anything"),
    }
    engine.send(Command::SetGain { gain: volume }.encode());
    engine.send(Command::SetTempo { bpm: 120.0 }.encode());
    for note in [48, 52, 55] {
        engine.send(Command::NoteOn { note, velocity: 90 }.encode());
    }
    engine.send(Command::Play.encode());
    std::thread::sleep(Duration::from_secs(3));
    engine.send(Command::Stop.encode());
    std::thread::sleep(Duration::from_millis(200));

    let beats = beats.lock().unwrap();
    println!("{} beats heard:", beats.len());
    for (bar, beat, ahead) in beats.iter() {
        println!("  {bar}.{beat}  {:.1} ms ahead of the speakers when reported", ahead / engine.sample_rate() as f64 * 1000.0);
    }
    if beats.len() < 5 {
        return Err("expected about six beats in three seconds".into());
    }
    Ok(())
}
