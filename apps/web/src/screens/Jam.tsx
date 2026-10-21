import { ChordDisplay } from '../components/ChordDisplay';
import { ControlPanel } from '../components/ControlPanel';
import { DrummerStage } from '../components/DrummerStage';
import { Keyboard } from '../components/Keyboard';
import { KitView } from '../components/KitView';
import { TopBar } from '../components/TopBar';
import { goTo, useSession } from '../session';
import { LoadingStatus } from './LoadingStatus';

export function Jam() {
  const split = useSession((s) => s.split);
  const drums = useSession((s) => s.instrument === 'drums');
  return (
    <div className="jam">
      <TopBar title="Jam" onBack={() => goTo('home')} />
      <main className="jam-main">
        <div className="jam-stage">
          {drums ? (
            <DrummerStage />
          ) : (
            <div className="panel">
              <ChordDisplay />
            </div>
          )}
          <LoadingStatus />
        </div>
        <ControlPanel drummer={drums} />
      </main>
      {drums ? <KitView height={170} /> : <Keyboard split={split} />}
    </div>
  );
}
