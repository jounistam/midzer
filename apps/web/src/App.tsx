import { useEffect } from 'react';
import { Home } from './screens/Home';
import { Jam } from './screens/Jam';
import { Learn } from './screens/Learn';
import { Lesson } from './screens/Lesson';
import { Onboarding } from './screens/Onboarding';
import { PlayAlong } from './screens/PlayAlong';
import { Songs } from './screens/Songs';
import { SongScreen } from './screens/SongScreen';
import { attachQwerty, useSession } from './session';

export function App() {
  const screen = useSession((s) => s.screen);
  const started = useSession((s) => s.audio !== 'idle');

  useEffect(() => (started ? attachQwerty() : undefined), [started]);

  switch (screen) {
    case 'onboarding':
      return <Onboarding />;
    case 'home':
      return <Home />;
    case 'jam':
      return <Jam />;
    case 'songs':
      return <Songs />;
    case 'song':
      return <SongScreen />;
    case 'learn':
      return <Learn />;
    case 'lesson':
      return <Lesson />;
    case 'along':
      return <PlayAlong />;
  }
}
