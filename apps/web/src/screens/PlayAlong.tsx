import { useEffect, useState } from 'react';
import type { LiveDevice } from '@midzer/audio-web';
import { ChordLanes } from '../components/ChordLanes';
import { ChordSheet } from '../components/ChordSheet';
import { PlayButton, SoundPicker } from '../components/ControlPanel';
import { HitFeedback } from '../components/HitFeedback';
import { Keyboard } from '../components/Keyboard';
import { KitView } from '../components/KitView';
import { SoloRoll } from '../components/SoloRoll';
import { TimingReport } from '../components/TimingReport';
import { TopBar } from '../components/TopBar';
import type { ChordNow } from '../playalong/chords';
import { MISS_SOUND_OPTIONS } from '../playalong/feedback';
import { ALONG_LEVELS } from '../playalong/levels';
import { isDrumTake, ownShare } from '../playalong/replay';
import { keyName } from '../playalong/scale';
import { DRUM_SOLO_LEVELS, feelWord, homeKey, SOLO_LEVELS, type CallNow, type DrumSoloLevel, type FitCount, type SoloLevel } from '../playalong/solo';
import { noteToQwerty } from '../qwerty';
import { NOTE_NAMES } from '@midzer/audio-web';
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
  playReplay,
  refreshLiveDevices,
  resetLiveSource,
  saveLiveSong,
  setAlongLevel,
  setAlongMode,
  setInstrument,
  setMissSound,
  setDrumSoloLevel,
  setSoloCall,
  setSoloLevel,
  startLiveSource,
  startLiveSync,
  stopLiveSource,
  stopReplay,
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
  const solo = useSession((s) => s.alongMode === 'solo');
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
          {solo ? <SoloPlay split={split} /> : !drums && <AlongChords split={split} />}
          {solo && <SoloReplay />}
          <AlongTiming />
          <LearnedSongs />
          <LoadingStatus />
        </div>
        <aside className="panel control-panel">
          <SourcePicker />
          <PlayButton className="full" />
          <InstrumentPicker />
          <SoundPicker />
          <ModePicker />
          {solo ? <SoloLevelPicker /> : !drums && <LevelPicker />}
          {solo && <CallToggle />}
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
  // Taps that drive a solo aren't timed.
  const tapping = useSession((s) => s.alongMode === 'solo' && (s.instrument === 'drums' ? s.drumSoloLevel : s.soloLevel) <= 3);
  if (tapping) return null;
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

/**
 * Soloing: midzer plays the solo, driven by the player's taps on the lit
 * keys, or fills around the player's own notes. How hard it is driven shows
 * big, so the player sees it follow them; how the player's own notes fit
 * shows under it.
 */
function SoloPlay({ split }: { split: number }) {
  const drums = useSession((s) => s.instrument === 'drums');
  return drums ? <DrumSoloPlay /> : <KeysSoloPlay split={split} />;
}

function KeysSoloPlay({ split }: { split: number }) {
  const key = useSession((s) => s.soloKey);
  const level = useSession((s) => s.soloLevel);
  const fit = useSession((s) => s.soloFit);
  const calling = useSession((s) => s.soloCall);
  const call = useSession((s) => s.soloCallNow);
  const locked = useSession((s) => s.liveSource.status === 'on' && s.liveTracker.locked);
  const prompt = !locked ? 'The solo starts once the beat is found.' : key === null ? 'Listening for the song’s key…' : calling ? callPrompt(level, homeKey(key)) : soloPrompt(level, homeKey(key));
  return (
    <section className="panel along-play solo-play">
      <div className="along-play-head">
        <p className="eyebrow">Solo{key ? ` · ${keyName(key)}` : ''}</p>
        <p className="setting-note" data-testid="solo-prompt">
          {prompt}
        </p>
      </div>
      <div className="solo-feel">
        <SoloFeel call={calling ? call : null} tapping={level <= 2} />
        {level >= 3 && <SoloFit fit={fit} snapped={level < 5} />}
      </div>
      <Keyboard split={split} height={150} />
    </section>
  );
}

/** A drum solo: midzer plays the kit (shown below the stage), driven by the player's taps, or around their own playing. */
function DrumSoloPlay() {
  const level = useSession((s) => s.drumSoloLevel);
  const calling = useSession((s) => s.soloCall);
  const call = useSession((s) => s.soloCallNow);
  const locked = useSession((s) => s.liveSource.status === 'on' && s.liveTracker.locked);
  const prompt = !locked ? 'The solo starts once the beat is found.' : calling ? drumCallPrompt(level) : drumSoloPrompt(level);
  return (
    <section className="panel along-play solo-play">
      <div className="along-play-head">
        <p className="eyebrow">Drum solo</p>
        <p className="setting-note" data-testid="solo-prompt">
          {prompt}
        </p>
      </div>
      <div className="solo-feel">
        <SoloFeel call={calling ? call : null} tapping={level <= 2} />
      </div>
    </section>
  );
}

/** How hard the solo is driven, big: a word and a meter; with call and response, where it is instead of the word. */
function SoloFeel({ call, tapping }: { call: CallNow | null; tapping: boolean }) {
  const feel = useSession((s) => s.soloFeel);
  return (
    <>
      {call ? (
        <CallStatus call={call} tapping={tapping} />
      ) : (
        <p className="solo-feel-word" data-testid="solo-feel">
          {feelWord(feel)}
        </p>
      )}
      <div className="solo-feel-track" role="meter" aria-label="Feel" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(feel * 100)}>
        <div className="solo-feel-fill" style={{ width: `${Math.round(feel * 100)}%` }} />
      </div>
    </>
  );
}

/** What to do at drum solo `level`. */
function drumSoloPrompt(level: DrumSoloLevel): string {
  switch (level) {
    case 1:
      return 'Tap the lit snare (F or the space bar). Faster for a busier groove with fills, harder for louder.';
    case 2:
      return 'Tap the kick (V) for a kick-led groove, the snare (F) for the backbeat, a tom for a fill.';
    case 3:
      return 'Play the kick and snare; midzer keeps the hi-hat and adds ghost notes and fills.';
    case 4:
      return 'Play the whole kit.';
  }
}

/** What to do with call and response on at drum solo `level`. */
function drumCallPrompt(level: DrumSoloLevel): string {
  if (level === 1) return 'Tap the snare to hear a call, then tap its rhythm back on the snare: each tap plays its next hit.';
  if (level === 2) return 'Tap a lit pad to hear a call, then tap its rhythm back: each tap plays its next hit.';
  return 'Hit a pad to hear a call, then play it back on the same drums; the next one to hit is lit.';
}

/** What to do at `level`, with the home key `home`. */
function soloPrompt(level: SoloLevel, home: number): string {
  const name = (k: number) => NOTE_NAMES[k % 12]!;
  const letter = (k: number) => noteToQwerty(k)?.toUpperCase();
  const labelled = (k: number, where: string) => `${name(k)} ${where}${letter(k) ? ` (${letter(k)})` : ''}`;
  switch (level) {
    case 1:
      return `Tap ${name(home)} (the lit key${letter(home) ? `, ${letter(home)}` : ''} or the space bar). Faster for more, harder for louder, hold for a long note.`;
    case 2:
      return `Tap ${labelled(home, 'low')}, ${labelled(home + 7, 'middle')} or ${labelled(home + 12, 'high')}. Faster for more, hold for a long note.`;
    case 3:
      return 'Play the lit keys; the bright ones fit the chord now. midzer fills the gaps when you leave them.';
    case 4:
      return 'Play every note; ones off the scale snap onto it.';
    case 5:
      return 'Play freely.';
  }
}

/** What to do with call and response on at `level`, with the home key `home`. */
function callPrompt(level: SoloLevel, home: number): string {
  const name = NOTE_NAMES[home % 12]!;
  if (level === 1) return `Tap ${name} to hear a call, then tap its rhythm back on ${name}: each tap plays its next note.`;
  if (level === 2) return 'Tap a lit key to hear a call, then tap its rhythm back on any of them: each tap plays its next note.';
  return 'Play a note to hear a call, then play it back; the next note to play is lit.';
}

/** Call and response: listen to midzer's call, then answer it; the call's notes as dots, filled as they sound or are got. */
function CallStatus({ call, tapping }: { call: CallNow; tapping: boolean }) {
  const word = { idle: tapping ? 'Tap to start' : 'Play to start', listen: 'Listen', answer: 'Your turn' }[call.phase];
  return (
    <div className="solo-call" data-testid="solo-call">
      <p className={`solo-feel-word solo-call-word ${call.phase}`}>{word}</p>
      {call.phase !== 'idle' && (
        <p className="solo-call-notes" aria-label={`${call.hit} of ${call.of}`}>
          {Array.from({ length: call.of }, (_, i) => (
            <span key={i} className={`solo-call-note ${i < call.hit ? 'on' : ''}`} />
          ))}
        </p>
      )}
      <p className="solo-call-last" data-testid="solo-call-last">
        {call.last ? `Last answer: ${call.last.hit} of ${call.last.of}` : '\u00a0'}
      </p>
    </div>
  );
}

/** After a solo run: its notes bar by bar, how much of it the player played, and a replay. */
function SoloReplay() {
  const take = useSession((s) => s.soloTake);
  const playing = useSession((s) => s.playing);
  const replay = useSession((s) => s.replay);
  const bpm = useSession((s) => s.liveFollow?.bpm ?? s.bpm);
  const locked = useSession((s) => s.liveSource.status === 'on' && s.liveTracker.locked);
  if (!take || playing) return null;
  const share = Math.round(ownShare(take) * 100);
  return (
    <section className="panel solo-replay" data-testid="solo-replay">
      <div className="along-play-head">
        <p className="eyebrow">Your solo</p>
        <p className="setting-note" data-testid="solo-replay-share">
          You played {share}% of {take.notes.length} {isDrumTake(take) ? 'hits' : 'notes'} over {take.bars} {take.bars === 1 ? 'bar' : 'bars'}.
        </p>
      </div>
      <p className="along-legend solo-replay-legend">
        {/* The player's notes in magenta, midzer's in cyan, as in the roll. */}
        <span className="hand left">You</span>
        <span className="hand right">midzer</span>
      </p>
      <SoloRoll take={take} playingBpm={replay === 'playing' ? bpm : null} />
      <div className="solo-replay-actions">
        {replay === 'off' ? (
          <button className="btn btn-secondary" onClick={playReplay}>
            Play it back
          </button>
        ) : (
          <button className="btn btn-secondary" onClick={stopReplay}>
            Stop
          </button>
        )}
        <p className="setting-note" data-testid="solo-replay-status">
          {replay === 'waiting' ? 'Starting on the next bar…' : replay === 'playing' ? 'Playing back' : locked ? 'Plays over the music from the next bar.' : 'Plays at the tempo you played, with a click.'}
        </p>
      </div>
    </section>
  );
}

/** How the player's own notes fit: chord tones, the scale, or off it. */
function SoloFit({ fit, snapped }: { fit: FitCount; snapped: boolean }) {
  const total = fit.chord + fit.scale + fit.outside;
  const word = { chord: 'Chord tone', scale: 'In the scale', outside: snapped ? 'Snapped' : 'Outside' } as const;
  return (
    <div className="solo-fit" data-testid="solo-fit">
      <p className={`solo-fit-last ${fit.last ?? ''}`}>{fit.last ? word[fit.last] : 'Your notes show here'}</p>
      {total > 0 && (
        <p className="solo-fit-counts">
          <span className="chord">{fit.chord} chord</span> · <span className="scale">{fit.scale} scale</span> ·{' '}
          <span className="outside">
            {fit.outside} {snapped ? 'snapped' : 'outside'}
          </span>
        </p>
      )}
    </div>
  );
}

/** Play along with the chords, or solo over them. */
function ModePicker() {
  const mode = useSession((s) => s.alongMode);
  const drums = useSession((s) => s.instrument === 'drums');
  return (
    <div className="control-group">
      <p className="label" id="along-mode-label">
        Play
      </p>
      <div className="segmented" role="group" aria-labelledby="along-mode-label">
        {(['chords', 'solo'] as const).map((m) => (
          <button key={m} className={`seg ${mode === m ? 'on' : ''}`} aria-pressed={mode === m} onClick={() => setAlongMode(m)}>
            {m === 'solo' ? 'Solo' : drums ? 'Along' : 'Chords'}
          </button>
        ))}
      </div>
    </div>
  );
}

/** How much of the solo the player plays: one key while midzer solos, up to every note. */
function SoloLevelPicker() {
  const drums = useSession((s) => s.instrument === 'drums');
  const level = useSession((s) => (drums ? s.drumSoloLevel : s.soloLevel));
  const levels: { level: number; name: string; detail: string }[] = drums ? DRUM_SOLO_LEVELS : SOLO_LEVELS;
  const current = levels.find((l) => l.level === level) ?? levels[0]!;
  const pick = (l: number) => (drums ? setDrumSoloLevel(l as DrumSoloLevel) : setSoloLevel(l as SoloLevel));
  return (
    <div className="control-group">
      <p className="label" id="solo-level-label">
        Solo level
      </p>
      <div className="segmented" role="group" aria-labelledby="solo-level-label">
        {levels.map((l) => (
          <button key={l.level} className={`seg ${level === l.level ? 'on' : ''}`} aria-pressed={level === l.level} title={l.name} onClick={() => pick(l.level)}>
            {l.level}
          </button>
        ))}
      </div>
      <p className="setting-note" data-testid="solo-level">
        <strong>{current.name}.</strong> {current.detail}
      </p>
    </div>
  );
}

/** Call and response while soloing: midzer plays a bar, the player answers it. */
function CallToggle() {
  const on = useSession((s) => s.soloCall);
  return (
    <div className="control-group">
      <p className="label" id="solo-call-label">
        Call and response
      </p>
      <div className="segmented" role="group" aria-labelledby="solo-call-label">
        {[false, true].map((v) => (
          <button key={String(v)} className={`seg ${on === v ? 'on' : ''}`} aria-pressed={on === v} onClick={() => setSoloCall(v)}>
            {v ? 'On' : 'Off'}
          </button>
        ))}
      </div>
      <p className="setting-note">midzer plays a bar, you answer it in the next. The calls grow longer as you get them right.</p>
    </div>
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
