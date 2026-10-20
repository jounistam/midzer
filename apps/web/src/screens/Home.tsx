import { DeviceChip } from '../components/TopBar';
import { goTo, startAudio, useSession } from '../session';
import { LoadingStatus } from './LoadingStatus';

const MODES = [
  { id: 'jam', title: 'Jam', blurb: 'Hold chords, the band follows you.', ready: true },
  { id: 'songs', title: 'Songs', blurb: 'Play along with chord charts and MIDI files.', ready: false },
  { id: 'learn', title: 'Learn', blurb: 'Falling notes, wait mode and lessons.', ready: false },
] as const;

export function Home() {
  const audio = useSession((s) => s.audio);
  const openJam = () => {
    if (audio === 'idle' || audio === 'error') void startAudio();
    goTo('jam');
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
          <button
            key={m.id}
            className={`mode-card ${m.ready ? '' : 'soon'}`}
            disabled={!m.ready}
            onClick={m.id === 'jam' ? openJam : undefined}
          >
            <span className="mode-title">{m.title}</span>
            <span className="mode-blurb">{m.blurb}</span>
            {!m.ready && <span className="badge">Coming soon</span>}
          </button>
        ))}
      </div>
      <LoadingStatus />
    </main>
  );
}
