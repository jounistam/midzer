import { STYLES } from '@midzer/audio-web';
import { ChordDisplay } from '../components/ChordDisplay';
import { PlayButton } from '../components/ControlPanel';
import { Keyboard } from '../components/Keyboard';
import { KitView } from '../components/KitView';
import { DeviceChip } from '../components/TopBar';
import { finishOnboarding, setInstrument, setOnboardingStep, setStyle, startAudio, useSession } from '../session';
import { LoadingStatus } from './LoadingStatus';

export function Onboarding() {
  const step = useSession((s) => s.onboardingStep);
  return (
    <main className="onboarding">
      <header className="onboarding-head">
        <span className="brand">midzer</span>
        <ol className="steps" aria-label="Setup progress">
          {['Connect', 'Vibe', 'Play'].map((label, i) => (
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
  const drums = useSession((s) => s.instrument === 'drums');
  const begin = () => {
    void startAudio();
    setOnboardingStep(1);
  };
  return (
    <section className="step">
      <p className="eyebrow">Step 1 of 3</p>
      <div className="segmented instrument-pick" role="group" aria-label="I play">
        <button className={`seg ${drums ? '' : 'on'}`} aria-pressed={!drums} onClick={() => setInstrument('keys')}>
          I play keys
        </button>
        <button className={`seg ${drums ? 'on' : ''}`} aria-pressed={drums} onClick={() => setInstrument('drums')}>
          I play drums
        </button>
      </div>
      <h1>{drums ? 'Plug in your e-kit' : 'Plug in your keyboard'}</h1>
      <p className="lead">
        {drums
          ? 'Connect your electronic drum kit with USB or a MIDI cable, then press Continue. No kit? You can drum on your computer keys.'
          : 'Connect a MIDI keyboard with USB, then press Continue. No keyboard? No problem — you can play with your computer keys.'}
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
  const style = useSession((s) => s.style);
  return (
    <section className="step">
      <p className="eyebrow">Step 2 of 3</p>
      <h1>Pick a vibe</h1>
      <p className="lead">This sets how your band plays. You can change it any time.</p>
      <div className="vibe-grid">
        {STYLES.map((v, i) => (
          <button
            key={v.name}
            className={`vibe-card ${i === style ? 'selected' : ''}`}
            aria-pressed={i === style}
            onClick={() => setStyle(i)}
          >
            <span className="vibe-name">{v.name}</span>
            <span className="vibe-blurb">{v.blurb}</span>
            <span className="vibe-tempo">{v.bpm} BPM</span>
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
  const drums = useSession((s) => s.instrument === 'drums');
  return drums ? <DrumPlayStep /> : <KeysPlayStep />;
}

function DrumPlayStep() {
  return (
    <section className="step step-play">
      <div className="step-play-head">
        <div>
          <p className="eyebrow">Step 3 of 3</p>
          <h1>Hit your drums</h1>
          <p className="lead">
            Each drum you hit lights up below. On the computer: <kbd>V</kbd> kick, <kbd>F</kbd> snare, <kbd>J</kbd> hi-hat. If
            the wrong drum lights up, open Settings and choose Learn my kit.
          </p>
        </div>
        <DeviceChip />
      </div>
      <div className="actions">
        <PlayButton />
        <button className="btn btn-primary btn-xl" onClick={finishOnboarding}>
          Start jamming
        </button>
        <LoadingStatus />
      </div>
      <KitView height={220} />
    </section>
  );
}

function KeysPlayStep() {
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
