import { useEffect, useMemo, useRef, useState } from 'react';
import { ChartView, type Selection } from '../components/ChartView';
import { BandMutes, EnergyControl, PlayButton, TakeButton, TempoControl } from '../components/ControlPanel';
import { Keyboard } from '../components/Keyboard';
import { KitView } from '../components/KitView';
import { TimingBar } from '../components/DrummerStage';
import { HitFeedback } from '../components/HitFeedback';
import { useStageHeight } from '../components/stageHeight';
import { TimingReport } from '../components/TimingReport';
import { TopBar } from '../components/TopBar';
import {
  analyzeSongAgain,
  closeSong,
  editSong,
  saveSong,
  saveTranscription,
  seekSong,
  setLoop,
  setPlayRecording,
  setRecordingDrums,
  setTempo,
  useSession,
  type LoopRange,
} from '../session';
import { STEP_LABELS } from '../songs/audio';
import { isBuiltin } from '../songs/library';
import {
  barStarts,
  emptyBar,
  formatChart,
  MAX_BARS,
  parseChart,
  resolvedBeats,
  SECTION_LABELS,
  type Bar,
  type Song,
  type SongSection,
} from '../songs/song';
import { LoadingStatus } from './LoadingStatus';
import { MissSoundControl } from './PlayAlong';

export function SongScreen() {
  const song = useSession((s) => s.song);
  const split = useSession((s) => s.split);
  const drums = useSession((s) => s.instrument === 'drums');
  const notes = useSession((s) => s.songNotes);
  const recording = useSession((s) => s.recording.status === 'ready' && s.playRecording);
  const kitHeight = useStageHeight(200);
  const [editing, setEditing] = useState(false);
  const [selection, setSelection] = useState<Selection | null>(null);
  if (!song) return null;

  const select = (bar: number, extend: boolean) => {
    const next =
      extend && selection
        ? { start: Math.min(selection.start, bar), end: Math.max(selection.end, bar + 1) }
        : { start: bar, end: bar + 1 };
    setSelection(next);
    // A plain click moves the start; extending a selection (for a loop) doesn't.
    if (!editing && !extend) seekSong(bar);
  };

  return (
    <div className="jam song-screen">
      <TopBar title="Songs" onBack={() => closeSong('songs')} />
      <main className="jam-main">
        <div className="song-stage">
          <NowPlaying song={song} />
          {(drums || recording) && <PlayTiming recording={recording} />}
          {notes.length > 0 && (
            <p className="notice" role="status">
              {notes.join(' ')}
            </p>
          )}
          <section className="panel chart-panel">
            <SongHeader song={song} editing={editing} onEditing={setEditing} />
            {editing && <EditTools song={song} selection={selection} onSelection={setSelection} />}
            <Chart song={song} editing={editing} selection={selection} onSelect={select} />
            <p className="setting-note">
              {editing
                ? 'Type the chords of each bar: "C" fills the bar, "Am F" splits it, "C . G7 ." gives each beat; "[3/4] C" makes it three beats long. Shift-click bar numbers to select several.'
                : 'Click a bar to start there (while playing, the band jumps there at the next bar). Shift-click to select bars to loop.'}
            </p>
          </section>
          <LoadingStatus />
        </div>
        <aside className="panel control-panel">
          <PlayButton className="full" />
          {song.backing && <RecordingControl />}
          {song.backing && !isBuiltin(song) && <AnalyseAgain />}
          {recording && <MissSoundControl />}
          <LoopControl song={song} selection={selection} />
          {!recording && (
            <>
              <EnergyControl />
              <TempoControl />
              <BandMutes />
            </>
          )}
          <TakeButton />
          {song.transcription && <TranscriptionButton tracks={song.transcription.length} />}
        </aside>
      </main>
      {drums ? <KitView height={kitHeight} /> : <Keyboard split={split} />}
    </div>
  );
}

/** How a drummer's hits sit against the band; or how a drummer's or keys player's sit against the recording, with a report after each run. */
function PlayTiming({ recording }: { recording: boolean }) {
  const playing = useSession((s) => s.playing);
  if (recording) {
    return playing ? <HitFeedback against="the recording" /> : <section className="panel song-timing"><TimingReport /></section>;
  }
  return <section className="panel song-timing">{playing ? <TimingBar /> : <p className="setting-note">Count in, or press Play: the band follows your tempo.</p>}</section>;
}

/** Songs read from a recording can play it instead of the band; on the desktop without its drums. */
function RecordingControl() {
  const recording = useSession((s) => s.recording);
  const on = useSession((s) => s.playRecording);
  const drums = useSession((s) => s.recordingDrums);
  const audio = useSession((s) => s.audio);
  return (
    <div className="control-group">
      <p className="label" id="play-along-label">
        Play along with
      </p>
      <div className="segmented" role="group" aria-labelledby="play-along-label">
        <button className={`seg ${on ? 'on' : ''}`} aria-pressed={on} onClick={() => setPlayRecording(true)}>
          Recording
        </button>
        <button className={`seg ${!on ? 'on' : ''}`} aria-pressed={!on} onClick={() => setPlayRecording(false)}>
          Band
        </button>
      </div>
      {on && recording.status === 'loading' && (
        <p className="setting-note" role="status">
          Reading the recording…
        </p>
      )}
      {on && recording.status === 'none' && audio !== 'ready' && <p className="setting-note">The recording loads once sound is on.</p>}
      {on && recording.status === 'error' && (
        <p className="status-line error" role="alert">
          {recording.message}
        </p>
      )}
      {on && recording.status === 'ready' && recording.drumStem && (
        <button className={`toggle ${drums ? 'on' : ''}`} aria-pressed={drums} onClick={() => setRecordingDrums(!drums)}>
          {drums ? 'Drums in the recording: on' : 'Drums in the recording: off'}
        </button>
      )}
      {on && recording.status === 'ready' && (
        <p className="setting-note">
          The recording keeps its own tempo; the chart follows its beat.
          {!recording.drumStem && ' Separate the instruments on import (desktop app) to play it without its drums.'}
        </p>
      )}
    </div>
  );
}

type Reading = { type: 'idle' } | { type: 'confirm' } | { type: 'working'; fraction: number; step: number } | { type: 'error'; message: string };

/** Reads the song's recording again with the current analysis; it replaces the chords and beat map, hand edits included. */
function AnalyseAgain() {
  const [state, setState] = useState<Reading>({ type: 'idle' });
  const abort = useRef<AbortController | null>(null);
  useEffect(() => () => abort.current?.abort(), []);
  const start = () => {
    const controller = new AbortController();
    abort.current = controller;
    setState({ type: 'working', fraction: 0, step: 0 });
    analyzeSongAgain((fraction, step) => !controller.signal.aborted && setState({ type: 'working', fraction, step }), controller.signal)
      .then(() => !controller.signal.aborted && setState({ type: 'idle' }))
      .catch((e: unknown) => !controller.signal.aborted && setState({ type: 'error', message: e instanceof Error ? e.message : String(e) }));
  };
  const cancel = () => {
    abort.current?.abort();
    setState({ type: 'idle' });
  };
  const pct = state.type === 'working' ? Math.round(state.fraction * 100) : 0;
  return (
    <div className="control-group">
      <p className="label">Beat and chords</p>
      {(state.type === 'idle' || state.type === 'error') && (
        <button className="btn btn-secondary" onClick={() => setState({ type: 'confirm' })}>
          Analyse again
        </button>
      )}
      {state.type === 'confirm' && (
        <>
          <p className="setting-note">Reads the recording again for the beat, chords and sections. Your changes to the chords are replaced.</p>
          <div className="actions">
            <button className="btn btn-primary" onClick={start}>
              Replace them
            </button>
            <button className="btn btn-ghost" onClick={cancel}>
              Cancel
            </button>
          </div>
        </>
      )}
      {state.type === 'working' && (
        <>
          <div className="status-line" role="status">
            <span>
              {STEP_LABELS[state.step] ?? 'Listening'}… {pct}%
            </span>
            <span className="progress" aria-hidden>
              <span style={{ width: `${pct}%` }} />
            </span>
          </div>
          <button className="btn btn-ghost" onClick={cancel}>
            Cancel
          </button>
        </>
      )}
      {state.type === 'error' && (
        <p className="status-line error" role="alert">
          {state.message}
        </p>
      )}
    </div>
  );
}

function Chart(props: { song: Song; editing: boolean; selection: Selection | null; onSelect: (bar: number, extend: boolean) => void }) {
  const songBar = useSession((s) => s.songBar);
  const beat = useSession((s) => s.beat);
  const startBar = useSession((s) => s.songStartBar);
  const loop = useSession((s) => s.loop);
  const onEditBar = (index: number, bar: Bar) =>
    editSong({ ...props.song, bars: props.song.bars.map((b, i) => (i === index ? bar : b)) });
  return (
    <ChartView
      {...props}
      onEditBar={onEditBar}
      currentBar={songBar}
      beat={beat}
      startBar={startBar}
      loop={loop}
    />
  );
}

/** The chord being heard and the next change. */
function NowPlaying({ song }: { song: Song }) {
  const chord = useSession((s) => s.chord);
  const songBar = useSession((s) => s.songBar);
  const beat = useSession((s) => s.beat);
  const startBar = useSession((s) => s.songStartBar);
  const beats = useMemo(() => resolvedBeats(song.bars), [song.bars]);
  const starts = useMemo(() => barStarts(song.bars), [song.bars]);

  const playing = songBar !== null;
  const at = playing ? (starts[songBar] ?? 0) + beat : (starts[startBar] ?? 0);
  const barBeats = song.bars[playing ? songBar : startBar]?.beats.length ?? 4;
  const now = playing ? (chord?.name ?? '—') : (beats[at] ?? '—');
  let next: { name: string; in: number } | null = null;
  for (let i = at + 1; i < beats.length; i++) {
    if (beats[i] !== beats[at]) {
      next = { name: beats[i] ?? 'N.C.', in: i - at };
      break;
    }
  }

  return (
    <section className="panel song-now" aria-live="polite">
      <div>
        <p className="eyebrow">{playing ? `Bar ${songBar + 1} of ${song.bars.length}` : `Starts at bar ${startBar + 1}`}</p>
        <h2 className={`now-chord ${now === '—' ? 'empty' : ''}`}>{now}</h2>
      </div>
      <div className="next-chord">
        <p className="eyebrow">Next</p>
        <p className="next-name">{next?.name ?? 'End'}</p>
        {next && playing && <p className="next-when">in {next.in} {next.in === 1 ? 'beat' : 'beats'}</p>}
      </div>
      <div className="beats" aria-hidden>
        {Array.from({ length: barBeats }, (_, b) => b).map((b) => (
          <span key={b} className={`beat-dot ${playing && b === beat ? 'active' : ''}`} />
        ))}
      </div>
    </section>
  );
}

function SongHeader({ song, editing, onEditing }: { song: Song; editing: boolean; onEditing: (on: boolean) => void }) {
  const dirty = useSession((s) => s.songDirty);
  const [saveNote, setSaveNote] = useState<string | null>(null);
  const save = () =>
    void saveSong().then((ok) => setSaveNote(ok ? 'Saved to My songs' : 'Could not save: browser storage is full or blocked'));
  const update = (fields: Partial<Song>) => {
    setSaveNote(null);
    editSong({ ...song, ...fields });
  };

  return (
    <div className="chart-head">
      {editing ? (
        <div className="song-fields">
          <label>
            <span className="label">Title</span>
            <input value={song.title} onChange={(e) => update({ title: e.target.value })} />
          </label>
          <label>
            <span className="label">Artist</span>
            <input value={song.artist ?? ''} onChange={(e) => update({ artist: e.target.value || undefined })} />
          </label>
          <label>
            <span className="label">Tempo</span>
            <input
              type="number"
              min={40}
              max={240}
              value={song.bpm}
              onChange={(e) => {
                const bpm = Math.round(Number(e.target.value));
                if (bpm >= 40 && bpm <= 240) {
                  update({ bpm });
                  setTempo(bpm);
                }
              }}
            />
          </label>
        </div>
      ) : (
        <div>
          <h1 className="song-title">{song.title}</h1>
          {song.artist && <p className="song-artist">{song.artist}</p>}
        </div>
      )}
      <div className="chart-actions">
        {saveNote && <span className="save-note">{saveNote}</span>}
        {(dirty || (editing && isBuiltin(song))) && (
          <button className="btn btn-primary" onClick={save}>
            {isBuiltin(song) ? 'Save a copy' : 'Save'}
          </button>
        )}
        <button className="btn btn-secondary" onClick={() => onEditing(!editing)} aria-pressed={editing}>
          {editing ? 'Done' : 'Edit chart'}
        </button>
      </div>
    </div>
  );
}

const SECTION_OPTIONS: (SongSection | '')[] = ['', 'intro', 'A', 'B', 'C', 'ending'];

function EditTools({
  song,
  selection,
  onSelection,
}: {
  song: Song;
  selection: Selection | null;
  onSelection: (s: Selection | null) => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const [textError, setTextError] = useState<string | null>(null);
  const sel = selection && selection.start < song.bars.length ? selection : null;
  const bars = song.bars;
  const setBars = (next: Bar[]) => editSong({ ...song, bars: next.slice(0, MAX_BARS) });
  const first = sel ? bars[sel.start] : undefined;
  const selected = sel ? bars.slice(sel.start, sel.end) : [];

  const setSection = (value: SongSection | '') =>
    sel &&
    setBars(bars.map((b, i) => (i === sel.start ? withSection(b, value || undefined) : b)));
  const toggleFill = () => {
    if (!sel) return;
    const on = !selected.every((b) => b.fill);
    setBars(bars.map((b, i) => (i >= sel.start && i < sel.end ? withFill(b, on) : b)));
  };
  const insert = (at: number, added: Bar[]) => {
    setBars([...bars.slice(0, at), ...added, ...bars.slice(at)]);
    onSelection({ start: at, end: at + added.length });
  };
  const remove = () => {
    if (!sel || sel.end - sel.start >= bars.length) return;
    setBars([...bars.slice(0, sel.start), ...bars.slice(sel.end)]);
    onSelection(null);
  };
  const applyText = () => {
    const { bars: parsed, unknown } = parseChart(text ?? '');
    if (parsed.length === 0) return setTextError('No bars found.');
    if (unknown.length) return setTextError(`Not chords: ${[...new Set(unknown)].join(', ')}`);
    // Keep lyrics where bars line up.
    setBars(parsed.map((b, i) => (bars[i]?.lyric ? { ...b, lyric: bars[i]!.lyric } : b)));
    setText(null);
    setTextError(null);
    onSelection(null);
  };

  if (text !== null) {
    return (
      <div className="edit-tools text-mode">
        <textarea
          className="chart-text"
          value={text}
          rows={Math.min(16, text.split('\n').length + 2)}
          aria-label="Chart as text"
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
        />
        {textError && (
          <p className="status-line error" role="alert">
            {textError}
          </p>
        )}
        <div className="actions">
          <button className="btn btn-primary" onClick={applyText}>
            Apply
          </button>
          <button className="btn btn-ghost" onClick={() => setText(null)}>
            Cancel
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="edit-tools" role="toolbar" aria-label="Edit bars">
      <span className="label">{sel ? `Bar${sel.end - sel.start > 1 ? `s ${sel.start + 1}–${sel.end}` : ` ${sel.start + 1}`}` : 'Select a bar'}</span>
      <select
        aria-label="Section starting here"
        value={first?.section ?? ''}
        disabled={!sel}
        onChange={(e) => setSection(e.target.value as SongSection | '')}
      >
        {SECTION_OPTIONS.map((s) => (
          <option key={s} value={s}>
            {s ? `${SECTION_LABELS[s]} starts here` : 'No section change'}
          </option>
        ))}
      </select>
      <button className={`toggle ${selected.length && selected.every((b) => b.fill) ? 'on' : ''}`} disabled={!sel} onClick={toggleFill}>
        Fill
      </button>
      <button className="btn btn-secondary" onClick={() => insert(sel ? sel.end : bars.length, [emptyBar((sel ? bars[sel.end - 1] : bars[bars.length - 1])?.beats.length)])}>
        Add bar
      </button>
      <button className="btn btn-secondary" disabled={!sel} onClick={() => sel && insert(sel.end, selected.map(withoutSection))}>
        Duplicate
      </button>
      <button className="btn btn-secondary" disabled={!sel || sel.end - sel.start >= bars.length} onClick={remove}>
        Delete
      </button>
      <button className="btn btn-ghost" onClick={() => setText(formatChart(bars))}>
        Edit as text
      </button>
    </div>
  );
}

function withSection(bar: Bar, section: SongSection | undefined): Bar {
  const { section: _, ...rest } = bar;
  return section ? { ...rest, section } : rest;
}

function withFill(bar: Bar, fill: boolean): Bar {
  const { fill: _, ...rest } = bar;
  return fill ? { ...rest, fill } : rest;
}

function withoutSection(bar: Bar): Bar {
  return withSection(bar, undefined);
}

function LoopControl({ song, selection }: { song: Song; selection: Selection | null }) {
  const loop = useSession((s) => s.loop);
  const range: LoopRange =
    selection && selection.end <= song.bars.length ? selection : { start: 0, end: song.bars.length };
  const describe = (r: LoopRange) =>
    r.start === 0 && r.end === song.bars.length
      ? 'the whole song'
      : r.end - r.start === 1
        ? `bar ${r.start + 1}`
        : `bars ${r.start + 1}–${r.end}`;
  return (
    <div className="control-group">
      <p className="label">Practice</p>
      <button
        className={`btn btn-secondary loop-button ${loop ? 'on' : ''}`}
        aria-pressed={loop !== null}
        onClick={() => setLoop(loop ? null : range)}
      >
        {loop ? `Looping ${describe(loop)}` : `Loop ${describe(range)}`}
      </button>
    </div>
  );
}

/** For songs made from a recording with separated instruments: the transcribed parts as MIDI. */
function TranscriptionButton({ tracks }: { tracks: number }) {
  return (
    <div className="control-group">
      <p className="label">Transcription</p>
      <button className="btn btn-secondary" onClick={saveTranscription}>
        Save transcription as MIDI
      </button>
      <p className="setting-note">{tracks} parts heard in the recording, on the chart's beat grid.</p>
    </div>
  );
}
