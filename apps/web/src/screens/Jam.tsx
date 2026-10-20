import { ChordDisplay } from '../components/ChordDisplay';
import { ControlPanel } from '../components/ControlPanel';
import { Keyboard } from '../components/Keyboard';
import { TopBar } from '../components/TopBar';
import { goTo, useSession } from '../session';
import { LoadingStatus } from './LoadingStatus';

export function Jam() {
  const split = useSession((s) => s.split);
  return (
    <div className="jam">
      <TopBar title="Jam" onBack={() => goTo('home')} />
      <main className="jam-main">
        <div className="jam-stage">
          <div className="panel">
            <ChordDisplay />
          </div>
          <LoadingStatus />
        </div>
        <ControlPanel />
      </main>
      <Keyboard split={split} />
    </div>
  );
}
