import { useEffect, useRef, useState } from 'react';
import { analyzeAudio, canSeparateStems } from '../songs/analyzeAudio';
import { songFromAnalysis, STEP_LABELS, type FromAudio } from '../songs/audio';

type Phase = { type: 'choose' } | { type: 'working'; fraction: number; step: number } | { type: 'error'; message: string };

/** Turns a recording into a song: pick how (desktop), then follow the analysis. */
export function AudioImport({
  file,
  bytes,
  title,
  onDone,
  onClose,
}: {
  file: string;
  bytes: Uint8Array;
  title: string;
  onDone: (result: FromAudio) => void;
  onClose: () => void;
}) {
  const [stems, setStems] = useState(false);
  const [phase, setPhase] = useState<Phase>(canSeparateStems() ? { type: 'choose' } : { type: 'working', fraction: 0, step: 0 });
  const abort = useRef<AbortController | null>(null);

  const start = (withStems: boolean) => {
    const controller = new AbortController();
    abort.current = controller;
    setPhase({ type: 'working', fraction: 0, step: 0 });
    analyzeAudio(bytes, {
      stems: withStems,
      signal: controller.signal,
      onProgress: (fraction, step) => !controller.signal.aborted && setPhase({ type: 'working', fraction, step }),
    })
      .then((analysis) => !controller.signal.aborted && onDone(songFromAnalysis(analysis, title)))
      .catch((e: unknown) => {
        if (controller.signal.aborted) return;
        setPhase({ type: 'error', message: e instanceof Error ? e.message : String(e) });
      });
  };

  // In the browser there is nothing to choose: start right away.
  const started = useRef(false);
  useEffect(() => {
    if (!canSeparateStems() && !started.current) {
      started.current = true;
      start(false);
    }
    return () => abort.current?.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cancel = () => {
    abort.current?.abort();
    onClose();
  };
  const pct = phase.type === 'working' ? Math.round(phase.fraction * 100) : 0;

  return (
    <div className="drawer-backdrop centered">
      <div className="dialog audio-dialog" role="dialog" aria-label="Song from a recording">
        <h2>Song from a recording</h2>
        <p className="setting-note">
          <strong>{file}</strong> is read on this device and never uploaded. Use recordings you have the right to use.
        </p>
        {phase.type === 'choose' && (
          <>
            <div className="segmented audio-modes" role="group" aria-label="How to read it">
              <button className={`seg ${!stems ? 'on' : ''}`} aria-pressed={!stems} onClick={() => setStems(false)}>
                Quick
              </button>
              <button className={`seg ${stems ? 'on' : ''}`} aria-pressed={stems} onClick={() => setStems(true)}>
                Separate the instruments
              </button>
            </div>
            <p className="setting-note">
              {stems
                ? 'Splits the recording into drums, bass, other instruments and vocals first: cleaner chords, the melody from the voice and the parts as MIDI. Takes a few minutes; the first time it downloads 166 MB of models.'
                : 'Beat, chords and sections in a few seconds, with a rough guess at the melody.'}
            </p>
            <div className="actions">
              <button className="btn btn-primary" onClick={() => start(stems)}>
                Make the song
              </button>
              <button className="btn btn-ghost" onClick={cancel}>
                Cancel
              </button>
            </div>
          </>
        )}
        {phase.type === 'working' && (
          <>
            <div className="status-line" role="status">
              <span>
                {STEP_LABELS[phase.step] ?? 'Listening'}… {pct}%
              </span>
              <span className="progress" aria-hidden>
                <span style={{ width: `${pct}%` }} />
              </span>
            </div>
            <div className="actions">
              <button className="btn btn-ghost" onClick={cancel}>
                Cancel
              </button>
            </div>
          </>
        )}
        {phase.type === 'error' && (
          <>
            <p className="status-line error" role="alert">
              {phase.message}
            </p>
            <div className="actions">
              <button className="btn btn-ghost" onClick={onClose}>
                Close
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
