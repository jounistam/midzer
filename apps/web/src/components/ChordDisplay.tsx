import { chordTones, NOTE_NAMES } from '@midzer/audio-web';
import { useSession } from '../session';
import { sectionLine } from './bandStatus';

/** The big "Now playing" chord with beat dots, as in the Jam design. */
export function ChordDisplay({ hint = 'Hold a chord to start the band' }: { hint?: string }) {
  const chord = useSession((s) => s.chord);
  const playing = useSession((s) => s.playing);
  const bar = useSession((s) => s.bar);
  const beat = useSession((s) => s.beat);
  const band = useSession((s) => s.band);
  const resting = playing && band.section === 'rest';

  return (
    <section className="chord-stage" aria-live="polite">
      <div className="chord-rings" aria-hidden>
        {/* Re-keyed on every beat so the pulse animation restarts. */}
        <div key={playing ? `${bar}.${beat}` : 'idle'} className={`ring ring-pulse ${playing ? 'on' : ''} ${beat === 0 ? 'downbeat' : ''}`} />
        <div className="ring ring-mid" />
        <div className="ring ring-outer" />
      </div>
      <p className="eyebrow">{resting ? 'Play to bring the band back' : chord ? 'Now playing' : playing ? 'Band is grooving' : 'Ready'}</p>
      <h2 className={`chord-name ${chord ? '' : 'empty'}`}>{chord?.name ?? '—'}</h2>
      <p className="chord-tones">
        {chord ? (
          <>
            {chordTones(chord).join(' · ')}
            <span className="chord-bass">bass {NOTE_NAMES[chord.bass]}</span>
          </>
        ) : (
          hint
        )}
      </p>
      <div className="beats" aria-label={playing ? `Bar ${bar + 1}, beat ${beat + 1}` : 'Stopped'}>
        {[0, 1, 2, 3].map((b) => (
          <span key={b} className={`beat-dot ${playing && b === beat ? 'active' : ''}`} />
        ))}
        <span className="bar-label">{playing ? `Bar ${bar + 1}` : 'Stopped'}</span>
      </div>
      {playing && <p className={`section-line ${band.section}`}>{sectionLine(band)}</p>}
    </section>
  );
}
