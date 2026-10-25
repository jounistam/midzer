import { useRef, useState, type KeyboardEvent } from 'react';

const MIN_BPM = 40;
const MAX_BPM = 240;

/**
 * A tempo to type in. Each keystroke that makes a tempo in range applies at
 * once (no shorter tempo is the start of a longer one); leaving the field
 * clamps whatever is left, or puts the tempo back if it isn't a number.
 */
export function BpmInput({ value, onChange, className, label = 'Tempo in BPM' }: { value: number; onChange: (bpm: number) => void; className?: string; label?: string }) {
  const [draft, setDraft] = useState<string | null>(null);
  const cancelled = useRef(false);
  const parse = (text: string) => (text.trim() === '' ? NaN : Math.round(Number(text)));
  const type = (text: string) => {
    setDraft(text);
    const bpm = parse(text);
    if (bpm >= MIN_BPM && bpm <= MAX_BPM && bpm !== value) onChange(bpm);
  };
  const leave = () => {
    const bpm = draft === null || cancelled.current ? NaN : parse(draft);
    cancelled.current = false;
    if (Number.isFinite(bpm)) {
      const clamped = Math.min(MAX_BPM, Math.max(MIN_BPM, bpm));
      if (clamped !== value) onChange(clamped);
    }
    setDraft(null);
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') e.currentTarget.blur();
    if (e.key === 'Escape') {
      cancelled.current = true;
      e.currentTarget.blur();
    }
  };
  return (
    <input
      type="number"
      inputMode="numeric"
      className={className}
      min={MIN_BPM}
      max={MAX_BPM}
      aria-label={label}
      value={draft ?? String(value)}
      onChange={(e) => type(e.target.value)}
      onFocus={(e) => e.currentTarget.select()}
      onBlur={leave}
      onKeyDown={key}
    />
  );
}
