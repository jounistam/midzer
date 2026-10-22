import { STYLES } from '@midzer/audio-web';
import { useState } from 'react';
import { goTo, setStyle, useSession } from '../session';
import { SettingsDrawer } from './SettingsDrawer';

/** `styles` off hides the style picker where the band doesn't play. */
export function TopBar({ title, onBack, styles = true }: { title: string; onBack?: () => void; styles?: boolean }) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  return (
    <header className="top-bar">
      <div className="top-left">
        {onBack && (
          <button className="btn btn-ghost btn-icon" onClick={onBack} aria-label="Back">
            ‹
          </button>
        )}
        <button className="brand" onClick={() => goTo('home')}>
          midzer
        </button>
        <span className="crumb">/</span>
        <span className="title">{title}</span>
      </div>
      {styles ? <StylePicker /> : <span />}
      <div className="top-right">
        <DeviceChip />
        <button className="btn btn-secondary btn-icon" onClick={() => setSettingsOpen(true)} aria-label="Settings">
          <span className="icon-sliders" aria-hidden />
        </button>
      </div>
      {settingsOpen && <SettingsDrawer onClose={() => setSettingsOpen(false)} />}
    </header>
  );
}

export function StylePicker() {
  const style = useSession((s) => s.style);
  const heard = useSession((s) => s.band.style);
  const playing = useSession((s) => s.playing);
  const queued = playing && style !== heard;
  return (
    <label className={`style-chip ${queued ? 'queued' : ''}`}>
      <span className="label">Style</span>
      <select value={style} onChange={(e) => setStyle(Number(e.target.value))} aria-label="Style">
        {STYLES.map((s, i) => (
          <option key={s.name} value={i}>
            {s.name} · {s.blurb}
          </option>
        ))}
      </select>
      {queued && <span className="queued-note">next bar</span>}
    </label>
  );
}

export function DeviceChip() {
  const inputs = useSession((s) => s.midiInputs);
  const supported = useSession((s) => s.midiSupported);
  const name = inputs[0]?.name;
  return (
    <div className={`device-chip ${name ? 'connected' : ''}`} title={inputs.map((i) => i.name).join(', ')}>
      <span className="dot" aria-hidden />
      {name ?? (supported ? 'Computer keyboard' : 'No MIDI in this browser')}
      {inputs.length > 1 && ` +${inputs.length - 1}`}
    </div>
  );
}
