import { useEffect, useRef, useState } from 'react';
import { PIECE_INFO, PIECES } from '../drums/kit';
import { clearPiece, currentPiece, hear, isDone, move, notesOf, startLearning } from '../drums/learn';
import { onDrumInput, setKit } from '../session';

/**
 * Learns which notes the player's e-kit sends: hit each piece in turn. Every
 * note heard during a piece's turn plays that piece from then on.
 */
export function KitLearn({ onClose }: { onClose: () => void }) {
  const [learning, setLearning] = useState(startLearning);
  const ref = useRef<HTMLDivElement>(null);
  const piece = currentPiece(learning);

  useEffect(() => ref.current?.focus(), []);
  useEffect(() => onDrumInput((note) => setLearning((l) => hear(l, note))), []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const learned = PIECES.filter((p) => notesOf(learning.map, p).length > 0);
  const notes = piece ? notesOf(learning.map, piece) : [];

  return (
    <div className="drawer-backdrop centered" onClick={onClose}>
      <div className="dialog kit-learn" role="dialog" aria-label="Learn my kit" tabIndex={-1} ref={ref} onClick={(e) => e.stopPropagation()}>
        <h2>Learn my kit</h2>
        {!isDone(learning) && piece ? (
          <>
            <p className="setting-note">
              {learning.step + 1} of {PIECES.length} · Hit every zone you play (head, rim, edge), then Next. Skip what your kit
              doesn't have.
            </p>
            <p className="kit-learn-piece">Hit the {PIECE_INFO[piece].label.toLowerCase()}</p>
            <p className="kit-learn-notes" aria-live="polite">
              {notes.length ? `Heard note ${notes.join(', ')}` : 'Waiting for a hit…'}
            </p>
            <div className="actions">
              <button className="btn btn-primary btn-xl" onClick={() => setLearning((l) => move(l, 1))}>
                {notes.length ? 'Next' : 'Skip'}
              </button>
              <button className="btn btn-secondary" onClick={() => setLearning((l) => move(l, -1))} disabled={learning.step === 0}>
                Back
              </button>
              {notes.length > 0 && (
                <button className="btn btn-ghost" onClick={() => setLearning(clearPiece)}>
                  Clear
                </button>
              )}
              <button className="btn btn-ghost" onClick={onClose}>
                Cancel
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="lead">
              {learned.length
                ? `Learned ${learned.length} of ${PIECES.length} pieces: ${learned.map((p) => PIECE_INFO[p].label).join(', ')}.`
                : 'No hits were heard. Is the kit plugged in?'}
            </p>
            <div className="actions">
              {learned.length > 0 && (
                <button
                  className="btn btn-primary btn-xl"
                  onClick={() => {
                    setKit({ preset: 'learned', map: learning.map });
                    onClose();
                  }}
                >
                  Save
                </button>
              )}
              <button className="btn btn-secondary" onClick={() => setLearning(startLearning())}>
                Start over
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
