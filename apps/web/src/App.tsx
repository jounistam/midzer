import { useEffect } from 'react';
import { Home } from './screens/Home';
import { Jam } from './screens/Jam';
import { Onboarding } from './screens/Onboarding';
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
  }
}
