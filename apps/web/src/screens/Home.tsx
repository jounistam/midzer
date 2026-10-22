import { DeviceChip } from '../components/TopBar';
import { goTo, startAudio, useSession } from '../session';
import { LoadingStatus } from './LoadingStatus';

const MODES = [
  { id: 'jam', title: 'Jam', blurb: 'Hold chords, the band follows you.' },
  { id: 'songs', title: 'Songs', blurb: 'Play along with chord charts and MIDI files.' },
  { id: 'learn', title: 'Learn', blurb: 'Falling notes, wait mode and lessons.' },
  { id: 'along', title: 'Play along', blurb: 'Drum or play keys along with any song this computer plays.' },
] as const;

export function Home() {
  const audio = useSession((s) => s.audio);
  const open = (screen: (typeof MODES)[number]['id']) => {
    if (audio === 'idle' || audio === 'error') void startAudio();
    goTo(screen);
  };
  return (
    <main className="home">
      <header className="home-head">
        <span className="brand">midzer</span>
        <DeviceChip />
      </header>
      <h1>What do you want to do?</h1>
      <div className="mode-grid">
        {MODES.map((m) => (
          <button key={m.id} className="mode-card" onClick={() => open(m.id)}>
            <span className="mode-title">{m.title}</span>
            <span className="mode-blurb">{m.blurb}</span>
          </button>
        ))}
      </div>
      <LoadingStatus />
    </main>
  );
}
