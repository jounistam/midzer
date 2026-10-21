//! The desktop app: the web UI in a Tauri window, with the engine, MIDI and
//! the microphone on native threads (`midzer-native`). The UI talks to it
//! through the commands below and one update channel; see
//! `packages/audio-web/src/native.ts` for the other side.

use std::sync::Mutex;

use midzer_native::{NativeEngine, Ports, Update};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeBody, Request};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, State};

#[derive(Default)]
struct AppEngine(Mutex<Option<NativeEngine>>);

#[derive(Serialize, Clone)]
struct Port {
    id: String,
    name: String,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum UiUpdate {
    Events { words: Vec<u32>, heard_frame: f64 },
    Input { words: Vec<u32> },
    Ports { inputs: Vec<Port>, outputs: Vec<Port> },
}

impl From<Ports> for UiUpdate {
    fn from(ports: Ports) -> Self {
        let list = |ports: Vec<midzer_native::Port>| ports.into_iter().map(|p| Port { id: p.id, name: p.name }).collect();
        UiUpdate::Ports { inputs: list(ports.inputs), outputs: list(ports.outputs) }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    sample_rate: u32,
    output_latency_ms: f32,
}

fn with_engine<T>(state: &State<AppEngine>, f: impl FnOnce(&NativeEngine) -> Result<T, String>) -> Result<T, String> {
    let engine = state.0.lock().map_err(|e| e.to_string())?;
    f(engine.as_ref().ok_or("The engine has not started.")?)
}

/// Starts (or, after a reload, restarts) the engine; updates stream to `on_update`.
#[tauri::command]
fn engine_start(state: State<AppEngine>, on_update: Channel<UiUpdate>) -> Result<Started, String> {
    let mut slot = state.0.lock().map_err(|e| e.to_string())?;
    *slot = None;
    let engine = NativeEngine::start(Box::new(move |update| {
        let update = match update {
            Update::Events { words, heard_frame } => UiUpdate::Events { words, heard_frame },
            Update::Input { words } => UiUpdate::Input { words },
            Update::Ports(ports) => ports.into(),
        };
        let _ = on_update.send(update);
    }))?;
    let started = Started { sample_rate: engine.sample_rate(), output_latency_ms: engine.output_latency_ms() };
    *slot = Some(engine);
    Ok(started)
}

/// Queues encoded commands, four words each. Returns how many were accepted.
#[tauri::command]
fn engine_send(state: State<AppEngine>, words: Vec<u32>) -> Result<usize, String> {
    with_engine(&state, |engine| {
        Ok(words.as_chunks::<4>().0.iter().filter(|w| engine.send([w[0], w[1], w[2], w[3]])).count())
    })
}

/// Loads an SF2 file sent as the raw request body.
#[tauri::command]
fn engine_load_soundfont(state: State<AppEngine>, request: Request) -> Result<(), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected the SoundFont as raw bytes.".into());
    };
    with_engine(&state, |engine| engine.load_soundfont(bytes))
}

#[tauri::command]
fn mic_open(state: State<AppEngine>) -> Result<(), String> {
    with_engine(&state, |engine| engine.open_mic())
}

#[tauri::command]
fn mic_close(state: State<AppEngine>) -> Result<(), String> {
    with_engine(&state, |engine| {
        engine.close_mic();
        Ok(())
    })
}

#[tauri::command]
fn midi_ports(state: State<AppEngine>) -> Result<UiUpdate, String> {
    with_engine(&state, |engine| Ok(engine.midi_ports().into()))
}

#[tauri::command]
fn midi_set_output(state: State<AppEngine>, id: Option<String>) -> Result<(), String> {
    with_engine(&state, |engine| engine.set_midi_output(id.as_deref()))
}

/// Ends a `MIDZER_SELFTEST` run: prints what the page saw and exits.
#[tauri::command]
fn selftest_done(app: AppHandle, ok: bool, log: String) {
    println!("{log}\nselftest: {}", if ok { "passed" } else { "FAILED" });
    app.exit(if ok { 0 } else { 1 });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let selftest = std::env::var_os("MIDZER_SELFTEST").is_some();
    if selftest {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(60));
            eprintln!("selftest: timed out");
            std::process::exit(2);
        });
    }
    tauri::Builder::default()
        .manage(AppEngine::default())
        .on_page_load(move |webview, payload| {
            if selftest && payload.event() == PageLoadEvent::Finished {
                eprintln!("selftest: page loaded: {}", payload.url());
                if let Err(e) = webview.eval(include_str!("selftest.js")) {
                    eprintln!("selftest: could not run: {e}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            engine_start,
            engine_send,
            engine_load_soundfont,
            mic_open,
            mic_close,
            midi_ports,
            midi_set_output,
            selftest_done
        ])
        .run(tauri::generate_context!())
        .expect("error while running midzer");
}
