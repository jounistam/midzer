import { useEffect, useState } from 'react';

/**
 * A height for the instrument under a stage screen that grows with the
 * window: a third of it, within `min` and `max`, so a big screen gets a kit
 * readable from the throne.
 */
export function useStageHeight(min = 170, max = 380, share = 0.32): number {
  const pick = () => Math.round(Math.min(max, Math.max(min, window.innerHeight * share)));
  const [height, setHeight] = useState(pick);
  useEffect(() => {
    const resize = () => setHeight(pick());
    window.addEventListener('resize', resize);
    return () => window.removeEventListener('resize', resize);
  }, [min, max, share]);
  return height;
}
