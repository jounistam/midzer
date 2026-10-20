import { startAudio, useSession } from '../session';

/** Inline status of the audio engine and sound download; empty when ready. */
export function LoadingStatus() {
  const audio = useSession((s) => s.audio);
  const progress = useSession((s) => s.loadProgress);
  const error = useSession((s) => s.error);

  if (audio === 'ready' || audio === 'idle') return null;
  if (audio === 'error') {
    return (
      <p className="status-line error" role="alert">
        {error ?? 'Something went wrong.'}{' '}
        <button className="btn btn-ghost" onClick={() => void startAudio()}>
          Try again
        </button>
      </p>
    );
  }
  const pct = Math.round(progress * 100);
  return (
    <div className="status-line" role="status">
      <span>{audio === 'starting' ? 'Starting audio…' : `Loading band sounds… ${pct}%`}</span>
      <span className="progress" aria-hidden>
        <span style={{ width: `${pct}%` }} />
      </span>
    </div>
  );
}
