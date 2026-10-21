import { chordTones } from '@midzer/audio-web';
import { useEffect, useState } from 'react';
import { BandMutes, PlayButton } from '../components/ControlPanel';
import { Keyboard } from '../components/Keyboard';
import { NoteLanes } from '../components/NoteLanes';
import { Stars } from '../components/Stars';
import { TopBar } from '../components/TopBar';
import { accuracy } from '../learn/judge';
import {
  attachPractice,
  closePractice,
  dismissResult,
  openLesson,
  setAnyOctave,
  setPart,
  setPracticeLoop,
  setSpeed,
  setWait,
  targetName,
  usePractice,
} from '../learn/practice';
import { PART_LABELS, partsOf, type Target } from '../learn/targets';
import { useSession } from '../session';
import { parseChord } from '../songs/chords';
import { LoadingStatus } from './LoadingStatus';

const SPEEDS = [0.5, 0.7, 0.85, 1];
/** The lanes and keyboard show the same keys. */
const LOW = 36;
const HIGH = 96;

export function Lesson() {
  const song = usePractice((s) => s.song);
  const lesson = usePractice((s) => s.lesson);
  const part = usePractice((s) => s.part);
  const split = useSession((s) => s.split);
  const result = usePractice((s) => s.result);

  useEffect(() => attachPractice(), []);
  if (!song) return null;

  return (
    <div className="lesson-screen">
      <TopBar title="Learn" onBack={closePractice} />
      <main className="lesson-main">
        <div className="lesson-stage">
          <section className="panel lesson-head">
            <div>
              <p className="eyebrow">{lesson ? lesson.unit : 'Practise a song'} · {PART_LABELS[part]}</p>
              <h1 className="song-title">{lesson?.title ?? song.title}</h1>
              <p className="lesson-tip">{lesson?.tip ?? 'Play what falls onto the keys. Wait mode holds the band until you do.'}</p>
            </div>
            <ScoreStrip />
          </section>
          <TargetPanel />
          <LoadingStatus />
          <div className="panel lesson-play">
            <NoteLanes low={LOW} high={HIGH} />
            <Keyboard low={LOW} high={HIGH} split={split} height={150} />
          </div>
        </div>
        <aside className="panel control-panel">
          <PlayButton className="full" />
          <PracticeOptions />
          <LoopPicker bars={song.bars.length} />
          <BandMutes />
        </aside>
      </main>
      {result && <ResultDialog />}
    </div>
  );
}

/** What to play now and next, the band's waiting and the last grade. */
function TargetPanel() {
  const now = usePractice((s) => s.now);
  const next = usePractice((s) => s.next);
  const holding = usePractice((s) => s.holding);
  const playing = useSession((s) => s.playing);
  const wait = usePractice((s) => s.wait);
  const empty = now.length === 0;

  return (
    <section className={`panel target-panel ${holding ? 'holding' : ''}`} aria-live="polite">
      <div>
        <p className="eyebrow">{holding ? 'The band waits for you' : playing ? 'Play' : 'First up'}</p>
        <h2 className={`now-chord ${empty ? 'empty' : ''}`}>{empty ? 'Done' : now.map(targetName).join(' + ')}</h2>
        <p className="target-tones">{now.map(tonesOf).filter(Boolean).join(' · ') || (empty ? 'Nothing left to play' : ' ')}</p>
      </div>
      <div className="next-chord">
        <p className="eyebrow">Next</p>
        <p className="next-name">{next.length ? next.map(targetName).join(' + ') : '—'}</p>
        <p className="next-when">{wait ? 'Wait mode: the band holds on each one' : 'The band keeps time'}</p>
      </div>
      <FeedbackChip />
    </section>
  );
}

function tonesOf(t: Target): string {
  if (t.kind !== 'chord' || !t.name) return '';
  const spec = parseChord(t.name);
  return spec ? `${t.name}: ${chordTones(spec).join(' ')}` : '';
}

const GRADE_TEXT = { perfect: 'Perfect', good: 'Good', ok: 'OK', miss: 'Missed' } as const;

function FeedbackChip() {
  const feedback = usePractice((s) => s.feedback);
  if (!feedback) return <span className="feedback" aria-hidden />;
  const { grade, offsetMs } = feedback;
  const timing = offsetMs === null || grade === 'perfect' || grade === 'miss' ? '' : offsetMs < 0 ? ' · early' : ' · late';
  return (
    <span key={feedback.seq} className={`feedback ${grade}`}>
      {GRADE_TEXT[grade]}
      {timing}
    </span>
  );
}

function ScoreStrip() {
  const score = usePractice((s) => s.score);
  const total = usePractice((s) => s.targetCount);
  const played = score.perfect + score.good + score.ok;
  const graded = played + score.miss;
  return (
    <dl className="score-strip">
      <div>
        <dt className="label">Streak</dt>
        <dd>{score.streak}</dd>
      </div>
      <div>
        <dt className="label">Accuracy</dt>
        <dd>{graded || score.wrong ? `${Math.round(accuracy(score) * 100)}%` : '—'}</dd>
      </div>
      <div>
        <dt className="label">Played</dt>
        <dd>
          {played}
          <small>/{total}</small>
        </dd>
      </div>
    </dl>
  );
}

function PracticeOptions() {
  const song = usePractice((s) => s.song);
  const lesson = usePractice((s) => s.lesson);
  const part = usePractice((s) => s.part);
  const wait = usePractice((s) => s.wait);
  const speed = usePractice((s) => s.speed);
  const anyOctave = usePractice((s) => s.anyOctave);
  const bpm = useSession((s) => s.bpm);
  if (!song) return null;
  const parts = partsOf(song);
  return (
    <>
      <div className="control-group">
        <div className="control-head">
          <p className="label">Wait for me</p>
          <button
            className={`toggle ${wait ? 'on' : ''}`}
            aria-pressed={wait}
            onClick={() => setWait(!wait)}
            title="The band stops at each note or chord until you play it"
          >
            {wait ? 'On' : 'Off'}
          </button>
        </div>
        {part !== 'chords' && (
          <div className="control-head">
            <p className="label">Melody in any octave</p>
            <button className={`toggle ${anyOctave ? 'on' : ''}`} aria-pressed={anyOctave} onClick={() => setAnyOctave(!anyOctave)}>
              {anyOctave ? 'On' : 'Off'}
            </button>
          </div>
        )}
      </div>
      <div className="control-group">
        <p className="label" id="speed-label">
          Speed · {bpm} BPM
        </p>
        <div className="segmented" role="group" aria-labelledby="speed-label">
          {SPEEDS.map((s) => (
            <button key={s} className={`seg ${Math.abs(s - speed) < 1e-6 ? 'on' : ''}`} aria-pressed={Math.abs(s - speed) < 1e-6} onClick={() => setSpeed(s)}>
              {Math.round(s * 100)}%
            </button>
          ))}
        </div>
      </div>
      {!lesson && parts.length > 1 && (
        <div className="control-group">
          <p className="label" id="part-label">
            Play
          </p>
          <div className="segmented" role="group" aria-labelledby="part-label">
            {parts.map((p) => (
              <button key={p} className={`seg ${p === part ? 'on' : ''}`} aria-pressed={p === part} onClick={() => setPart(p)}>
                {PART_LABELS[p]}
              </button>
            ))}
          </div>
        </div>
      )}
    </>
  );
}

function LoopPicker({ bars }: { bars: number }) {
  const loop = useSession((s) => s.loop);
  const [range, setRange] = useState({ start: 1, end: Math.min(bars, 4) });
  const clamp = (n: number) => Math.min(bars, Math.max(1, Math.round(n) || 1));
  const apply = () => {
    const start = Math.min(range.start, range.end);
    const end = Math.max(range.start, range.end);
    setPracticeLoop({ start: start - 1, end });
  };
  return (
    <div className="control-group">
      <p className="label">Loop</p>
      <div className="loop-picker">
        <label>
          <span className="sr-only">From bar</span>
          <input type="number" min={1} max={bars} value={range.start} onChange={(e) => setRange({ ...range, start: clamp(Number(e.target.value)) })} />
        </label>
        <span aria-hidden>–</span>
        <label>
          <span className="sr-only">To bar</span>
          <input type="number" min={1} max={bars} value={range.end} onChange={(e) => setRange({ ...range, end: clamp(Number(e.target.value)) })} />
        </label>
        <button
          className={`btn btn-secondary loop-button ${loop ? 'on' : ''}`}
          aria-pressed={loop !== null}
          onClick={() => (loop ? setPracticeLoop(null) : apply())}
        >
          {loop ? `Looping ${loop.start + 1}–${loop.end}` : 'Loop bars'}
        </button>
      </div>
      <p className="setting-note">{loop ? 'Runs in a loop are not scored for stars.' : 'Repeat a hard part until it sits.'}</p>
    </div>
  );
}

function ResultDialog() {
  const result = usePractice((s) => s.result)!;
  const lesson = usePractice((s) => s.lesson);
  const { score, stars, previousStars, next } = result;
  const heading = stars === 3 ? 'Brilliant!' : stars === 2 ? 'Nicely done' : stars === 1 ? 'You made it through' : 'Keep at it';
  return (
    <div className="drawer-backdrop centered" onClick={dismissResult}>
      <div className="dialog result-dialog" role="dialog" aria-label="Result" onClick={(e) => e.stopPropagation()}>
        <Stars count={stars} />
        <h2>{heading}</h2>
        <p className="result-accuracy">{Math.round(result.accuracy * 100)}% accuracy</p>
        {stars > previousStars && previousStars > 0 && <p className="notice">A new best for this {lesson ? 'lesson' : 'song'}.</p>}
        <dl className="result-counts">
          <div>
            <dt>Perfect</dt>
            <dd>{score.perfect}</dd>
          </div>
          <div>
            <dt>Good</dt>
            <dd>{score.good}</dd>
          </div>
          <div>
            <dt>OK</dt>
            <dd>{score.ok}</dd>
          </div>
          <div>
            <dt>Missed</dt>
            <dd>{score.miss}</dd>
          </div>
          <div>
            <dt>Wrong notes</dt>
            <dd>{score.wrong}</dd>
          </div>
          <div>
            <dt>Best streak</dt>
            <dd>{score.bestStreak}</dd>
          </div>
        </dl>
        {stars === 0 && <p className="setting-note">Try a slower speed or wait mode: a star opens the next lesson.</p>}
        <div className="actions">
          {next && stars > 0 && (
            <button className="btn btn-primary" onClick={() => openLesson(next)}>
              Next: {next.title}
            </button>
          )}
          <button className={`btn ${next && stars > 0 ? 'btn-secondary' : 'btn-primary'}`} onClick={dismissResult}>
            Play again
          </button>
          <button className="btn btn-ghost" onClick={closePractice}>
            {lesson ? 'All lessons' : 'Back to Learn'}
          </button>
        </div>
      </div>
    </div>
  );
}
