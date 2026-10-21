// How the band's state reads in the UI: section pad lights and the status line.

import { STYLES, VARIATION_NAMES, type BandState, type SectionRequest } from '@midzer/audio-web';

type PadState = 'on' | 'queued' | 'off';

/** Whether a section pad is playing now, queued, or neither. */
export function padState(request: SectionRequest, band: BandState): PadState {
  const { section, variation, nextVariation, fillPending, introPending, endingPending, playing } = band;
  const v = VARIATION_NAMES.indexOf(request as 'A');
  if (v >= 0) {
    if (!playing) return v === nextVariation ? 'on' : 'off';
    const current = section === 'main' || section === 'fill' || section === 'rest' ? variation : -1;
    if (v === current && v === nextVariation) return 'on';
    return v === nextVariation ? 'queued' : v === current ? 'on' : 'off';
  }
  if (!playing) return 'off';
  switch (request) {
    case 'intro':
      return section === 'intro' ? 'on' : introPending ? 'queued' : 'off';
    case 'fill':
      return section === 'fill' ? 'on' : fillPending ? 'queued' : 'off';
    case 'ending':
      return section === 'ending' ? 'on' : endingPending ? 'queued' : 'off';
    default:
      return 'off';
  }
}

/** One line on what the band is doing, e.g. "Pop · B → C". */
export function sectionLine(band: BandState): string {
  const style = STYLES[band.style]?.name ?? '';
  const v = VARIATION_NAMES[band.variation] ?? 'A';
  const next = VARIATION_NAMES[band.nextVariation] ?? 'A';
  switch (band.section) {
    case 'intro':
      return `${style} · Intro → ${next}`;
    case 'fill':
      return `${style} · Fill → ${next}`;
    case 'ending':
      return `${style} · Ending`;
    case 'rest':
      return `${style} · Waiting for you`;
    default:
      if (band.endingPending) return `${style} · ${v} → Ending`;
      if (band.cued) return `${style} · ${v} → ${next} on a crash`;
      return band.fillPending ? `${style} · ${v} → ${next === v ? 'Fill' : next}` : `${style} · ${v}`;
  }
}
