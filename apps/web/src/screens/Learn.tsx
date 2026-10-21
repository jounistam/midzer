import { STYLES } from '@midzer/audio-web';
import { useEffect, useState } from 'react';
import { Stars } from '../components/Stars';
import { TopBar } from '../components/TopBar';
import { isUnlocked, LESSONS, nextLesson, units, type Lesson } from '../learn/lessons';
import { openLesson, openPractice, refreshProgress, usePractice } from '../learn/practice';
import { songProgressId } from '../learn/progress';
import { PART_LABELS, partsOf, type Part } from '../learn/targets';
import { goTo, startAudio, useSession } from '../session';
import { BUILTIN_SONGS } from '../songs/builtin';
import { loadUserSongs } from '../songs/library';
import type { Song } from '../songs/song';
import { LoadingStatus } from './LoadingStatus';

export function Learn() {
  const progress = usePractice((s) => s.progress);
  const audio = useSession((s) => s.audio);
  const [mine, setMine] = useState<Song[]>([]);

  useEffect(() => {
    let live = true;
    void refreshProgress();
    void loadUserSongs().then((songs) => live && setMine(songs));
    return () => {
      live = false;
    };
  }, []);

  /** Starts audio on the first click, as browsers require. */
  const start = (fn: () => void) => {
    if (audio === 'idle' || audio === 'error') void startAudio();
    fn();
  };
  const suggested = nextLesson(progress);
  const songs = [...mine, ...BUILTIN_SONGS];

  return (
    <div className="songs-screen learn-screen">
      <TopBar title="Learn" onBack={() => goTo('home')} />
      <main className="songs-main">
        <div className="songs-head">
          <div>
            <h1>Learn</h1>
            <p className="lead">Notes fall onto the keys you play. In wait mode the band holds until you get them.</p>
          </div>
          {suggested && (
            <div className="actions">
              <button className="btn btn-primary" onClick={() => start(() => openLesson(suggested))}>
                {suggested === LESSONS[0] ? 'Start' : 'Continue'}: {suggested.title}
              </button>
            </div>
          )}
        </div>
        <LoadingStatus />

        {units().map(({ unit, lessons }) => (
          <section key={unit} className="unit" aria-label={unit}>
            <h2 className="label songs-group">{unit}</h2>
            <ol className="lesson-path">
              {lessons.map((lesson) => (
                <LessonCard
                  key={lesson.id}
                  lesson={lesson}
                  number={LESSONS.indexOf(lesson) + 1}
                  stars={progress.get(lesson.id)?.stars ?? 0}
                  locked={!isUnlocked(lesson.id, progress)}
                  suggested={lesson === suggested}
                  onOpen={() => start(() => openLesson(lesson))}
                />
              ))}
            </ol>
          </section>
        ))}

        <h2 className="label songs-group">Practise a song</h2>
        <ul className="song-list">
          {songs.map((song) => (
            <li key={song.id} className="song-card practice-card">
              <div className="song-open static">
                <span className="song-card-title">{song.title}</span>
                {song.artist && <span className="song-card-artist">{song.artist}</span>}
                <span className="song-card-meta">
                  {STYLES[song.style]?.name} · {song.bpm} BPM · {song.bars.length} bars
                </span>
              </div>
              <div className="part-buttons">
                {partsOf(song).map((part: Part) => (
                  <button
                    key={part}
                    className="btn btn-secondary part-button"
                    aria-label={`Practise ${song.title}: ${PART_LABELS[part]}`}
                    onClick={() => start(() => openPractice(song, part))}
                  >
                    {PART_LABELS[part]}
                    <Stars count={progress.get(songProgressId(song.id, part))?.stars ?? 0} small />
                  </button>
                ))}
              </div>
            </li>
          ))}
        </ul>
      </main>
    </div>
  );
}

function LessonCard(props: { lesson: Lesson; number: number; stars: number; locked: boolean; suggested: boolean; onOpen: () => void }) {
  const { lesson, number, stars, locked, suggested, onOpen } = props;
  return (
    <li>
      <button
        className={`lesson-card ${locked ? 'locked' : ''} ${suggested ? 'suggested' : ''}`}
        disabled={locked}
        onClick={onOpen}
        aria-label={`Lesson ${number}: ${lesson.title}${locked ? ', locked' : `, ${stars} of 3 stars`}`}
      >
        <span className="lesson-number" aria-hidden>
          {number}
        </span>
        <span className="lesson-text">
          <span className="lesson-title">{lesson.title}</span>
          <span className="lesson-blurb">{locked ? 'Get a star in the lesson before to open this one.' : lesson.blurb}</span>
        </span>
        <span className="lesson-meta">
          <span className="badge">{PART_LABELS[lesson.part]}</span>
          <Stars count={stars} />
        </span>
      </button>
    </li>
  );
}
