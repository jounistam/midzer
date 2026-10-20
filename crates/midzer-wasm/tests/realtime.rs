//! Real-time safety: after warm-up, rendering must never allocate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use midzer_core::Command;
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
    command(&mut host, Command::Play);
    host.process(BLOCK_FRAMES); // warm-up

    COUNTING.store(true, Ordering::Relaxed);
    let chords: [&[u8]; 4] = [&[48, 52, 55], &[45, 48, 52], &[41, 45, 48], &[43, 47, 50, 53]];
    let mut events = 0;
    for bar in 0..32 {
        let chord = chords[bar % chords.len()];
        for &note in chord {
            command(&mut host, Command::NoteOn { note, velocity: 90 });
        }
        for _ in 0..(48_000 * 60 / 140 * 4 / BLOCK_FRAMES) {
            events += host.process(BLOCK_FRAMES);
        }
        for &note in chord {
            command(&mut host, Command::NoteOff { note });
        }
    }
    command(&mut host, Command::Stop);
    host.process(BLOCK_FRAMES);
    COUNTING.store(false, Ordering::Relaxed);

    assert!(events > 1000, "band produced only {events} events");
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0, "allocated on the audio path");
}
