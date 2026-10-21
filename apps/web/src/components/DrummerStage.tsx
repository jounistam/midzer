import { useEffect, useMemo, useRef } from 'react';
import { pickProgression, progressionBars } from '../drums/progressions';
import { countInLabel, nextChord, stageSection, timingVerdict } from '../drums/stage';
import { live } from '../live';
import { useSession } from '../session';
import { resolvedBeats } from '../songs/song';

/**
 * The drummer's Jam display. Hands and feet are busy, so it is read from
 * about two metres away rather than clicked: the chord and the next change,
 * the section and where a fill takes it, the tempo, the beat and whether the
 * hits rush or drag.
 */
export function DrummerStage() {
  const playing = useSession((s) => s.playing);
  const chord = useSession((s) => s.chord);
  const songBar = useSession((s) => s.songBar);
  const beat = useSession((s) => s.beat);
  const bar = useSession((s) => s.bar);
  const bpm = useSession((s) => s.bpm);
  const band = useSession((s) => s.band);
  const progression = useSession((s) => pickProgression(s.style, s.progression));
  const beats = useMemo(() => resolvedBeats(progressionBars(progression)), [progression]);

  const at = playing && songBar !== null ? songBar * 4 + beat : 0;
  const now = playing ? (chord?.name ?? '—') : (beats[0] ?? '—');
  const next = nextChord(beats, at, true);
  const section = stageSection(band);

  return (
    <section className="panel drum-stage" aria-live="polite">
      <div className="stage-cell stage-chord">
        <p className="eyebrow">Chord</p>
        <h2 className={`stage-big ${now === '—' ? 'empty' : ''}`}>{now}</h2>
        <p className="stage-sub">
          {next ? (
            <>
              Next <strong>{next.name}</strong>
              {playing && ` in ${next.in}`}
            </>
          ) : (
            'One chord'
          )}
        </p>
      </div>
      <div className="stage-cell stage-section">
        <p className="eyebrow">Section</p>
        <p className={`stage-big ${band.cued ? 'cued' : ''}`} data-testid="stage-section">
          {section.now}
          {section.next && <span className="stage-next"> → {section.next}</span>}
        </p>
        <p className="stage-sub">{playing ? section.hint || ' ' : progression.name}</p>
      </div>
      <div className="stage-cell stage-tempo">
        <p className="eyebrow">Tempo</p>
        <p className="stage-big">{bpm}</p>
        <p className="stage-sub">{playing ? 'BPM · following you' : 'BPM'}</p>
      </div>
      {playing ? (
        <>
          <div className="stage-beats" aria-label={`Bar ${bar + 1}, beat ${beat + 1}`}>
            {[0, 1, 2, 3].map((b) => (
              <span key={b} className={`stage-beat ${b === beat ? 'active' : ''} ${b === 0 ? 'one' : ''}`} />
            ))}
          </div>
          <TimingBar />
        </>
      ) : (
        <CountIn />
      )}
    </section>
  );
}

/** Four dots that fill with each even count-in hit. */
function CountIn() {
  const countIn = useSession((s) => s.countIn);
  const hits = useSession((s) => s.countInHits);
  const ready = useSession((s) => s.audio === 'ready');
  return (
    <div className="stage-count-in" role="status">
      <p className="stage-sub">{!ready ? 'Getting the band ready…' : hits === 4 ? 'Here we go!' : countInLabel(countIn)}</p>
      {countIn !== 'off' && (
        <div className="stage-beats count" aria-label={`${hits} of 4 count-in hits`}>
          {[0, 1, 2, 3].map((i) => (
            <span key={i} className={`stage-beat ${i < hits ? 'active' : ''}`} />
          ))}
        </div>
      )}
    </div>
  );
}

/** Scale of the meter on each side of the band's grid, in ms. */
const METER_MS = 40;

/** Rushing or dragging against the band's grid, smoothed over the last hits. */
export function TimingBar() {
  const needle = useRef<HTMLDivElement>(null);
  const label = useRef<HTMLParagraphElement>(null);
  useEffect(() => {
    let frame = 0;
    let shown = -1;
    const draw = () => {
      const { average, hits } = live.timing;
      if (hits !== shown) {
        shown = hits;
        const x = Math.max(-1, Math.min(1, average / METER_MS));
        if (needle.current) {
          needle.current.style.left = `${50 + x * 50}%`;
          needle.current.classList.toggle('off', hits < 4 || Math.abs(average) >= 8);
        }
        if (label.current) label.current.textContent = timingVerdict(average, hits);
      }
      frame = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(frame);
  }, []);
  return (
    <div className="timing">
      <div className="timing-track" role="meter" aria-label="Timing against the band">
        <span className="timing-end">Rushing</span>
        <div className="timing-pocket" />
        <div className="timing-needle" ref={needle} />
        <span className="timing-end right">Dragging</span>
      </div>
      <p className="timing-label" ref={label} data-testid="timing">
        Listening…
      </p>
    </div>
  );
}
