import { useMemo } from 'react';
import { describeOffset, HISTOGRAM_RANGE_MS, HISTOGRAM_STEP_MS, ON_TIME_MS, summarize } from '../drums/judging';
import { judgedHits, useSession } from '../session';

const pct = (x: number) => `${Math.round(x * 100)}%`;

/** How the last run against the recording went: overall, by offset, bar by bar and by part of the kit. */
export function TimingReport() {
  const count = useSession((s) => s.judgedCount);
  // judgedHits() grows in place; the count says when to look again.
  const summary = useMemo(() => summarize(judgedHits()), [count]);
  if (summary.hits === 0) return <p className="setting-note">Press Play and drum along: every hit is timed against the recording's own beat.</p>;

  return (
    <div className="timing-report" data-testid="timing-report">
      <div className="report-tiles">
        <Tile label="On time" value={pct(summary.onTime)} note={`within ${ON_TIME_MS} ms, ${summary.hits} hits`} />
        <Tile label="On average" value={describeOffset(summary.meanMs)} />
        <Tile label="Spread" value={`±${Math.round(summary.spreadMs)} ms`} />
      </div>
      <Histogram bins={summary.histogram} />
      {summary.bars.length > 1 && <BarAccuracy bars={summary.bars} />}
      <table className="report-parts">
        <thead>
          <tr>
            <th scope="col">Part</th>
            <th scope="col">Hits</th>
            <th scope="col">On time</th>
            <th scope="col">Average</th>
            <th scope="col">Tends to be</th>
          </tr>
        </thead>
        <tbody>
          {summary.parts.map((p) => (
            <tr key={p.id}>
              <th scope="row">{p.label}</th>
              <td>{p.hits}</td>
              <td>{pct(p.onTime)}</td>
              <td>{describeOffset(p.meanMs)}</td>
              <td>{p.lean ?? '—'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Tile({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="report-tile">
      <p className="label">{label}</p>
      <p className="report-value">{value}</p>
      {note && <p className="setting-note">{note}</p>}
    </div>
  );
}

const W = 400;
const H = 96;

/** Hits by how far off they were; the bins inside the on-time window are lit. */
function Histogram({ bins }: { bins: number[] }) {
  const max = Math.max(1, ...bins);
  const w = W / bins.length;
  return (
    <figure className="report-chart">
      <figcaption className="label">Early ← offset → late</figcaption>
      <svg viewBox={`0 0 ${W} ${H + 16}`} role="img" aria-label="Hits by offset from the beat">
        <line className="chart-axis" x1={0} x2={W} y1={H} y2={H} />
        <line className="chart-zero" x1={W / 2} x2={W / 2} y1={0} y2={H} />
        {bins.map((n, i) => {
          const from = -HISTOGRAM_RANGE_MS + i * HISTOGRAM_STEP_MS;
          const inside = from >= -ON_TIME_MS && from + HISTOGRAM_STEP_MS <= ON_TIME_MS;
          const h = (n / max) * (H - 4);
          const range =
            i === 0 ? `${HISTOGRAM_RANGE_MS - HISTOGRAM_STEP_MS}+ ms early` : i === bins.length - 1 ? `${from}+ ms late` : `${from} to ${from + HISTOGRAM_STEP_MS} ms`;
          return (
            <g key={i} className="chart-hit">
              <rect className="chart-target" x={i * w} y={0} width={w} height={H} />
              {n > 0 && <rect className={`chart-bar ${inside ? 'on' : ''}`} x={i * w + 1} y={H - h} width={w - 2} height={h} rx={2} />}
              <title>{`${range}: ${n} ${n === 1 ? 'hit' : 'hits'}`}</title>
            </g>
          );
        })}
        <text className="chart-tick" x={2} y={H + 13}>
          −{HISTOGRAM_RANGE_MS} ms
        </text>
        <text className="chart-tick" x={W / 2} y={H + 13} textAnchor="middle">
          0
        </text>
        <text className="chart-tick" x={W - 2} y={H + 13} textAnchor="end">
          +{HISTOGRAM_RANGE_MS} ms
        </text>
      </svg>
    </figure>
  );
}

/** The share of hits on time in each bar played, in order. */
function BarAccuracy({ bars }: { bars: { bar: number; hits: number; onTime: number }[] }) {
  const left = 32;
  const w = Math.min(24, (W - left) / bars.length);
  const top = H - 4;
  const [first, last] = [bars[0]!.bar + 1, bars[bars.length - 1]!.bar + 1];
  return (
    <figure className="report-chart">
      <figcaption className="label">On time, bar by bar</figcaption>
      <svg viewBox={`0 0 ${W} ${H + 16}`} role="img" aria-label="Share of hits on time in each bar">
        {[1, 0.5].map((share) => (
          <g key={share}>
            <line className="chart-grid" x1={left} x2={W} y1={H - share * top} y2={H - share * top} />
            <text className="chart-tick" x={left - 4} y={H - share * top + 3} textAnchor="end">
              {pct(share)}
            </text>
          </g>
        ))}
        <line className="chart-axis" x1={left} x2={W} y1={H} y2={H} />
        {bars.map((b, i) => {
          const h = Math.max(2, b.onTime * top);
          const x = left + i * w;
          return (
            <g key={i} className="chart-hit">
              <rect className="chart-target" x={x} y={0} width={w} height={H} />
              <rect className="chart-bar on" x={x + Math.min(1, w / 4)} y={H - h} width={Math.max(1, w - Math.min(2, w / 2))} height={h} rx={Math.min(2, w / 4)} />
              <title>{`Bar ${b.bar + 1}: ${pct(b.onTime)} of ${b.hits} on time`}</title>
            </g>
          );
        })}
        {bars.length * w < 120 ? (
          <text className="chart-tick" x={left} y={H + 13}>
            Bars {first}–{last}
          </text>
        ) : (
          <>
            <text className="chart-tick" x={left} y={H + 13}>
              Bar {first}
            </text>
            <text className="chart-tick" x={left + bars.length * w} y={H + 13} textAnchor="end">
              Bar {last}
            </text>
          </>
        )}
      </svg>
    </figure>
  );
}
