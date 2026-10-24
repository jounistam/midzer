import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react';
import type { LoopRange } from '../session';
import { formatBar, parseChart, SECTION_LABELS, type Bar, type Song } from '../songs/song';

/** Selected bars `start..end`. */
export type Selection = LoopRange;

interface ChartViewProps {
  song: Song;
  editing: boolean;
  selection: Selection | null;
  /** `extend` grows the selection (shift-click) instead of starting over. */
  onSelect: (bar: number, extend: boolean) => void;
  onEditBar: (index: number, bar: Bar) => void;
  /** The bar being heard, and its beat. */
  currentBar: number | null;
  beat: number;
  startBar: number;
  loop: LoopRange | null;
}

/** The chord chart as a grid of bars; each section starts a new row. */
export function ChartView(props: ChartViewProps) {
  const { song, currentBar } = props;
  const current = useRef<HTMLDivElement>(null);

  // Keep the bar being heard in view.
  useEffect(() => {
    current.current?.scrollIntoView?.({ block: 'nearest', behavior: 'smooth' });
  }, [currentBar]);

  return (
    <div className={`chart ${props.editing ? 'editing' : ''}`} role="list" aria-label="Chord chart">
      {song.bars.map((bar, i) => (
        <div key={i} ref={i === currentBar ? current : undefined} role="listitem" className={cellClass(i, bar, props)}>
          {bar.section && <span className={`section-tag ${bar.section}`}>{SECTION_LABELS[bar.section]}</span>}
          {props.editing ? (
            <BarEditor index={i} bar={bar} onSelect={props.onSelect} onEdit={props.onEditBar} />
          ) : (
            <BarButton index={i} bar={bar} onSelect={props.onSelect} />
          )}
          {i === currentBar && (
            <span className="bar-progress" aria-hidden>
              <span style={{ width: `${((props.beat + 1) / bar.beats.length) * 100}%` }} />
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

function cellClass(i: number, bar: Bar, p: ChartViewProps): string {
  const inRange = (r: LoopRange | null) => r !== null && i >= r.start && i < r.end;
  return [
    'bar-cell',
    bar.section ? 'section-start' : '',
    i === p.currentBar ? 'current' : '',
    inRange(p.selection) ? 'selected' : '',
    inRange(p.loop) ? 'looped' : '',
    p.currentBar === null && i === p.startBar ? 'start' : '',
  ]
    .filter(Boolean)
    .join(' ');
}

function BarButton({ index, bar, onSelect }: { index: number; bar: Bar; onSelect: ChartViewProps['onSelect'] }) {
  const chords = bar.beats.filter(Boolean).join(' ') || 'same chord';
  return (
    <button
      className="bar-body"
      onClick={(e: MouseEvent) => onSelect(index, e.shiftKey)}
      aria-label={`Bar ${index + 1}${bar.beats.length !== 4 ? ` (${bar.beats.length} beats)` : ''}: ${chords}${bar.fill ? ', fill' : ''}`}
    >
      <span className="bar-number">
        {index + 1}
        {bar.beats.length !== 4 && <span className="fill-mark"> · {bar.beats.length}/4</span>}
        {bar.fill && <span className="fill-mark"> · fill</span>}
      </span>
      <span className={`bar-beats${bar.beats.length !== 4 ? ` beats-${bar.beats.length}` : ''}`}>
        {bar.beats.map((chord, b) => (
          <span key={b} className={chord ? 'chord' : 'hold'}>
            {chord || (b === 0 ? '%' : '')}
          </span>
        ))}
      </span>
      {bar.lyric && <span className="bar-lyric">{bar.lyric}</span>}
    </button>
  );
}

/** One bar as editable chart notation, e.g. "Am . F G7". Applied on Enter or leaving the field; it keeps its length unless a tag like "[3/4]" gives another. */
function BarEditor({
  index,
  bar,
  onSelect,
  onEdit,
}: {
  index: number;
  bar: Bar;
  onSelect: ChartViewProps['onSelect'];
  onEdit: ChartViewProps['onEditBar'];
}) {
  const text = formatBar(bar);
  const [draft, setDraft] = useState(text);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setDraft(text), [text]);

  const commit = () => {
    if (draft.trim() === text) return setError(null);
    const { bars, unknown } = parseChart(`[${bar.beats.length}/4] ${draft.replace(/\|/g, ' ')}`);
    if (unknown.length > 0 || bars.length !== 1) {
      setError(unknown.length ? `Not a chord: ${unknown.join(', ')}` : 'Type the chords of one bar, e.g. "Am F"');
      return;
    }
    setError(null);
    // The text may not change (a new length, same chords): show it as written now.
    setDraft(formatBar(bars[0]!));
    onEdit(index, { ...bar, beats: bars[0]!.beats });
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') commit();
    if (e.key === 'Escape') {
      setDraft(text);
      setError(null);
    }
  };

  return (
    <div className="bar-body">
      <button className="bar-number" onClick={(e) => onSelect(index, e.shiftKey)} aria-label={`Select bar ${index + 1}`}>
        {index + 1}
        {bar.beats.length !== 4 && <span className="fill-mark"> · {bar.beats.length}/4</span>}
        {bar.fill && <span className="fill-mark"> · fill</span>}
      </button>
      <input
        className={`bar-input ${error ? 'invalid' : ''}`}
        value={draft}
        aria-label={`Chords in bar ${index + 1}`}
        aria-invalid={error !== null}
        title={error ?? 'A chord for the bar, two for its halves, or one a beat; "." keeps the chord for a beat'}
        spellCheck={false}
        onFocus={() => onSelect(index, false)}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={onKey}
      />
    </div>
  );
}
