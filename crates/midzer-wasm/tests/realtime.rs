//! Real-time safety: after warm-up, rendering must never allocate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use midzer_core::{BandDrums, Chord, Command, IdleMode, Instrument, KitAction, ListenMode, Piece, Quality, Request, Slot, Track, Trigger};
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
    // A 20 s recording with a drum stem, for the backing track at the end.
    let samples = |v: i16| (0..48_000 * 20 * 2).map(|i| (i % 2000) as i16 - 1000 + v).collect::<Vec<i16>>();
    host.set_track(Some(Track { main: samples(0), drums: Some(samples(7)) }));

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

    // Drummer mode: a count-in, then a groove with fills and crashes over
    // each drum layer, kit triggers and a choke that stops the band.
    command(&mut host, Command::SetCountIn { pieces: Piece::HiHatClosed.bit() });
    command(&mut host, Command::KitTrigger { trigger: Trigger::Note(60), action: Some(KitAction::NextSection) });
    command(&mut host, Command::KitTrigger { trigger: Trigger::Control(64), action: Some(KitAction::Fill) });
    for layer in [BandDrums::Off, BandDrums::Percussion, BandDrums::Click] {
        command(&mut host, Command::SetBandDrums { mode: layer });
        // 120 BPM: 187.5 blocks a beat, so a hit every 94 blocks is an eighth.
        for block in 0..400usize {
            if block % 188 == 0 {
                command(&mut host, Command::DrumHit { piece: Piece::HiHatClosed, velocity: 80 });
            }
            events += host.process(BLOCK_FRAMES);
        }
        for block in 0..6000usize {
            let eighth = block / 94;
            if block % 94 == 0 {
                let in_bar = eighth % 8;
                let fill = eighth / 8 % 4 == 3 && in_bar >= 4;
                let piece = match in_bar {
                    _ if fill => Piece::Tom2,
                    0 if eighth % 32 == 0 => Piece::Crash,
                    0 | 4 => Piece::Kick,
                    2 | 6 => Piece::Snare,
                    _ => Piece::HiHatClosed,
                };
                command(&mut host, Command::DrumHit { piece, velocity: 100 });
            }
            match block {
                1000 => command(&mut host, Command::DrumNote { note: 60, velocity: 100 }),
                2000 => command(&mut host, Command::ControlChange { cc: 64, value: 127 }),
                2010 => command(&mut host, Command::ControlChange { cc: 64, value: 0 }),
                _ => {}
            }
            events += host.process(BLOCK_FRAMES);
        }
        command(&mut host, Command::DrumChoke { note: 49 });
        host.process(BLOCK_FRAMES);
    }

    // The song again, played by the recording: a 112 BPM tempo map, a loop,
    // a seek, the drum stem muted and a drummer judged against it all.
    command(&mut host, Command::TrackClear { beats: 33 });
    for i in 0..33 {
        command(&mut host, Command::TrackBeat { index: i, seconds: 0.4 + i as f32 * 60.0 / 112.0 });
    }
    command(&mut host, Command::SetSong { on: true });
    command(&mut host, Command::SetTrack { on: true });
    command(&mut host, Command::SongLoop { start: 1, end: 3 });
    command(&mut host, Command::Play);
    for block in 0..6000usize {
        if block % 94 == 0 {
            command(&mut host, Command::DrumHit { piece: [Piece::Kick, Piece::HiHatClosed, Piece::Snare][block / 94 % 3], velocity: 90 });
        }
        match block {
            1500 => command(&mut host, Command::SongSeek { bar: 0 }),
            2500 => command(&mut host, Command::SetTrackDrums { on: false }),
            3000 => command(&mut host, Command::SongLoop { start: 0, end: 0 }),
            _ => {}
        }
        events += host.process(BLOCK_FRAMES);
    }
    command(&mut host, Command::SetTrack { on: false });
    host.process(BLOCK_FRAMES);

    // Live audio: a 120 BPM pulse the tracker locks onto, told half and
    // double, its 1 moved, hits judged a beat later, started over, then silence.
    command(&mut host, Command::SetLiveOffset { ms: 25.0 });
    command(&mut host, Command::SetLive { on: true });
    for block in 0..7000usize {
        for (i, x) in host.live_mut().iter_mut().enumerate() {
            let f = block * BLOCK_FRAMES + i;
            let into = f % 24_000;
            *x = if block < 5500 && into < 400 { ((f * 7919) % 200) as f32 / 400.0 - 0.25 } else { 0.0 };
        }
        if block % 94 == 0 {
            command(&mut host, Command::DrumHit { piece: [Piece::Kick, Piece::HiHatClosed, Piece::Snare][block / 94 % 3], velocity: 90 });
        }
        match block {
            3000 => command(&mut host, Command::LiveTempo { double: true }),
            3200 => command(&mut host, Command::LiveTempo { double: false }),
            3500 => command(&mut host, Command::LiveDownbeat),
            4000 => command(&mut host, Command::LiveReset),
            _ => {}
        }
        host.listen_live(BLOCK_FRAMES);
        events += host.process(BLOCK_FRAMES);
    }
    command(&mut host, Command::SetLive { on: false });
    host.process(BLOCK_FRAMES);
    COUNTING.store(false, Ordering::Relaxed);

    assert!(events > 1000, "band produced only {events} events");
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0, "allocated on the audio path");
}
