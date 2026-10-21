//! Real-time safety: after warm-up, rendering must never allocate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use midzer_core::{Chord, Command, IdleMode, Instrument, ListenMode, Piece, Quality, Request, Slot};
use midzer_wasm::{BLOCK_FRAMES, Host};

struct CountingAlloc;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn command(host: &mut Host, cmd: Command) {
    assert!(host.command(cmd.encode()));
}

#[test]
fn playing_band_renders_without_allocating() {
    let mut host = Host::new(48_000.0);
    let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/web/public/soundfonts/GeneralUser-GS.sf2");
    match std::fs::read(&font) {
        Ok(bytes) => assert!(host.load_soundfont(&bytes)),
        Err(_) => eprintln!("no SoundFont found; checking the engine without the synth"),
    }

    command(&mut host, Command::SetTempo { bpm: 140.0 });
    command(&mut host, Command::SetIdleMode { mode: IdleMode::Rest });
    command(&mut host, Command::Play);
    host.process(BLOCK_FRAMES); // warm-up; also builds the styles

    COUNTING.store(true, Ordering::Relaxed);
    let chords: [&[u8]; 4] = [&[48, 52, 55], &[45, 48, 52], &[41, 45, 48], &[43, 47, 50, 53]];
    let mut events = 0;
    for bar in 0..48 {
        let chord = chords[bar % chords.len()];
        // Every style, section and a silent stretch that rests the band.
        match bar % 8 {
            0 => command(&mut host, Command::SetStyle { index: (bar / 8 % 6) as u8 }),
            2 => command(&mut host, Command::Section { request: Request::Variation(2) }),
            4 => command(&mut host, Command::Section { request: Request::Fill }),
            5 => command(&mut host, Command::SetFollow { on: true }),
            _ => {}
        }
        let silent = bar % 8 == 6 || bar % 8 == 7;
        for &note in chord.iter().filter(|_| !silent) {
            command(&mut host, Command::NoteOn { note, velocity: 40 + (bar % 40) as u8 * 2 });
        }
        for _ in 0..(48_000 * 60 / 140 * 4 / BLOCK_FRAMES) {
            events += host.process(BLOCK_FRAMES);
        }
        for &note in chord {
            command(&mut host, Command::NoteOff { note });
        }
    }
    command(&mut host, Command::Section { request: Request::Ending });
    for _ in 0..(48_000 * 60 / 140 * 8 / BLOCK_FRAMES) {
        host.process(BLOCK_FRAMES);
    }
    command(&mut host, Command::Section { request: Request::Intro });
    host.process(BLOCK_FRAMES);
    command(&mut host, Command::Stop);
    host.process(BLOCK_FRAMES);

    // A looping song with marks, loaded and reloaded while playing.
    let load = |host: &mut Host| {
        command(host, Command::SongClear { bars: 4 });
        for (beat, root) in [(0, 0), (4, 9), (8, 5), (12, 7)] {
            let slot = Slot::Chord(Chord::new(root, Quality::Major));
            command(host, Command::SongChord { beat, slot });
        }
        command(host, Command::SongMark { bar: 0, mark: Some(Request::Intro) });
        command(host, Command::SongMark { bar: 2, mark: Some(Request::Variation(2)) });
        command(host, Command::SongLoop { start: 1, end: 4 });
    };
    load(&mut host);
    command(&mut host, Command::SetSong { on: true });
    command(&mut host, Command::Play);
    for block in 0..(48_000 * 60 / 140 * 4 * 10 / BLOCK_FRAMES) {
        if block == 2000 {
            load(&mut host);
            command(&mut host, Command::SongSeek { bar: 2 });
        }
        // Wait mode: hold on a beat and inside one, then go on.
        match block {
            3000 => command(&mut host, Command::SongWait { tick: Some(12 * 480) }),
            4000 => command(&mut host, Command::SongWait { tick: Some(13 * 480 + 240) }),
            5000 => command(&mut host, Command::SongWait { tick: None }),
            _ => {}
        }
        events += host.process(BLOCK_FRAMES);
    }
    command(&mut host, Command::Stop);
    host.process(BLOCK_FRAMES);

    // Listening to a microphone in both modes, with tempo follow.
    command(&mut host, Command::SetSong { on: false });
    command(&mut host, Command::SetTempoFollow { on: true });
    command(&mut host, Command::Play);
    for mode in [ListenMode::Notes, ListenMode::Chords, ListenMode::Off] {
        command(&mut host, Command::SetListen { mode, gate_db: -50.0 });
        for block in 0..2000usize {
            let hz = [220.0, 261.6, 329.6][block / 300 % 3];
            let strum = (block % 200) as f32 / 200.0;
            for (i, x) in host.input_mut().iter_mut().enumerate() {
                let t = (block * BLOCK_FRAMES + i) as f32 / 48_000.0;
                *x = (2.0 * std::f32::consts::PI * hz * t).sin() * 0.3 * (1.0 - strum);
            }
            host.listen(BLOCK_FRAMES);
            events += host.process(BLOCK_FRAMES);
        }
    }
    command(&mut host, Command::Stop);
    host.process(BLOCK_FRAMES);

    // A drummer: a remapped kit, sounding through the synth, with chokes and the pedal.
    command(&mut host, Command::SetListen { mode: ListenMode::Off, gate_db: -50.0 });
    command(&mut host, Command::SetInstrument { instrument: Instrument::Drums });
    command(&mut host, Command::SetKitSound { on: true });
    command(&mut host, Command::KitNote { note: 26, piece: Some(Piece::HiHatOpen) });
    command(&mut host, Command::Play);
    for block in 0..3000usize {
        match block % 24 {
            0 => command(&mut host, Command::NoteOn { note: 36, velocity: 110 }),
            6 => command(&mut host, Command::DrumNote { note: 26, velocity: 80 }),
            12 => command(&mut host, Command::DrumHit { piece: Piece::Snare, velocity: 100 }),
            18 => command(&mut host, Command::HiHatPedal { value: (block % 128) as u8 }),
            _ => {}
        }
        if block % 500 == 0 {
            command(&mut host, Command::DrumHit { piece: Piece::Crash, velocity: 127 });
        } else if block % 500 == 100 {
            command(&mut host, Command::DrumChoke { note: 49 });
        }
        events += host.process(BLOCK_FRAMES);
    }
    command(&mut host, Command::Stop);
    host.process(BLOCK_FRAMES);
    COUNTING.store(false, Ordering::Relaxed);

    assert!(events > 1000, "band produced only {events} events");
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0, "allocated on the audio path");
}
