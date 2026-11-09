// The React bodies of the screen states of this feature (Plan/45 §1.6, rule SM1): the title states are the windows of
// `ui/windows/login` (opened by the screen controller), so their body is only a visually hidden status line (the probe of
// the Playwright specs and screen readers read it); `Reconnecting` and `Reloading` draw their overlay lines (45 §3.4).

import type { CSSProperties } from 'react';
import type { ClientContext } from '../../app/context';
import { useStoreSel } from '../../stores/framework';

const HIDDEN: CSSProperties = {
  position: 'absolute',
  width: 1,
  height: 1,
  margin: -1,
  padding: 0,
  overflow: 'hidden',
  clip: 'rect(0 0 0 0)',
  whiteSpace: 'nowrap',
  border: 0,
};

/** Login, Connecting, CharSelect, CharCreate, Entering, Leaving, InGame, Loading: the windows are the UI. */
export function LobbyBody({ ctx }: { ctx: ClientContext }) {
  const screen = useStoreSel(ctx.stores.sessionStore, (s) => s.screen);
  const overlay = useStoreSel(ctx.stores.sessionStore, (s) => s.overlay);
  return (
    <div style={HIDDEN} data-testid="screen-status">
      <h1>Flyff V15</h1>
      <span data-testid="screen-name">{screen.s}</span>
      {screen.s === 'Login' && screen.notice && (
        <span data-testid="screen-notice">
          {screen.notice.text ?? `message ${String(screen.notice.tid)}`}
        </span>
      )}
      {overlay !== 'none' && <span data-testid="screen-overlay">{overlay}</span>}
    </div>
  );
}

const OVERLAY: CSSProperties = {
  position: 'fixed',
  inset: 0,
  zIndex: 99500,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  background: 'rgba(0, 0, 0, 0.55)',
  color: '#fff',
  font: '16px "Flyff DejaVu Sans", sans-serif',
  textShadow: '1px 1px 0 #000',
};

/** `Reconnecting`: "Reconnecting..." over the last frame; the windows stay open (45 §3.4, 12 §13.3). */
export function ReconnectingBody({ ctx }: { ctx: ClientContext }) {
  const screen = useStoreSel(ctx.stores.sessionStore, (s) => s.screen);
  const attempt = screen.s === 'Reconnecting' ? screen.attempt : 0;
  return (
    <div style={OVERLAY} role="alert" data-testid="reconnecting">
      <span data-testid="screen-name" style={{ display: 'none' }}>
        {screen.s}
      </span>
      <p>{attempt > 1 ? `Reconnecting... (attempt ${String(attempt)})` : 'Reconnecting...'}</p>
    </div>
  );
}

/** `Reloading`: a full-screen "Reloading..." line (the page reloads, 45 §3.4). */
export function ReloadingBody() {
  return (
    <main
      className="reloading"
      data-testid="reloading"
      role="status"
      style={{ ...OVERLAY, background: '#101418' }}
    >
      <p>Reloading...</p>
    </main>
  );
}
