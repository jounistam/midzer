import { useEffect, useRef } from 'react';
import { setMidiOutput, setSplit, setVolume, SPLIT_OPTIONS, useSession } from '../session';

export function SettingsDrawer({ onClose }: { onClose: () => void }) {
  const s = useSession();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="drawer-backdrop" onClick={onClose}>
      <div
        className="drawer"
        role="dialog"
        aria-label="Settings"
        tabIndex={-1}
        ref={ref}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="drawer-head">
          <h2>Settings</h2>
          <button className="btn btn-ghost btn-icon" onClick={onClose} aria-label="Close">
            ×
          </button>
        </div>

        <label className="setting">
          <span>Volume</span>
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={s.volume}
            onChange={(e) => setVolume(Number(e.target.value))}
          />
        </label>

        <h3 className="label">Advanced</h3>

        <label className="setting">
          <span>
            Chord detection
            <small>Which keys tell the band the chord.</small>
          </span>
          <select value={s.split} onChange={(e) => setSplit(Number(e.target.value))}>
            {SPLIT_OPTIONS.map((o) => (
              <option key={o.note} value={o.note}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <div className="setting">
          <span>
            MIDI in
            <small>All connected devices are used.</small>
          </span>
          <span className="value">
            {!s.midiSupported
              ? 'Not supported in this browser — try Chrome or Edge'
              : s.midiInputs.length
                ? s.midiInputs.map((i) => i.name).join(', ')
                : 'None connected'}
          </span>
        </div>

        <label className="setting">
          <span>
            MIDI out
            <small>Send the band to a synth or DAW.</small>
          </span>
          <select
            value={s.midiOutputId ?? ''}
            onChange={(e) => setMidiOutput(e.target.value || null)}
            disabled={!s.midiOutputs.length}
          >
            <option value="">Off</option>
            {s.midiOutputs.map((o) => (
              <option key={o.id} value={o.id}>
                {o.name}
              </option>
            ))}
          </select>
        </label>
      </div>
    </div>
  );
}
