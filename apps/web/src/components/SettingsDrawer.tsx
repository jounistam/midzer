import { NOTE_NAMES } from '@midzer/audio-web';
import { useEffect, useRef, useState } from 'react';
import { live } from '../live';
import {
  IDLE_OPTIONS,
  LISTEN_OPTIONS,
  playerLatencyMs,
  setHeadphones,
  setIdleMode,
  setListenMode,
  setMicGate,
  setMidiOutput,
  setSplit,
  setTempoFollow,
  setVolume,
  SPLIT_OPTIONS,
  useSession,
} from '../session';
import { Calibration } from './Calibration';

export function SettingsDrawer({ onClose }: { onClose: () => void }) {
  const s = useSession();
  const ref = useRef<HTMLDivElement>(null);
  const [calibrating, setCalibrating] = useState(false);

  useEffect(() => {
    ref.current?.focus();
    if (calibrating) return;
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose, calibrating]);

  if (calibrating) return <Calibration onClose={() => setCalibrating(false)} />;

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

        <label className="setting">
          <span>
            When you stop playing
            <small>After two quiet bars.</small>
          </span>
          <select value={s.idleMode} onChange={(e) => setIdleMode(e.target.value as (typeof IDLE_OPTIONS)[number]['mode'])}>
            {IDLE_OPTIONS.map((o) => (
              <option key={o.mode} value={o.mode}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <div className="setting">
          <span>
            Timing
            <small>
              {s.tapOffsetMs === null ? 'Not calibrated' : `Calibrated: ${s.tapOffsetMs > 0 ? '+' : ''}${s.tapOffsetMs} ms`}
              {s.audio === 'ready' && ` · ${playerLatencyMs()} ms total`}
            </small>
          </span>
          <button className="btn btn-secondary" onClick={() => setCalibrating(true)} disabled={s.audio !== 'ready'}>
            Calibrate
          </button>
        </div>

        <div className="setting">
          <span>
            Follow my tempo
            <small>The band speeds up and slows down with you.</small>
          </span>
          <button
            className={`toggle ${s.tempoFollow ? 'on' : ''}`}
            aria-pressed={s.tempoFollow}
            onClick={() => setTempoFollow(!s.tempoFollow)}
          >
            {s.tempoFollow ? 'On' : 'Off'}
          </button>
        </div>

        <MicSettings />

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

function MicSettings() {
  const s = useSession();
  const hint = LISTEN_OPTIONS.find((o) => o.mode === s.listenMode)?.hint;
  return (
    <section className="mic-settings" aria-labelledby="mic-label">
      <h3 className="label" id="mic-label">
        Microphone
      </h3>
      <div className="segmented" role="group" aria-label="Listen for">
        {LISTEN_OPTIONS.map((o) => (
          <button
            key={o.mode}
            className={`seg ${s.listenMode === o.mode ? 'on' : ''}`}
            aria-pressed={s.listenMode === o.mode}
            disabled={s.audio !== 'ready'}
            onClick={() => void setListenMode(o.mode)}
          >
            {o.label}
          </button>
        ))}
      </div>
      {s.micError && (
        <p className="setting-note error" role="alert">
          {s.micError}
        </p>
      )}
      {s.listenMode !== 'off' && (
        <>
          {hint && <p className="setting-note">{hint}</p>}
          <MicMeter gateDb={s.micGateDb} showNote={s.listenMode === 'notes'} />
          <label className="setting">
            <span>
              Sensitivity
              <small>Quieter sounds than the line are ignored.</small>
            </span>
            <input
              type="range"
              min={-80}
              max={-20}
              step={1}
              value={-100 - s.micGateDb}
              aria-label="Sensitivity"
              onChange={(e) => setMicGate(-100 - Number(e.target.value))}
            />
          </label>
          <div className="setting">
            <span>
              Headphones
              <small>{s.headphones ? 'Clearest pitch.' : 'With speakers the mic also hears the band.'}</small>
            </span>
            <button className={`toggle ${s.headphones ? 'on' : ''}`} aria-pressed={s.headphones} onClick={() => setHeadphones(!s.headphones)}>
              {s.headphones ? 'On' : 'Off'}
            </button>
          </div>
        </>
      )}
    </section>
  );
}

/** Live input level with the gate marked, and the note being heard. */
function MicMeter({ gateDb, showNote }: { gateDb: number; showNote: boolean }) {
  const fill = useRef<HTMLDivElement>(null);
  const note = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    let frame = 0;
    const draw = () => {
      if (fill.current) fill.current.style.width = `${Math.round(live.mic.level * 100)}%`;
      if (note.current) {
        const key = live.mic.key;
        note.current.textContent = key < 0 ? '–' : `${NOTE_NAMES[key % 12]}${Math.floor(key / 12) - 1}`;
      }
      frame = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(frame);
  }, []);
  const gatePercent = ((gateDb + 80) / 80) * 100;
  return (
    <div className="mic-meter">
      <div className="mic-meter-track" role="meter" aria-label="Microphone level">
        <div className="mic-meter-fill" ref={fill} />
        <div className="mic-meter-gate" style={{ left: `${gatePercent}%` }} />
      </div>
      {showNote && <span className="mic-meter-note" ref={note} aria-label="Note heard" />}
    </div>
  );
}
