import { useEffect, useRef, useState } from 'react';
import { OFF_MS, ON_TIME_MS } from '../drums/judging';
import { timingVerdict } from '../drums/stage';
import { live } from '../live';
import { gradeOf, type Grade } from '../playalong/feedback';

/** The lane shows hits this far either side of the beat; further ones sit at its ends. */
const LANE_MS = 60;
/** Hits shown on the lane, newest brightest. */
const SHOWN = 8;
/** The share on time over this many recent hits. */
const RECENT = 16;

interface View {
  count: number;
  onTime: number | null;
  verdict: string;
  hits: { ms: number; grade: Grade }[];
  chords: { checked: number; right: number };
}

function read(): View {
  const recent = live.hits.hits;
  const chords = recent.filter((h) => h.chord !== null);
  return {
    count: live.hits.count,
    onTime: live.hits.onTime(RECENT),
    verdict: timingVerdict(live.timing.average, live.timing.hits),
    hits: recent.slice(-SHOWN).map((h) => ({ ms: h.ms, grade: gradeOf(h.ms, h.chord) })),
    chords: { checked: chords.length, right: chords.filter((h) => h.chord).length },
  };
}

const laneX = (ms: number) => 50 + (Math.max(-LANE_MS, Math.min(LANE_MS, ms)) / LANE_MS) * 50;

/**
 * How the playing goes right now, readable from the drum throne: the share of
 * recent hits on time, rushing or dragging, and every hit on an early/late
 * lane. The panel flashes green, amber or red as each hit is judged.
 */
export function HitFeedback({ against = 'the track', lagging = false }: { against?: string; lagging?: boolean }) {
  const [view, setView] = useState<View>(read);
  const panel = useRef<HTMLElement>(null);
  useEffect(() => {
    let frame = 0;
    let shown = live.hits.count;
    const tick = () => {
      if (live.hits.count !== shown) {
        shown = live.hits.count;
        const next = read();
        setView(next);
        const last = next.hits[next.hits.length - 1];
        const el = panel.current;
        if (el && last) {
          // Restart the flash animation for every hit.
          el.dataset.flash = '';
          void el.offsetWidth;
          el.dataset.flash = last.grade;
        }
      }
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
  }, []);

  const pct = view.onTime === null ? null : Math.round(view.onTime * 100);
  const level = view.onTime === null ? 'none' : view.onTime >= 0.8 ? 'on' : view.onTime >= 0.6 ? 'near' : 'off';
  const needle = view.count >= 4 ? laneX(live.timing.average) : null;

  return (
    <section className="panel hit-feedback" ref={panel} data-level={level} aria-live="off">
      <div className="hit-score">
        <p className="eyebrow">On time</p>
        <p className="hit-percent" data-testid="hit-percent" role="meter" aria-label={`Hits on time against ${against}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? 0}>
          {pct === null ? '—' : `${pct}%`}
        </p>
        <p className="hit-verdict" data-testid="timing">
          {view.count === 0 ? 'Play along: every hit is timed' : view.verdict}
        </p>
        {view.chords.checked > 0 && (
          <p className="stage-sub" data-testid="hit-chords">
            Chords <strong>{view.chords.right}</strong> of {view.chords.checked} right
          </p>
        )}
      </div>
      <div className="hit-lane" role="img" aria-label={`Recent hits against ${against}: early to the left, late to the right`}>
        <span className="hit-lane-end">Early</span>
        <div className="hit-lane-near" style={{ left: `${laneX(-OFF_MS)}%`, width: `${laneX(OFF_MS) - laneX(-OFF_MS)}%` }} />
        <div className="hit-lane-pocket" style={{ left: `${laneX(-ON_TIME_MS)}%`, width: `${laneX(ON_TIME_MS) - laneX(-ON_TIME_MS)}%` }} />
        <div className="hit-lane-zero" />
        {view.hits.map((h, i) => (
          <span
            key={view.count - view.hits.length + i}
            className={`hit-dot ${h.grade} ${i === view.hits.length - 1 ? 'newest' : ''}`}
            style={{ left: `${laneX(h.ms)}%`, opacity: 0.25 + (0.75 * (i + 1)) / view.hits.length }}
          />
        ))}
        {needle !== null && <div className="hit-needle" style={{ left: `${needle}%` }} />}
        <span className="hit-lane-end right">Late</span>
      </div>
      {lagging && <p className="setting-note hit-note">Hits are timed against the beats around them, so each shows up about a beat after you play it.</p>}
    </section>
  );
}
