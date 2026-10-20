import { useEffect, useRef, useState } from 'react';
import { onBeat, setCalibrationClick, setTapOffset, useSession } from '../session';
import { tapOffset } from './calibrate';

const TAPS_NEEDED = 8;

/**
 * Tap-along latency calibration: the engine plays a click on every beat and
 * the player taps with it. The median distance between taps and the heard
 * clicks is how late the player's notes land, on top of the output latency.
 */
export function Calibration({ onClose }: { onClose: () => void }) {
  const [phase, setPhase] = useState<'intro' | 'tapping' | 'done'>('intro');
  const [taps, setTaps] = useState<number[]>([]);
  const [result, setResult] = useState<number | null>(null);
  const beats = useRef<number[]>([]);
  const saved = useSession((s) => s.tapOffsetMs);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => ref.current?.focus(), [phase]);

  // Stop the click whenever the dialog goes away.
  useEffect(() => () => setCalibrationClick(false), []);

  useEffect(() => {
    if (phase !== 'tapping') return;
    beats.current = [];
    const off = onBeat((_bar, _beat, timeMs) => {
      beats.current.push(timeMs);
      if (beats.current.length > 64) beats.current.shift();
    });
    setCalibrationClick(true);
    return () => {
      off();
      setCalibrationClick(false);
    };
  }, [phase]);

  const tap = (timeMs: number) => {
    if (phase !== 'tapping') return;
    const next = [...taps, timeMs];
    setTaps(next);
    if (next.length >= TAPS_NEEDED) {
      setResult(tapOffset(next, beats.current));
      setPhase('done');
    }
  };

  // Keys tap instead of playing notes while the dialog is open.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      e.stopImmediatePropagation();
      if (e.key === 'Escape') return onClose();
      if (e.key === 'Tab') return;
      e.preventDefault();
      if (!e.repeat) tap(e.timeStamp);
    };
    const swallow = (e: KeyboardEvent) => e.stopImmediatePropagation();
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('keyup', swallow, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('keyup', swallow, true);
    };
  });

  const restart = () => {
    setTaps([]);
    setResult(null);
    setPhase('tapping');
  };

  return (
    <div className="drawer-backdrop centered" onClick={onClose}>
      <div className="dialog" role="dialog" aria-label="Calibrate timing" tabIndex={-1} ref={ref} onClick={(e) => e.stopPropagation()}>
        <h2>Calibrate timing</h2>
        {phase === 'intro' && (
          <>
            <p className="lead">
              You'll hear a click on every beat. Tap any key, or the pad below, right with the clicks — {TAPS_NEEDED} times.
              The band then knows when you mean a beat, even with a slow sound card.
            </p>
            {saved !== null && <p className="setting-note">Current calibration: {saved} ms</p>}
            <div className="actions">
              <button className="btn btn-primary btn-xl" onClick={restart}>
                Start
              </button>
              <button className="btn btn-ghost" onClick={onClose}>
                Cancel
              </button>
              {saved !== null && (
                <button className="btn btn-ghost" onClick={() => setTapOffset(null)}>
                  Reset
                </button>
              )}
            </div>
          </>
        )}
        {phase === 'tapping' && (
          <>
            <p className="lead">Tap with the click.</p>
            <button className="tap-pad" onPointerDown={(e) => tap(e.timeStamp)} aria-label="Tap">
              Tap
            </button>
            <div className="tap-dots" aria-label={`${taps.length} of ${TAPS_NEEDED} taps`}>
              {Array.from({ length: TAPS_NEEDED }, (_, i) => (
                <span key={i} className={`beat-dot ${i < taps.length ? 'done' : ''}`} />
              ))}
            </div>
          </>
        )}
        {phase === 'done' && (
          <>
            {result === null ? (
              <p className="lead">Those taps were too uneven to measure. Try again, following the click closely.</p>
            ) : (
              <p className="lead">
                Your taps land <strong>{Math.abs(Math.round(result))} ms</strong> {result >= 0 ? 'after' : 'before'} the click.
              </p>
            )}
            <div className="actions">
              {result !== null && (
                <button
                  className="btn btn-primary btn-xl"
                  onClick={() => {
                    setTapOffset(result);
                    onClose();
                  }}
                >
                  Save
                </button>
              )}
              <button className="btn btn-secondary" onClick={restart}>
                Try again
              </button>
              <button className="btn btn-ghost" onClick={onClose}>
                Cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
