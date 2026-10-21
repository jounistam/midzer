import type { BandPart, SectionRequest } from '@midzer/audio-web';
import type { CSSProperties } from 'react';
import { padState } from './bandStatus';
import { pickProgression, progressionsFor } from '../drums/progressions';
import {
  BAND_DRUMS_OPTIONS,
  requestSection,
  saveTake,
  setBandDrums,
  setProgression,
  setEnergy,
  setFollow,
  setTempo,
  tapTempo,
  toggleMute,
  togglePlay,
  useSession,
} from '../session';

const PARTS: { part: BandPart; label: string }[] = [
  { part: 'drums', label: 'Drums' },
  { part: 'bass', label: 'Bass' },
  { part: 'keys', label: 'Keys' },
  { part: 'pad', label: 'Pad' },
];

export function PlayButton({ className = '' }: { className?: string }) {
  const playing = useSession((s) => s.playing);
  const ready = useSession((s) => s.audio === 'ready');
  return (
    <button className={`btn btn-primary btn-xl ${className}`} onClick={togglePlay} disabled={!ready}>
      <span className={playing ? 'icon-stop' : 'icon-play'} aria-hidden />
      {playing ? 'Stop' : 'Play'}
    </button>
  );
}

export function TempoControl() {
  const bpm = useSession((s) => s.bpm);
  return (
    <div className="control-group">
      <p className="label">Tempo</p>
      <div className="tempo">
        <button className="btn btn-secondary btn-square" onClick={() => setTempo(bpm - 1)} aria-label="Slower">
          −
        </button>
        <span className="tempo-value">
          {bpm}
          <small>BPM</small>
        </span>
        <button className="btn btn-secondary btn-square" onClick={() => setTempo(bpm + 1)} aria-label="Faster">
          +
        </button>
        <button className="btn btn-secondary tap" onClick={() => tapTempo()}>
          Tap
        </button>
      </div>
    </div>
  );
}

export function BandMutes() {
  const muted = useSession((s) => s.muted);
  return (
    <div className="control-group">
      <p className="label">Band</p>
      <div className="band-grid">
        {PARTS.map(({ part, label }) => (
          <button
            key={part}
            className={`btn btn-secondary band-chip ${muted[part] ? 'muted' : ''}`}
            aria-pressed={!muted[part]}
            onClick={() => toggleMute(part)}
          >
            <span className={muted[part] ? 'icon-muted' : 'icon-sound'} aria-hidden />
            {label}
            {muted[part] && ' (muted)'}
          </button>
        ))}
      </div>
    </div>
  );
}

const PADS: { request: SectionRequest; label: string }[] = [
  { request: 'intro', label: 'Intro' },
  { request: 'A', label: 'A' },
  { request: 'B', label: 'B' },
  { request: 'C', label: 'C' },
  { request: 'fill', label: 'Fill' },
  { request: 'ending', label: 'Ending' },
];

export function SectionPads() {
  const band = useSession((s) => s.band);
  const ready = useSession((s) => s.audio === 'ready');
  return (
    <div className="control-group">
      <p className="label">Sections</p>
      <div className="pad-grid">
        {PADS.map(({ request, label }) => {
          const state = padState(request, band);
          return (
            <button
              key={request}
              className={`section-pad ${state}`}
              aria-pressed={state !== 'off'}
              aria-label={request.length === 1 ? `Variation ${label}` : label}
              title={state === 'queued' ? 'Starts on the next bar' : undefined}
              disabled={!ready}
              onClick={() => requestSection(request)}
            >
              {label}
            </button>
          );
        })}
      </div>
    </div>
  );
}

const energyFill = (energy: number): CSSProperties => {
  const pct = `${Math.round(energy * 100)}%`;
  return {
    background: `linear-gradient(90deg, var(--color-accent-default), var(--color-primary-default) ${pct}, var(--color-surface-track) ${pct})`,
  };
};

export function EnergyControl() {
  const energy = useSession((s) => s.band.energy);
  const follow = useSession((s) => s.band.follow);
  return (
    <div className="control-group">
      <div className="control-head">
        <p className="label" id="energy-label">
          Energy
        </p>
        <button
          className={`toggle ${follow ? 'on' : ''}`}
          aria-pressed={follow}
          onClick={() => setFollow(!follow)}
          title="Energy and variation follow how hard and how much you play"
        >
          Follow me
        </button>
      </div>
      <input
        className={`energy ${follow ? 'following' : ''}`}
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={energy}
        aria-labelledby="energy-label"
        aria-valuetext={`${Math.round(energy * 100)}%${follow ? ', following you' : ''}`}
        style={energyFill(energy)}
        onChange={(e) => setEnergy(Number(e.target.value))}
      />
    </div>
  );
}

/** Saves the last performance (Play to Stop) as a MIDI file. */
export function TakeButton() {
  const hasTake = useSession((s) => s.hasTake);
  const playing = useSession((s) => s.playing);
  return (
    <div className="control-group">
      <p className="label">Recording</p>
      <button
        className="btn btn-secondary"
        onClick={saveTake}
        disabled={!hasTake}
        title={hasTake ? 'Your notes and the band, one track each' : undefined}
      >
        Save last take as MIDI
      </button>
      <p className="setting-note">
        {playing ? 'Recording… stop to save.' : hasTake ? 'Your notes and every band part.' : 'Everything from Play to Stop is recorded.'}
      </p>
    </div>
  );
}

/** The chords a drummer plays over: a loop picked from the style's progressions. */
export function ProgressionPicker() {
  const style = useSession((s) => s.style);
  const current = useSession((s) => pickProgression(s.style, s.progression));
  return (
    <label className="control-group">
      <span className="label">Chords</span>
      <select value={current.id} onChange={(e) => setProgression(e.target.value)} aria-label="Chords">
        {progressionsFor(style).map((p) => (
          <option key={p.id} value={p.id}>
            {p.name} · {p.hint}
          </option>
        ))}
      </select>
    </label>
  );
}

/** What the band's own drums play while the player drums. */
export function BandDrumsControl() {
  const mode = useSession((s) => s.bandDrums);
  return (
    <div className="control-group">
      <p className="label" id="band-drums-label">
        Band drums
      </p>
      <div className="segmented" role="group" aria-labelledby="band-drums-label">
        {BAND_DRUMS_OPTIONS.map((o) => (
          <button key={o.mode} className={`seg ${mode === o.mode ? 'on' : ''}`} aria-pressed={mode === o.mode} onClick={() => setBandDrums(o.mode)}>
            {o.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function ControlPanel({ drummer = false }: { drummer?: boolean }) {
  return (
    <aside className="panel control-panel">
      <PlayButton className="full" />
      {drummer && <ProgressionPicker />}
      <SectionPads />
      {drummer && <BandDrumsControl />}
      <EnergyControl />
      <TempoControl />
      <BandMutes />
      <TakeButton />
    </aside>
  );
}
