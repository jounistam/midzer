import { chordTones } from '@midzer/audio-web';
import { useState } from 'react';
import { keyNames, voicing, type Voicing } from '../playalong/levels';
import { parseChord } from '../songs/chords';

/** The keys drawn: C2 up to C6, both hands' reach at every level. */
const LOW = 36;
const HIGH = 84;
const BLACK = new Set([1, 3, 6, 8, 10]);
const WHITE_W = 12;
const WHITE_H = 52;
const BLACK_W = 7;
const BLACK_H = 32;

/**
 * Every chord a song uses, as buttons; picking one shows the whole chord on
 * a small keyboard: root and fifth with the left hand, every chord tone
 * with the right (as at the top level of playing along).
 */
export function ChordSheet({ chords, title = 'Chords in this song' }: { chords: readonly string[]; title?: string }) {
  const [picked, setPicked] = useState<string | null>(null);
  if (chords.length === 0) return null;
  const shown = picked !== null && chords.includes(picked) ? picked : null;
  const spec = shown ? parseChord(shown) : null;
  const keys = spec ? voicing(spec, 5) : null;
  return (
    <section className="panel chord-sheet" data-testid="chord-sheet">
      <div className="chord-sheet-head">
        <p className="eyebrow">{title}</p>
        <p className="chord-sheet-hint">{shown ? '' : 'Pick a chord to see its keys'}</p>
      </div>
      <div className="chord-chips" role="group" aria-label={title}>
        {chords.map((c) => (
          <button key={c} className={`chord-chip ${c === shown ? 'on' : ''}`} aria-pressed={c === shown} onClick={() => setPicked(c === shown ? null : c)}>
            {c}
          </button>
        ))}
      </div>
      {spec && keys && (
        <div className="chord-shape">
          <MiniKeys keys={keys} label={`${shown} on the keys`} />
          <p className="chord-shape-notes">
            <span>
              <strong>{shown}</strong>: {chordTones(spec).join(' · ')}
            </span>
            {keys.left.length > 0 && (
              <span className="hand left">
                Left hand <strong>{keyNames(keys.left)}</strong>
              </span>
            )}
            <span className="hand right">
              Right hand <strong>{keyNames(keys.right)}</strong>
            </span>
          </p>
        </div>
      )}
    </section>
  );
}

/** A small drawn keyboard with each hand's keys lit. */
function MiniKeys({ keys, label }: { keys: Voicing; label: string }) {
  const left = new Set(keys.left);
  const right = new Set(keys.right);
  const hand = (n: number) => (left.has(n) ? 'left' : right.has(n) ? 'right' : '');
  const whites: number[] = [];
  const blacks: { note: number; x: number }[] = [];
  for (let n = LOW; n <= HIGH; n++) {
    if (BLACK.has(n % 12)) blacks.push({ note: n, x: whites.length * WHITE_W - BLACK_W / 2 });
    else whites.push(n);
  }
  const width = whites.length * WHITE_W;
  return (
    <svg className="mini-keys" viewBox={`0 0 ${width} ${WHITE_H}`} role="img" aria-label={label}>
      {whites.map((n, i) => (
        <rect key={n} className={`white ${hand(n)}`} x={i * WHITE_W + 0.5} y={0.5} width={WHITE_W - 1} height={WHITE_H - 1} rx={2} />
      ))}
      {blacks.map(({ note, x }) => (
        <rect key={note} className={`black ${hand(note)}`} x={x} y={0} width={BLACK_W} height={BLACK_H} rx={1.5} />
      ))}
      {/* Middle C, to find the place. */}
      <circle className="middle-c" cx={whites.indexOf(60) * WHITE_W + WHITE_W / 2} cy={WHITE_H - 6} r={1.6} />
    </svg>
  );
}
