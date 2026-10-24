//! The desktop app: the web UI in a Tauri window, with the engine, MIDI, the
//! microphone and live audio on native threads (`midzer-native`). The UI talks to it
//! through the commands below and one update channel; see
//! `packages/audio-web/src/native.ts` for the other side.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use midzer_native::{NativeEngine, Ports, Update};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeBody, Request};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
struct AppEngine(Mutex<Option<NativeEngine>>);

/// A recording waiting to be analysed, and the flag that cancels its analysis.
#[derive(Default)]
struct AudioJob {
    bytes: Mutex<Option<Vec<u8>>>,
    cancel: AtomicBool,
}

#[derive(Serialize, Clone)]
struct AudioProgress {
    fraction: f32,
    step: u32,
}

#[derive(Serialize, Clone)]
struct Port {
    id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveDevice {
    id: String,
    name: String,
    loopback: bool,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum UiUpdate {
    Events { words: Vec<u32>, heard_frame: f64 },
    Input { words: Vec<u32> },
    Ports { inputs: Vec<Port>, outputs: Vec<Port> },
    Chords { spans: Vec<f64>, heard_frame: f64 },
    LiveSong { seconds: f64, flags: u32 },
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
            Update::Chords { spans, heard_frame } => UiUpdate::Chords { spans, heard_frame },
            Update::LiveSong { seconds, flags } => UiUpdate::LiveSong { seconds, flags },
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
fn live_devices(state: State<AppEngine>) -> Result<Vec<LiveDevice>, String> {
    with_engine(&state, |engine| {
        Ok(engine.live_devices().into_iter().map(|d| LiveDevice { id: d.id, name: d.name, loopback: d.loopback }).collect())
    })
}

#[tauri::command]
fn live_open(state: State<AppEngine>, id: String) -> Result<(), String> {
    with_engine(&state, |engine| engine.open_live(&id))
}

#[tauri::command]
fn live_reset(state: State<AppEngine>) -> Result<(), String> {
    with_engine(&state, |engine| {
        engine.reset_live();
        Ok(())
    })
}

#[tauri::command]
fn live_close(state: State<AppEngine>) -> Result<(), String> {
    with_engine(&state, |engine| {
        engine.close_live();
        Ok(())
    })
}

#[tauri::command]
fn live_song_finish(state: State<AppEngine>) -> Result<(), String> {
    with_engine(&state, |engine| {
        engine.finish_live_song();
        Ok(())
    })
}

/// Analyses the last song captured from the live source into a song (JSON, see `midzer_analyze::Analysis`).
#[tauri::command]
async fn live_song_analyze(app: AppHandle, on_progress: Channel<AudioProgress>) -> Result<String, String> {
    let song = with_engine(&app.state::<AppEngine>(), |engine| engine.take_live_song().ok_or_else(|| "No song was heard.".to_string()))?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut progress = |fraction: f32, step: midzer_analyze::Step| {
            let _ = on_progress.send(AudioProgress { fraction, step: step as u32 });
        };
        let analysis = midzer_analyze::analyze_heard(&song.samples, &mut progress)?;
        serde_json::to_string(&analysis).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn midi_ports(state: State<AppEngine>) -> Result<UiUpdate, String> {
    with_engine(&state, |engine| Ok(engine.midi_ports().into()))
}

#[tauri::command]
fn midi_set_output(state: State<AppEngine>, id: Option<String>) -> Result<(), String> {
    with_engine(&state, |engine| engine.set_midi_output(id.as_deref()))
}

/// Takes a recording, sent as the raw request body, for [`audio_analyze`].
#[tauri::command]
fn audio_load(job: State<AudioJob>, request: Request) -> Result<(), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected the recording as raw bytes.".into());
    };
    *job.bytes.lock().map_err(|e| e.to_string())? = Some(bytes.clone());
    Ok(())
}

/// Analyses the loaded recording into a song (JSON, see `midzer_analyze::Analysis`).
/// With `stems`, the instruments are separated first; the models are downloaded
/// into the app's data folder on first use and the stems saved next to them.
#[tauri::command]
async fn audio_analyze(app: AppHandle, stems: bool, on_progress: Channel<AudioProgress>) -> Result<String, String> {
    let job = app.state::<AudioJob>();
    let bytes = job.bytes.lock().map_err(|e| e.to_string())?.take().ok_or("No recording was loaded.")?;
    job.cancel.store(false, Ordering::Relaxed);
    let data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let job = worker.state::<AudioJob>();
        let mut progress = |fraction: f32, step: midzer_analyze::Step| {
            let _ = on_progress.send(AudioProgress { fraction, step: step as u32 });
        };
        let analysis = if stems {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let options = midzer_stems::Options {
                models: &data.join("models"),
                stems_out: Some(data.join("stems").join(stamp.to_string())),
                cancel: &job.cancel,
            };
            midzer_stems::transcribe_file(bytes, &options, &mut progress)
        } else {
            midzer_analyze::analyze_file(bytes, &mut progress)
        }?;
        serde_json::to_string(&analysis).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackLoaded {
    seconds: f32,
    drum_stem: bool,
}

/// Decodes a recording off the UI thread at the engine's rate and hands it to the engine.
async fn load_track(app: AppHandle, decode: impl FnOnce(u32) -> Result<midzer_analyze::Track, String> + Send + 'static) -> Result<TrackLoaded, String> {
    let rate = with_engine(&app.state::<AppEngine>(), |engine| Ok(engine.sample_rate()))?;
    let track = tauri::async_runtime::spawn_blocking(move || decode(rate)).await.map_err(|e| e.to_string())??;
    let loaded = TrackLoaded { seconds: track.frames() as f32 / rate as f32, drum_stem: track.drums.is_some() };
    with_engine(&app.state::<AppEngine>(), |engine| {
        engine.load_track(Some(track));
        Ok(loaded)
    })
}

/// Loads a recording, sent as the raw request body, as the songs' backing track.
#[tauri::command]
async fn track_load(app: AppHandle, request: Request<'_>) -> Result<TrackLoaded, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected the recording as raw bytes.".into());
    };
    let bytes = bytes.clone();
    load_track(app, move |rate| midzer_analyze::backing_track(bytes, rate)).await
}

/// Loads a song's saved stems as its backing track, with the drums apart.
#[tauri::command]
async fn track_load_stems(app: AppHandle, dir: String) -> Result<TrackLoaded, String> {
    load_track(app, move |rate| midzer_stems::backing_from_stems(std::path::Path::new(&dir), rate)).await
}

/// Frees the backing track.
#[tauri::command]
fn track_unload(state: State<AppEngine>) -> Result<(), String> {
    with_engine(&state, |engine| {
        engine.load_track(None);
        Ok(())
    })
}

/// Stops a running [`audio_analyze`] at its next step.
#[tauri::command]
fn audio_cancel(job: State<AudioJob>) {
    job.cancel.store(true, Ordering::Relaxed);
}

/// Ends a `MIDZER_SELFTEST` run: prints what the page saw and exits.
#[tauri::command]
fn selftest_done(app: AppHandle, ok: bool, log: String) {
    println!("{log}\nselftest: {}", if ok { "passed" } else { "FAILED" });
    app.exit(if ok { 0 } else { 1 });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // `MIDZER_SELFTEST=stems` also separates the stems of its test recording (models downloaded if needed).
    let selftest = std::env::var_os("MIDZER_SELFTEST").is_some();
    let stems = std::env::var("MIDZER_SELFTEST").is_ok_and(|v| v == "stems");
    if selftest {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(if stems { 360 } else { 60 }));
            eprintln!("selftest: timed out");
            std::process::exit(2);
        });
    }
    tauri::Builder::default()
        .manage(AppEngine::default())
        .manage(AudioJob::default())
        .on_page_load(move |webview, payload| {
            if selftest && payload.event() == PageLoadEvent::Finished {
                eprintln!("selftest: page loaded: {}", payload.url());
                let script = format!("window.__MIDZER_SELFTEST_STEMS = {stems};\n{}", include_str!("selftest.js"));
                if let Err(e) = webview.eval(&script) {
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
            live_devices,
            live_open,
            live_reset,
            live_close,
            live_song_finish,
            live_song_analyze,
            midi_ports,
            midi_set_output,
            audio_load,
            audio_analyze,
            audio_cancel,
            track_load,
            track_load_stems,
            track_unload,
            selftest_done
        ])
        .run(tauri::generate_context!())
        .expect("error while running midzer");
}
