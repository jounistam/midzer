import { useEffect, useState } from 'react';
import type { LiveDevice } from '@midzer/audio-web';
import { ChordLanes } from '../components/ChordLanes';
import { ChordSheet } from '../components/ChordSheet';
import { PlayButton } from '../components/ControlPanel';
import { HitFeedback } from '../components/HitFeedback';
import { Keyboard } from '../components/Keyboard';
import { KitView } from '../components/KitView';
import { TimingReport } from '../components/TimingReport';
import { TopBar } from '../components/TopBar';
import type { ChordNow } from '../playalong/chords';
import { MISS_SOUND_OPTIONS } from '../playalong/feedback';
import { ALONG_LEVELS } from '../playalong/levels';
import { SYNC_TAPS } from '../playalong/sync';
import { useStageHeight } from '../components/stageHeight';
import {
  cancelLiveSync,
  canCaptureTab,
  canLearnLiveSongs,
  clearLearnError,
  dismissLearnedSong,
  goTo,
  keepLearnedSong,
  liveDownbeat,
  liveSourceKey,
  liveTempo,
  notThisSong,
  refreshLiveDevices,
  resetLiveSource,
  saveLiveSong,
  setAlongLevel,
  setInstrument,
  setMissSound,
  startLiveSource,
  startLiveSync,
  stopLiveSource,
  useSession,
} from '../session';
import type { LearnedSong } from '../session';
import { LoadingStatus } from './LoadingStatus';

/**
 * Playing along with any audio, on drums or keys: a browser tab, the
 * computer's own output or a line input. midzer finds the beat as it plays
 * and times every hit against it, a beat later; the band stays out. While
 * playing, how the hits land is the biggest thing on screen.
 */
export function PlayAlong() {
  const drums = useSession((s) => s.instrument === 'drums');
  const split = useSession((s) => s.split);
  const height = useStageHeight();
  return (
    <div className="jam along-screen">
      <TopBar title="Play along" onBack={() => goTo('home')} styles={false} />
      <main className="jam-main">
        <div className="jam-stage along-main">
          <AlongStage />
          <FollowedSong />
          {!drums && <FollowedChords />}
          {!drums && <AlongChords split={split} />}
          <AlongTiming />
          <LearnedSongs />
          <LoadingStatus />
        </div>
        <aside className="panel control-panel">
          <SourcePicker />
          <PlayButton className="full" />
          <InstrumentPicker />
          {!drums && <LevelPicker />}
          <BeatFixes />
          <SyncControl />
          <MissSoundControl />
        </aside>
      </main>
      {drums && <KitView height={height} />}
    </div>
  );
}

/** The tempo found, the chord, how sure it is, and the beat: a strip above the accuracy, readable from a distance. */
function AlongStage() {
  const source = useSession((s) => s.liveSource);
  const tracker = useSession((s) => s.liveTracker);
  const chord = useSession((s) => s.liveChord);
  const heard = useSession((s) => s.bpm);
  const beat = useSession((s) => s.beat);
  const listening = source.status === 'on';
  // A song kept keeps its own tempo.
  const followed = useSession((s) => (listening ? s.liveFollow : null));
  const bpm = followed?.bpm ?? heard;
  const status = !listening
    ? 'Pick something to play along with'
    : followed && tracker.locked
      ? 'The tempo kept for this song'
      : tracker.locked
        ? 'Found the beat'
        : tracker.level < 0.25
          ? 'Waiting for sound…'
          : 'Listening for the beat…';
  const locked = listening && tracker.locked;
  return (
    <section className="panel along-strip" aria-live="polite">
      <div className="along-tempo">
        <p className="eyebrow">Tempo</p>
        <p className={`along-bpm ${locked ? '' : 'empty'}`} data-testid="along-tempo">
          {locked ? bpm : '—'}
        </p>
        <p className="stage-sub" data-testid="along-status">
          {status}
        </p>
      </div>
      <div className="along-chord">
        <p className="eyebrow">Chord</p>
        <p className={`along-chord-name ${chord.current ? '' : 'empty'}`} data-testid="along-chord">
          {listening && chord.current ? chord.current : '—'}
        </p>
        <p className="stage-sub" data-testid="along-chord-trail">
          {chordLine(listening, chord)}
        </p>
      </div>
      <div className={`stage-beats ${locked ? '' : 'waiting'}`} aria-label={locked ? `Beat ${beat + 1}` : 'No beat yet'}>
        {/* Four, or more in a longer bar of a song followed. */}
        {Array.from({ length: Math.max(4, beat + 1) }, (_, b) => b).map((b) => (
          <span key={b} className={`stage-beat ${locked && b === beat ? 'active' : ''} ${b === 0 ? 'one' : ''}`} />
        ))}
      </div>
      <div className="along-meters">
        <Meter label="Sound" value={listening ? tracker.level : 0} />
        <Meter label="Sure of the beat" value={listening ? tracker.confidence : 0} />
      </div>
    </section>
  );
}

/** Under the chord: what comes next once the song's loop is clear, else what came before. */
function chordLine(listening: boolean, chord: ChordNow): string {
  if (!listening) return '\u00a0';
  if (chord.next) return `next ${chord.next.name} in ${Math.max(0, chord.next.inMs / 1000).toFixed(1)} s`;
  if (chord.before.length > 0) return `after ${[...chord.before].reverse().join(' · ')}`;
  return 'Listening for chords…';
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

/** How the hits land while playing; the report after each run. */
function AlongTiming() {
  const playing = useSession((s) => s.playing);
  const drums = useSession((s) => s.instrument === 'drums');
  // On keys the chords coming take the room; the timing stays in view, smaller.
  if (playing) return <HitFeedback against="the track" lagging compact={!drums} />;
  return (
    <section className="panel song-timing">
      <div className="along-report">
        <p className="setting-note">
          {drums
            ? 'Drum on an e-kit, the pads below or the computer keys (V kick, F snare, J hi-hat).'
            : 'Play a MIDI keyboard, the keys below or the computer keys (A S D F… for C D E F…); notes struck together count as one chord.'}
        </p>
        <TimingReport source="the track" />
      </div>
    </section>
  );
}

/** The keys of each chord at the player's level falling onto the keyboard, as in Learn. */
function AlongChords({ split }: { split: number }) {
  const level = useSession((s) => s.alongLevel);
  const twoHands = level >= 4;
  return (
    <section className="panel along-play">
      <div className="along-play-head">
        <p className="eyebrow">Chords</p>
        <p className="along-legend">
          {twoHands && <span className="hand left">Left hand</span>}
          <span className="hand right">Right hand</span>
          <span className="hand next">Next chord</span>
        </p>
      </div>
      <ChordLanes />
      <Keyboard split={split} height={150} />
    </section>
  );
}

/** How much to play along with the chords: one key up to full chords with both hands. */
function LevelPicker() {
  const level = useSession((s) => s.alongLevel);
  const current = ALONG_LEVELS.find((l) => l.level === level)!;
  return (
    <div className="control-group">
      <p className="label" id="along-level-label">
        Level
      </p>
      <div className="segmented" role="group" aria-labelledby="along-level-label">
        {ALONG_LEVELS.map((l) => (
          <button key={l.level} className={`seg ${level === l.level ? 'on' : ''}`} aria-pressed={level === l.level} title={l.name} onClick={() => setAlongLevel(l.level)}>
            {l.level}
          </button>
        ))}
      </div>
      <p className="setting-note" data-testid="along-level">
        <strong>{current.name}.</strong> {current.detail}
      </p>
    </div>
  );
}

/** Drums or keys: what the computer keys play and what is timed. */
function InstrumentPicker() {
  const instrument = useSession((s) => s.instrument);
  return (
    <div className="control-group">
      <p className="label" id="along-instrument-label">
        I'm playing
      </p>
      <div className="segmented" role="group" aria-labelledby="along-instrument-label">
        {(['drums', 'keys'] as const).map((i) => (
          <button key={i} className={`seg ${instrument === i ? 'on' : ''}`} aria-pressed={instrument === i} onClick={() => setInstrument(i)}>
            {i === 'drums' ? 'Drums' : 'Keys'}
          </button>
        ))}
      </div>
    </div>
  );
}

/** A soft tick when a hit is clearly off, for a player watching their instrument instead of the screen. */
export function MissSoundControl() {
  const sound = useSession((s) => s.missSound);
  return (
    <div className="control-group">
      <p className="label" id="miss-sound-label">
        Sound when off the beat
      </p>
      <div className="segmented" role="group" aria-labelledby="miss-sound-label">
        {MISS_SOUND_OPTIONS.map((o) => (
          <button key={o.value} className={`seg ${sound === o.value ? 'on' : ''}`} aria-pressed={sound === o.value} onClick={() => setMissSound(o.value)}>
            {o.label}
          </button>
        ))}
      </div>
      <p className="setting-note">A high tick when early, a low one when late, a double one for a wrong chord.</p>
    </div>
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

/** A song kept, recognised in the source: its stored beats and chords lead, known ahead. */
function FollowedSong() {
  const followed = useSession((s) => (s.liveSource.status === 'on' ? s.liveFollow : null));
  if (!followed) return null;
  return (
    <section className="panel along-followed" aria-live="polite" data-testid="along-followed">
      <span className="followed-dot" aria-hidden />
      <div className="learned-info">
        <span className="eyebrow">Following</span>
        <span className="learned-title">
          {followed.title} <span className="learned-meta">· {followed.bpm} BPM</span>
        </span>
      </div>
      <button className="btn btn-ghost" onClick={notThisSong} title="It is another song: listen afresh until the music stops">
        Not this song
      </button>
    </section>
  );
}

/** The chords of the song followed, each one to look up. */
function FollowedChords() {
  const chords = useSession((s) => (s.liveSource.status === 'on' ? s.liveFollow?.chords : undefined));
  return chords ? <ChordSheet chords={chords} /> : null;
}

const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.round(seconds % 60)).padStart(2, '0')}`;

/**
 * Songs heard while playing along, learned from start to end (a silence
 * either side) or up to "Save this song": the one being read, then each
 * offered to keep in My songs. Only the beat map, chords and a fingerprint
 * are kept, never the audio.
 */
function LearnedSongs() {
  const { analysing, offered, error } = useSession((s) => s.liveLearning);
  const [kept, setKept] = useState<string | null>(null);
  if (!analysing && offered.length === 0 && !error && !kept) return null;
  const keep = async (id: string, title: string, update: boolean) => {
    const ok = await keepLearnedSong(id, update ? undefined : title);
    setKept(!ok ? 'It could not be saved on this device.' : update ? `Updated “${title}” in My songs.` : `Saved “${title}” in My songs.`);
  };
  return (
    <section className="panel along-learned" aria-live="polite" data-testid="along-learned">
      <p className="eyebrow">Songs heard</p>
      {analysing && (
        <div className="status-line" role="status">
          <span>Learning the song heard ({clock(analysing.seconds)})… {Math.round(analysing.fraction * 100)}%</span>
          <span className="progress" aria-hidden>
            <span style={{ width: `${Math.round(analysing.fraction * 100)}%` }} />
          </span>
        </div>
      )}
      {offered.map((learned) => (
        <LearnedOffer key={learned.song.id} learned={learned} onKeep={keep} />
      ))}
      {kept && offered.length === 0 && !analysing && (
        <p className="status-line" role="status">
          {kept}
          <button className="btn btn-ghost" onClick={() => setKept(null)}>
            OK
          </button>
        </p>
      )}
      {error && (
        <p className="status-line error" role="alert">
          The song heard could not be learned: {error}
          <button className="btn btn-ghost" onClick={clearLearnError}>
            OK
          </button>
        </p>
      )}
    </section>
  );
}

/**
 * One song heard, offered: name it and save it, the thing to do; dismissing
 * forgets it for good, so it asks first.
 */
function LearnedOffer({ learned: { song, extends: more }, onKeep }: { learned: LearnedSong; onKeep: (id: string, title: string, update: boolean) => Promise<void> }) {
  const [name, setName] = useState(song.title);
  const [saving, setSaving] = useState(false);
  const [dismissing, setDismissing] = useState(false);
  const title = more ? more.title : name.trim() || song.title;
  const save = async () => {
    setSaving(true);
    await onKeep(song.id, title, more !== undefined);
    setSaving(false);
  };
  return (
    <div className="learned-song">
      <div className="learned-info">
        <span className="learned-title">{more ? `More of “${more.title}”` : 'A new song'}</span>
        <span className="learned-meta">
          {clock(song.heard?.seconds ?? 0)} · {song.bpm} BPM · {song.bars.length} bars
          {song.heard && !song.heard.complete && (
            <span className="badge" title="Heard from the middle, or saved before it ended">
              partial
            </span>
          )}
        </span>
      </div>
      {dismissing ? (
        <div className="learned-actions">
          <p className="setting-note">Dismiss it? What was learned is forgotten and can't be saved later.</p>
          <button className="btn btn-danger" onClick={() => dismissLearnedSong(song.id)}>
            Dismiss it
          </button>
          <button className="btn btn-ghost" autoFocus onClick={() => setDismissing(false)}>
            Cancel
          </button>
        </div>
      ) : (
        <form
          className="learned-actions"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          {!more && <input type="text" className="learned-name" value={name} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.target.select()} aria-label="Song name" placeholder={song.title} maxLength={120} />}
          <button
            type="submit"
            className="btn btn-primary"
            disabled={saving}
            title={more ? 'Replace the part kept with what was heard now: its chords and beats' : 'Save it in My songs'}
          >
            {more ? 'Update' : 'Save'}
          </button>
          <button type="button" className="btn btn-ghost" disabled={saving} onClick={() => setDismissing(true)}>
            Dismiss
          </button>
        </form>
      )}
    </div>
  );
}

/** When the beat found is off: the 1 in the wrong place, or twice or half the tempo; or start the song over, or save it. */
function BeatFixes() {
  const on = useSession((s) => s.liveSource.status === 'on');
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
      <button className="btn btn-ghost" disabled={!on} onClick={resetLiveSource} title="Forget the beat and chords heard and listen afresh, as when a new song starts">
        Start over
      </button>
      {canLearnLiveSongs() && (
        <button className="btn btn-ghost" disabled={!on} onClick={saveLiveSong} title="Learn the song heard so far now; songs are also learned by themselves when they end in a silence">
          Save this song
        </button>
      )}
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
