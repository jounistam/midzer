import type { BandPart } from '@midzer/audio-web';
import { setTempo, tapTempo, toggleMute, togglePlay, useSession } from '../session';

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

export function ControlPanel() {
  return (
    <aside className="panel control-panel">
      <PlayButton className="full" />
      <TempoControl />
      <BandMutes />
      <p className="panel-note">Sections, fills and energy come in the next update.</p>
    </aside>
  );
}
