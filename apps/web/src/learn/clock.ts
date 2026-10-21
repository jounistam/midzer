// Where the song is, in beats, at any moment, for falling notes and timing.
//
// The engine reports every song beat and every hold with the time it is heard.
// Those arrive a little early, so each becomes an anchor that takes effect at
// its time; between anchors the position moves on at the tempo.

export interface Anchor {
  /** performance.now() time the anchor is heard. */
  timeMs: number;
  /** Song position in beats. */
  beat: number;
  /** Whether the song moves on from here (false while the band waits). */
  moving: boolean;
}

export class SongClock {
  private anchors: Anchor[] = [];
  private beatMs = 600;

  setTempo(bpm: number): void {
    this.beatMs = 60_000 / bpm;
  }

  get msPerBeat(): number {
    return this.beatMs;
  }

  /** A song beat starts at `timeMs`. */
  onBeat(beat: number, timeMs: number): void {
    this.add({ timeMs, beat, moving: true });
  }

  /** The band stops at, or goes on from, song position `beat`. */
  onHold(holding: boolean, beat: number, timeMs: number): void {
    this.add({ timeMs, beat, moving: !holding });
  }

  reset(): void {
    this.anchors = [];
  }

  /** Song position at `now`, or null before the first beat is heard. */
  position(now: number): number | null {
    const a = this.current(now);
    if (!a) return null;
    if (!a.moving) return a.beat;
    // A beat event comes every beat; never run past the next one.
    return a.beat + Math.min(1, Math.max(0, now - a.timeMs) / this.beatMs);
  }

  /** Whether the band is waiting at `now`. */
  holding(now: number): boolean {
    return this.current(now)?.moving === false;
  }

  private add(anchor: Anchor): void {
    let i = this.anchors.length;
    while (i > 0 && this.anchors[i - 1]!.timeMs > anchor.timeMs) i--;
    this.anchors.splice(i, 0, anchor);
  }

  private current(now: number): Anchor | null {
    let i = -1;
    while (i + 1 < this.anchors.length && this.anchors[i + 1]!.timeMs <= now) i++;
    if (i < 0) return null;
    // Keep only the anchor in force and the ones still to come.
    if (i > 0) this.anchors.splice(0, i);
    return this.anchors[0]!;
  }
}
