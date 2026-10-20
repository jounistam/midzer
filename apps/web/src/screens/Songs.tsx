import { STYLES } from '@midzer/audio-web';
import { useRef, useState, type DragEvent } from 'react';
import { TopBar } from '../components/TopBar';
import { goTo, openSong } from '../session';
import { BUILTIN_SONGS } from '../songs/builtin';
import { importFile, importText, type Imported } from '../songs/importing';
import { deleteUserSong, loadUserSongs, saveUserSong } from '../songs/library';
import { newSongId, parseChart, type Song } from '../songs/song';
import { LoadingStatus } from './LoadingStatus';

const ACCEPT = '.mid,.midi,.cho,.chopro,.chordpro,.crd,.pro,.txt';

export function Songs() {
  const [mine, setMine] = useState(loadUserSongs);
  const [pasting, setPasting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  /** Saves an import to My songs and opens it with its notes for the user. */
  const open = ({ song, notes }: Imported) => {
    saveUserSong(song);
    setMine(loadUserSongs());
    openSong(song, notes);
  };
  const fromFile = async (file: File | undefined) => {
    if (!file) return;
    setError(null);
    try {
      open(await importFile(file));
    } catch (e) {
      setError(`${file.name}: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    void fromFile(e.dataTransfer.files[0]);
  };
  const newSong = () => {
    const song: Song = {
      id: newSongId(),
      title: 'New song',
      bpm: 100,
      style: 0,
      bars: parseChart('[A] C | Am | F | G').bars,
      source: 'user',
    };
    open({ song, notes: [] });
  };
  const remove = (song: Song) => {
    if (!window.confirm(`Delete “${song.title}”?`)) return;
    deleteUserSong(song.id);
    setMine(loadUserSongs());
  };

  return (
    <div className="songs-screen" onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <TopBar title="Songs" onBack={() => goTo('home')} />
      <main className="songs-main">
        <div className="songs-head">
          <div>
            <h1>Songs</h1>
            <p className="lead">The band plays the chart while you play on top. Drop a MIDI or ChordPro file anywhere here.</p>
          </div>
          <div className="actions">
            <button className="btn btn-primary" onClick={newSong}>
              New song
            </button>
            <button className="btn btn-secondary" onClick={() => fileInput.current?.click()}>
              Import file
            </button>
            <button className="btn btn-secondary" onClick={() => setPasting(true)}>
              Paste chords
            </button>
            <input
              ref={fileInput}
              type="file"
              accept={ACCEPT}
              hidden
              aria-label="Import a MIDI or ChordPro file"
              onChange={(e) => {
                void fromFile(e.target.files?.[0]);
                e.target.value = '';
              }}
            />
          </div>
        </div>
        {error && (
          <p className="status-line error" role="alert">
            {error}
          </p>
        )}
        <LoadingStatus />

        {mine.length > 0 && (
          <>
            <h2 className="label songs-group">My songs</h2>
            <SongList songs={mine} onRemove={remove} />
          </>
        )}
        <h2 className="label songs-group">Built in</h2>
        <SongList songs={BUILTIN_SONGS} />
      </main>
      {pasting && <PasteDialog onClose={() => setPasting(false)} onImport={open} />}
    </div>
  );
}

function SongList({ songs, onRemove }: { songs: Song[]; onRemove?: (song: Song) => void }) {
  return (
    <ul className="song-list">
      {songs.map((song) => (
        <li key={song.id} className="song-card">
          <button className="song-open" onClick={() => openSong(song)}>
            <span className="song-card-title">{song.title}</span>
            {song.artist && <span className="song-card-artist">{song.artist}</span>}
            <span className="song-card-meta">
              {STYLES[song.style]?.name} · {song.bpm} BPM · {song.bars.length} bars
            </span>
          </button>
          {onRemove && (
            <button className="btn btn-ghost btn-icon" aria-label={`Delete ${song.title}`} onClick={() => onRemove(song)}>
              ×
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

function PasteDialog({ onClose, onImport }: { onClose: () => void; onImport: (i: Imported) => void }) {
  const [text, setText] = useState('');
  const [title, setTitle] = useState('');
  const [error, setError] = useState<string | null>(null);
  const submit = () => {
    try {
      onImport(importText(text, title.trim() || undefined));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  return (
    <div className="drawer-backdrop centered" onClick={onClose}>
      <div className="dialog paste-dialog" role="dialog" aria-label="Paste chords" onClick={(e) => e.stopPropagation()}>
        <h2>Paste chords</h2>
        <p className="setting-note">
          ChordPro (<code>[Am]words [F]here</code>) or a chart: <code>[Verse] C | Am | F G | C</code>.
        </p>
        <label className="field">
          <span className="label">Title</span>
          <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Taken from the text if it has one" />
        </label>
        <textarea
          className="chart-text"
          rows={10}
          value={text}
          aria-label="Chords"
          spellCheck={false}
          autoFocus
          onChange={(e) => setText(e.target.value)}
        />
        {error && (
          <p className="status-line error" role="alert">
            {error}
          </p>
        )}
        <div className="actions">
          <button className="btn btn-primary" onClick={submit} disabled={!text.trim()}>
            Open song
          </button>
          <button className="btn btn-ghost" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
