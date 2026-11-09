// The shell's own subscription on the tab's live socket: the 1 Hz status the top bar shows (Plan/25 section 11.4 `StatusSubscribe`, `view.dashboard`).
// While no tick is fresh the pills use the polled `GET /admin/api/status` (app/TopBar.tsx), so a socket that cannot connect (an old proxy without the
// WebSocket upgrade) costs the live feel, not the pills. A refusal (an account without `view.dashboard`) simply leaves the subscription empty.
import { useEffect } from 'react';
import { adminSocket, useLive } from '../stores/live';
import { useStatus } from '../stores/status';

export function useStatusFeed(enabled = true): void {
  // a new socket (the session ended and the owner of the host took over) needs the subscription again
  const epoch = useLive((s) => s.epoch);
  useEffect(() => {
    if (!enabled) return;
    const sub = adminSocket().subscribeStatus(
      (tick) => {
        useStatus.getState().applyTick(tick);
      },
      {
        onError: () => {
          useStatus.setState({ live: null });
        },
      },
    );
    return () => {
      sub.close();
    };
  }, [enabled, epoch]);
}
