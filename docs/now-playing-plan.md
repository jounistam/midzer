# Now playing: track info, reset on change, per-track memory

## Context
The Play-along live mode can already capture Spotify audio (tab capture, or the PulseAudio monitor via `openLive`), but it has no idea *what* is playing. Adding "now playing" metadata lets us (1) show title/artist/art on the Stage, (2) reset the live tracker the moment the song changes instead of letting it drift across songs, and (3) remember the tempo learned for a track and start the tracker with it on replay.

Sources (user chose both):
- **Desktop (Tauri, Linux): MPRIS over D-Bus.** No keys; covers the Spotify app, Spotify in a browser, VLC, etc.
- **Web: Spotify Web API "currently playing"**, using a PKCE login. This works for Spotify only, and the app is in development mode, so users must be added to its allowlist.

Limits found during exploration:
- **Key cannot be remembered yet.** Nothing in live mode detects a key; key detection exists only in offline analysis (`midzer-analyze` `chords::key`). Per-track memory therefore stores **BPM** (plus the live offset it was tracked with). Remembering key would mean adding live key detection first, which this plan does not cover.
- **Nothing exists to seed or reset the tracker.** There is no seed command and no dedicated reset, so this plan adds both.

## 1. Core: reset + tempo seed commands
- `crates/midzer-core/src/command.rs`: add `LiveReset` and `LiveSeed { bpm: f32 }` (encode/decode next to the other live variants, ~:91-104, :279-283, :367-371).
- `crates/midzer-core/src/engine.rs:207-222`:
  - `LiveReset` calls `live.forget()` with no Stop and no band-tempo emit. That is the difference from toggling `SetLive`.
  - `LiveSeed` calls `live.seed(bpm)`.
- `crates/midzer-core/src/live.rs`: `seed(bpm)` forwards to the tracker.
- `crates/midzer-core/src/tracker.rs`: add `BeatTracker::seed(bpm)`.
  - It sets `prior_bpm = bpm` and `prior_octaves = TOLD_OCTAVES`, the same mechanism `guide()`/`scale_tempo()` use at :503-545.
  - It must survive the `forget()` that comes right before it, so the order is reset first, then seed.
- `packages/audio-web/src/protocol.ts`: mirror the opcodes, the `EngineCommand` variants and the encoder (`liveReset`, `liveSeed`).
- Unit test in `tracker.rs`: after seeding 128, a 64-BPM-ambiguous click track locks at 128 rather than 64. Follow the existing tracker tests.

## 2. Neural listener reset
- `packages/audio-web/src/engine.ts`:
  - Add `resetLive()` to `EngineHandle`.
  - Web: post `{type:'reset'}` to the hear worker (the same message `openLive` already sends) and `send({type:'liveReset'})`.
- `packages/audio-web/src/native.ts` + `apps/desktop/src-tauri/src/lib.rs`:
  - Add a `live_reset` command that bumps `live_generation` in `crates/midzer-native/src/audio.rs`. The hear thread already resets when the generation changes (`hear.rs:43-48`).
  - Then send `liveReset` as normal.
- UI side: `followLiveChords(false/true)` clears the chord timeline (`session.ts:1100-1104`), and `resetLive()` in `apps/web/src/live.ts:62` resets the visuals.

## 3. Desktop source: MPRIS (Rust)
- New module `apps/desktop/src-tauri/src/nowplaying.rs` (or `crates/midzer-native/src/nowplaying.rs` if it fits better there).
  - It uses **`zbus`**, which is pure Rust. Do not use the `mpris`/`dbus` crates: they need libdbus-dev, which the build env lacks.
  - On a background thread with a blocking zbus connection:
    1. Find the `org.mpris.MediaPlayer2.*` names on the session bus.
    2. Prefer the one whose `PlaybackStatus` is `Playing`; Spotify wins ties.
    3. Watch `PropertiesChanged` on `/org/mpris/MediaPlayer2` and `NameOwnerChanged` for players that come and go.
  - It reads `xesam:title`, `xesam:artist`, `xesam:album`, `mpris:artUrl`, `mpris:length`, `mpris:trackid`, `PlaybackStatus` and `Position`.
- Add a `UiUpdate::NowPlaying(Option<NowPlaying>)` variant to the existing single Channel (`lib.rs:44-50`).
  - It is emitted only when the track id or play status changes.
  - The thread starts in `engine_start`, next to the other workers.
- Art: Spotify `artUrl`s are https (`i.scdn.co`). For `file://` art (local players), skip the image.
- `packages/audio-web/src/native.ts`: decode the new update and expose `onNowPlaying(listener)` on the handle. The web engine gets the same method, fed by the Spotify poller below.

## 4. Web source: Spotify Web API
- New `apps/web/src/nowplaying/spotify.ts`:
  - PKCE auth with `user-read-currently-playing` and `user-read-playback-state`.
  - The client ID comes from `import.meta.env.VITE_SPOTIFY_CLIENT_ID`. Without it, the feature is hidden.
  - The redirect URI is `http://127.0.0.1:5173/`; Spotify no longer accepts `localhost`.
  - Store the token and refresh token in localStorage under `midzer.spotify`.
- Poll `GET /v1/me/player/currently-playing` every 3 s, but only while Play along is in live mode.
  - It maps to the same `NowPlaying` shape: `{ id, title, artist, album, artUrl, durationMs, positionMs, playing }`.
  - On 401 it refreshes the token. On 204 the result is null.
- COEP is `require-corp`, so the API fetch must be CORS mode (Spotify sends CORS headers) and the cover `<img>` needs `crossOrigin="anonymous"`. Verify that `i.scdn.co` loads under COEP; if it fails, drop the art on web.
- Shared type: `packages/audio-web/src/nowplaying.ts`. Track key: `spotify:<id>`, or for MPRIS `mpris:<trackid>` with a fallback of `artist|title|length`. A Spotify MPRIS trackid is `spotify:track:…`, so normalise it to the same `spotify:<id>`, and memory then matches across web and desktop.

## 5. Session wiring (`apps/web/src/session.ts`)
- Add store fields `nowPlaying: NowPlaying | null` and `spotifyLinked: boolean`.
- Subscribe to `onNowPlaying` once the engine is up: MPRIS on desktop (`isNativeHost()`), the Spotify poller on web when linked.
- When the track key changes while live is open:
  1. Save the outgoing track's tempo (below).
  2. Call `engine.resetLive()` and clear the live chord timeline.
  3. If memory exists for the new key, send `liveSeed{bpm}` and restore the remembered offset if it differs from the source's.
- Saving: when the track changes or live closes, if `liveTracker.locked` held at confidence ≥ the existing "locked" threshold for ≥ 20 s, write `{ bpm, offset, updatedAt }`.
  - Also save the BPM after a manual ×2/½ fix (`BeatFixes`), because that is a strong signal.
- Storage: a new IndexedDB store `'tracks'` in `apps/web/src/storage/db.ts`. Add it to `StoreName`/`STORES` and bump `DB_VERSION` to 3.

## 6. UI (`apps/web/src/screens/PlayAlong.tsx`)
- A `NowPlaying` chip in the TopBar's empty middle slot (`TopBar.tsx:21`, passed as a prop on PlayAlong): small cover, title · artist, and a "remembered 128 BPM" badge when seeded. Follow the Stage/club styling.
- Web without a link: a "Connect Spotify" button in `SourcePicker` (`:233`), shown only when the client ID is set.

## Verification
- `cargo test -p midzer-core` covers the new seed/reset test and the command round-trip.
- Desktop, run with Spotify playing:
  1. The chip shows the track.
  2. Skipping the track resets the beat dots and chords immediately.
  3. Replaying a learned track shows the badge and locks at the remembered BPM faster.
  4. Also check browser Spotify (Chrome MPRIS) and pausing.
- Web: set `VITE_SPOTIFY_CLIENT_ID`, link the account, capture the Spotify web player tab, and repeat steps 1–3. Check that the cover art loads under COEP.
- Rebuild wasm + web before e2e (see memory). Add a Playwright test that stubs `onNowPlaying` and asserts the chip renders and `resetLive` fires on a key change.
- Live-tracker regressions: run `live_eval` with no seed and confirm the numbers are unchanged, since seed is opt-in.
