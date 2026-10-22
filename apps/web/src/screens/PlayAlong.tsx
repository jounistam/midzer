import { useEffect, useState } from 'react';
import type { LiveDevice } from '@midzer/audio-web';
import { PlayButton } from '../components/ControlPanel';
import { TimingBar } from '../components/DrummerStage';
import { KitView } from '../components/KitView';
import { TimingReport } from '../components/TimingReport';
import { TopBar } from '../components/TopBar';
import { SYNC_TAPS } from '../playalong/sync';
import {
  cancelLiveSync,
  canCaptureTab,
  goTo,
  liveDownbeat,
  liveSourceKey,
  liveTempo,
  refreshLiveDevices,
  startLiveSource,
  startLiveSync,
  stopLiveSource,
  useSession,
} from '../session';
import { LoadingStatus } from './LoadingStatus';

/**
 * Drumming along with any audio: a browser tab, the computer's own output or
 * a line input. midzer finds the beat as it plays and times every hit against
 * it, a beat later; the band stays out.
 */
export function PlayAlong() {
  return (
    <div className="jam along-screen">
      <TopBar title="Play along" onBack={() => goTo('home')} styles={false} />
      <main className="jam-main">
        <div className="jam-stage">
          <AlongStage />
          <AlongTiming />
          <LoadingStatus />
        </div>
        <aside className="panel control-panel">
          <SourcePicker />
          <PlayButton className="full" />
          <BeatFixes />
          <SyncControl />
        </aside>
      </main>
      <KitView height={170} />
    </div>
  );
}

/** The tempo found, how sure it is, and the beat. Read from a distance, like the drummer's Jam display. */
function AlongStage() {
  const source = useSession((s) => s.liveSource);
  const tracker = useSession((s) => s.liveTracker);
  const bpm = useSession((s) => s.bpm);
  const beat = useSession((s) => s.beat);
  const listening = source.status === 'on';
  const status = !listening ? 'Pick something to play along with' : tracker.locked ? 'Found the beat' : tracker.level < 0.25 ? 'Waiting for sound…' : 'Listening for the beat…';
  return (
    <section className="panel drum-stage along-stage" aria-live="polite">
      <div className="stage-cell">
        <p className="eyebrow">Tempo</p>
        <p className={`stage-big ${listening && tracker.locked ? '' : 'empty'}`} data-testid="along-tempo">
          {listening && tracker.locked ? bpm : '—'}
        </p>
        <p className="stage-sub" data-testid="along-status">
          {status}
        </p>
      </div>
      <div className="stage-cell along-meters">
        <Meter label="Sound" value={listening ? tracker.level : 0} />
        <Meter label="Sure of the beat" value={listening ? tracker.confidence : 0} />
      </div>
      {listening && tracker.locked && (
        <div className="stage-beats" aria-label={`Beat ${beat + 1}`}>
          {[0, 1, 2, 3].map((b) => (
            <span key={b} className={`stage-beat ${b === beat ? 'active' : ''} ${b === 0 ? 'one' : ''}`} />
          ))}
        </div>
      )}
    </section>
  );
}

function Meter({ label, value }: { label: string; value: number }) {
  return (
    <div className="along-meter">
      <p className="label">{label}</p>
      <div className="mic-meter-track" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(value * 100)}>
        <div className="mic-meter-fill" style={{ width: `${Math.round(value * 100)}%` }} />
      </div>
    </div>
  );
}

/** Rushing or dragging while drumming; the report after each run. */
function AlongTiming() {
  const playing = useSession((s) => s.playing);
  const drums = useSession((s) => s.instrument === 'drums');
  return (
    <section className="panel song-timing">
      {playing ? (
        <TimingBar against="the track" />
      ) : (
        <div className="along-report">
          {!drums && <p className="setting-note">Hits are timed on drums: an e-kit, the pads below or the computer keys (V kick, F snare, J hi-hat).</p>}
          <TimingReport source="the track" />
        </div>
      )}
    </section>
  );
}

/** Where the audio comes from: a browser tab, or an input (system audio first where there is one). */
function SourcePicker() {
  const source = useSession((s) => s.liveSource);
  const devices = useSession((s) => s.liveDevices);
  const ready = useSession((s) => s.audio === 'ready');
  const tab = canCaptureTab();
  const [picked, setPicked] = useState<string>('');
  useEffect(() => {
    if (ready) void refreshLiveDevices();
  }, [ready]);
  const device = devices.find((d) => d.id === picked) ?? devices[0];
  const name = (key: string) => (key === 'tab' ? 'the browser tab' : (devices.find((d) => liveSourceKey({ kind: 'device', id: d.id }) === key)?.name ?? 'the input'));
  const busy = source.status === 'opening';

  return (
    <div className="control-group">
      <p className="label" id="along-source-label">
        Play along with
      </p>
      <div className="along-sources" role="group" aria-labelledby="along-source-label">
        {tab && (
          <button className="btn btn-secondary" disabled={!ready || busy} onClick={() => void startLiveSource({ kind: 'tab' })}>
            A browser tab
          </button>
        )}
        {devices.length > 0 && (
          <div className="along-device">
            <select value={device?.id ?? ''} onChange={(e) => setPicked(e.target.value)} aria-label="Audio input">
              {devices.map((d) => (
                <option key={d.id} value={d.id}>
                  {deviceLabel(d)}
                </option>
              ))}
            </select>
            <button className="btn btn-secondary" disabled={!ready || busy || !device} onClick={() => device && void startLiveSource({ kind: 'device', id: device.id })}>
              Listen
            </button>
          </div>
        )}
      </div>
      {source.status === 'none' && (
        <p className="setting-note">
          {tab
            ? 'Play a song in another tab, pick that tab and share its audio. Or listen to an input: a line in, or "Monitor" for what this computer plays.'
            : 'Pick system audio to follow whatever this computer plays, or an input such as a line in.'}
        </p>
      )}
      {source.status === 'opening' && (
        <p className="setting-note" role="status">
          {source.key === 'tab' ? 'Pick the tab and turn on sharing its audio…' : 'Opening the input…'}
        </p>
      )}
      {(source.status === 'on' || source.status === 'ended') && (
        <div className="along-listening">
          <p className="setting-note" role="status" data-testid="along-source">
            {source.status === 'on' ? `Listening to ${name(source.key)}` : 'Sharing stopped.'}
          </p>
          {source.status === 'on' && (
            <button className="btn btn-ghost" onClick={stopLiveSource}>
              Stop listening
            </button>
          )}
        </div>
      )}
      {source.status === 'error' && (
        <p className="status-line error" role="alert">
          {source.message}
        </p>
      )}
    </div>
  );
}

function deviceLabel(d: LiveDevice): string {
  return d.loopback ? `System audio · ${d.name}` : d.name;
}

/** When the beat found is off: the 1 in the wrong place, or twice or half the tempo. */
function BeatFixes() {
  const locked = useSession((s) => s.liveSource.status === 'on' && s.liveTracker.locked);
  return (
    <div className="control-group">
      <p className="label">The beat</p>
      <button className="btn btn-secondary" disabled={!locked} onClick={liveDownbeat} title="Press on a 1 of the song">
        This is the 1
      </button>
      <div className="segmented" role="group" aria-label="Tempo">
        <button className="seg" disabled={!locked} onClick={() => liveTempo(false)}>
          Half as fast
        </button>
        <button className="seg" disabled={!locked} onClick={() => liveTempo(true)}>
          Twice as fast
        </button>
      </div>
    </div>
  );
}

/** Lines the source up with what the player hears: tap the beat for a few bars. */
function SyncControl() {
  const source = useSession((s) => s.liveSource);
  const offsets = useSession((s) => s.liveOffsets);
  const taps = useSession((s) => s.liveSyncTaps);
  const error = useSession((s) => s.liveSyncError);
  const locked = useSession((s) => s.liveTracker.locked);
  if (source.status === 'none' || source.status === 'error') return null;
  const offset = offsets[source.key];
  return (
    <div className="control-group">
      <p className="label">Sync</p>
      {taps === null ? (
        <>
          <p className="setting-note" data-testid="along-offset">
            {offset === undefined ? 'Not synced yet: hits may look early or late by a fixed amount.' : `Heard ${Math.abs(offset)} ms ${offset >= 0 ? 'after' : 'before'} midzer gets it.`}
          </p>
          <button className="btn btn-secondary" disabled={source.status !== 'on' || !locked} onClick={startLiveSync}>
            Sync by tapping
          </button>
        </>
      ) : (
        <>
          <p className="setting-note" role="status" data-testid="along-sync">
            Tap every beat on any pad: {taps} of {SYNC_TAPS}
          </p>
          <button className="btn btn-ghost" onClick={cancelLiveSync}>
            Cancel
          </button>
        </>
      )}
      {error && (
        <p className="setting-note error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
