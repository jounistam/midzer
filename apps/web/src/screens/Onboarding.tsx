import { ChordDisplay } from '../components/ChordDisplay';
import { PlayButton } from '../components/ControlPanel';
import { Keyboard } from '../components/Keyboard';
import { DeviceChip } from '../components/TopBar';
import { finishOnboarding, setOnboardingStep, startAudio, useSession } from '../session';
import { LoadingStatus } from './LoadingStatus';

const VIBES = [
  { name: 'Pop', blurb: 'Straight 8ths, warm piano', ready: true },
  { name: 'Rock', blurb: 'Driving drums, power chords', ready: false },
  { name: 'Funk', blurb: 'Tight 16ths, slap bass', ready: false },
  { name: 'Bossa', blurb: 'Soft nylon and brushes', ready: false },
  { name: 'Ballad', blurb: 'Slow, wide pads', ready: false },
  { name: 'Blues', blurb: 'Shuffle and walking bass', ready: false },
];

export function Onboarding() {
  const step = useSession((s) => s.onboardingStep);
  return (
    <main className="onboarding">
      <header className="onboarding-head">
        <span className="brand">midzer</span>
        <ol className="steps" aria-label="Setup progress">
          {['Keyboard', 'Vibe', 'Play'].map((label, i) => (
            <li key={label} className={i === step ? 'current' : i < step ? 'done' : ''}>
              {label}
            </li>
          ))}
        </ol>
      </header>
      {step === 0 && <ConnectStep />}
      {step === 1 && <VibeStep />}
      {step === 2 && <PlayStep />}
    </main>
  );
}

function ConnectStep() {
  const supported = useSession((s) => s.midiSupported);
  const begin = () => {
    void startAudio();
    setOnboardingStep(1);
  };
  return (
    <section className="step">
      <p className="eyebrow">Step 1 of 3</p>
      <h1>Plug in your keyboard</h1>
      <p className="lead">
        Connect a MIDI keyboard with USB, then press Continue. No keyboard? No problem — you can play with your computer
        keys.
      </p>
      {!supported && (
        <p className="notice">This browser can't use MIDI devices. Chrome or Edge can — or carry on with your computer keys.</p>
      )}
      <div className="actions">
        <button className="btn btn-primary btn-xl" onClick={begin}>
          Continue
        </button>
        <button className="btn btn-ghost" onClick={begin}>
          Use computer keyboard
        </button>
      </div>
    </section>
  );
}

function VibeStep() {
  return (
    <section className="step">
      <p className="eyebrow">Step 2 of 3</p>
      <h1>Pick a vibe</h1>
      <p className="lead">This sets how your band plays. You can change it any time.</p>
      <div className="vibe-grid">
        {VIBES.map((v) => (
          <button key={v.name} className={`vibe-card ${v.ready ? 'selected' : ''}`} disabled={!v.ready}>
            <span className="vibe-name">{v.name}</span>
            <span className="vibe-blurb">{v.ready ? v.blurb : 'Coming soon'}</span>
          </button>
        ))}
      </div>
      <div className="actions">
        <button className="btn btn-primary btn-xl" onClick={() => setOnboardingStep(2)}>
          Continue
        </button>
        <LoadingStatus />
      </div>
    </section>
  );
}

function PlayStep() {
  const chord = useSession((s) => s.chord);
  const playing = useSession((s) => s.playing);
  return (
    <section className="step step-play">
      <div className="step-play-head">
        <div>
          <p className="eyebrow">Step 3 of 3</p>
          <h1>Press Play and hold a chord</h1>
          <p className="lead">
            Try C major: <kbd>A</kbd> <kbd>D</kbd> <kbd>G</kbd> on your computer keyboard, or C–E–G on your MIDI keyboard.
          </p>
        </div>
        <DeviceChip />
      </div>
      <div className="panel compact-stage">
        <ChordDisplay hint="The band follows the chord you hold" />
      </div>
      <div className="actions">
        <PlayButton />
        <button className={`btn ${playing && chord ? 'btn-primary' : 'btn-secondary'} btn-xl`} onClick={finishOnboarding}>
          Start jamming
        </button>
        <LoadingStatus />
      </div>
      <Keyboard split={0} height={160} />
    </section>
  );
}
