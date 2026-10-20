import { useState } from 'react';
import { goTo, useSession } from '../session';
import { SettingsDrawer } from './SettingsDrawer';

export function TopBar({ title, onBack }: { title: string; onBack?: () => void }) {
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
      <div className="style-chip">Style: Pop · Straight 8ths</div>
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
