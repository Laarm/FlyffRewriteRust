// React root (Plan/40 section 2.1, WP-CLIENT-05): switches the screen root by `ScreenState` (rule SM1: the
// state machine decides the state, doc 45 renders the body). Screen bodies register with `ctx.screens`; until
// one is registered a placeholder shows the state name. `Boot` shows the CSS-only boot screen with the status
// of the boot steps; `Fatal` shows the L4 screen, which needs no game assets.

import { useStore } from 'zustand';
import { defineLoadingStore, LoadingLayer } from '../features/loading';
import { UiLayer } from '../features/session';
import { StoresProvider, useStoreSel } from '../stores/framework';
import type { ScreenState } from '../stores/screen';
import type { BootStatusStore } from './bootStatus';
import type { ClientContext } from './context';
import { FatalScreen } from './FatalScreen';

export interface RootProps {
  readonly ctx: ClientContext;
  readonly status: BootStatusStore;
  /** Fatal screen button "Reload" (default `location.reload()`; the recovery ladder overrides it). */
  readonly onReload?: () => void;
  /** Fatal screen button "Back to login" (default: reload; the recovery ladder drops the token first). */
  readonly onBackToLogin?: () => void;
}

export function Root({ ctx, status, onReload, onBackToLogin }: RootProps) {
  return (
    <StoresProvider stores={ctx.stores}>
      <ScreenSwitch
        ctx={ctx}
        status={status}
        onReload={onReload ?? reloadPage}
        onBackToLogin={onBackToLogin ?? reloadPage}
      />
      <UiLayer ctx={ctx} />
      <LoadingLayer ctx={ctx} store={defineLoadingStore(ctx.stores.registry)} />
    </StoresProvider>
  );
}

function reloadPage(): void {
  location.reload();
}

interface SwitchProps {
  readonly ctx: ClientContext;
  readonly status: BootStatusStore;
  readonly onReload: () => void;
  readonly onBackToLogin: () => void;
}

function ScreenSwitch({ ctx, status, onReload, onBackToLogin }: SwitchProps) {
  const screen = useStoreSel(ctx.stores.sessionStore, (s) => s.screen);
  const bodies = useStore(ctx.screens.store, (s) => s.bodies);
  const overlay = useStoreSel(ctx.stores.sessionStore, (s) => s.overlay);

  if (screen.s === 'Boot') return <BootScreen ctx={ctx} status={status} />;
  if (screen.s === 'Fatal') {
    return (
      <FatalScreen
        reason={screen.error.reason}
        buildId={ctx.build.id}
        logLines={ctx.log.lines(50)}
        onReload={onReload}
        onBackToLogin={onBackToLogin}
      />
    );
  }
  if (screen.s === 'Reloading' && bodies.Reloading === undefined) {
    return (
      <main className="reloading" data-testid="screen" data-screen="Reloading">
        <p>Reloading...</p>
      </main>
    );
  }
  const Body = bodies[screen.s];
  if (Body) {
    return (
      <div data-testid="screen" data-screen={screen.s} data-overlay={overlay}>
        <Body ctx={ctx} />
      </div>
    );
  }
  return <ScreenPlaceholder screen={screen} overlay={overlay} />;
}

/** What a screen shows until doc 45 registers its body. */
function ScreenPlaceholder({ screen, overlay }: { screen: ScreenState; overlay: string }) {
  return (
    <main
      className="screen-placeholder"
      data-testid="screen"
      data-screen={screen.s}
      data-overlay={overlay}
    >
      <h1>Flyff V15</h1>
      <p data-testid="screen-name">{screen.s}</p>
      {screen.s === 'Login' && screen.notice && (
        <p data-testid="screen-notice">
          {screen.notice.text ?? `message ${String(screen.notice.tid)}`}
        </p>
      )}
      {overlay !== 'none' && <p data-testid="screen-overlay">{overlay}</p>}
    </main>
  );
}

function BootScreen({ ctx, status }: { ctx: ClientContext; status: BootStatusStore }) {
  const s = useStore(status);
  return (
    <main className="boot" data-testid="boot" data-phase={s.phase}>
      <h1>Flyff V15</h1>
      {s.phase === 'starting' && <div className="boot__spinner" role="presentation" />}
      <p data-testid="build-id">build {ctx.build.id}</p>
      <ul className="boot__status">
        <li data-testid="react-status" data-state="ok">
          React: ok
        </li>
        <li data-testid="bootstrap-status" data-state={s.bootstrap.state}>
          Server: {s.bootstrap.detail}
        </li>
        <li data-testid="wasm-status" data-state={s.wasm.state}>
          WASM: {s.wasm.detail}
        </li>
        <li data-testid="worker-status" data-state={s.worker.state}>
          Worker: {s.worker.detail}
        </li>
        <li data-testid="data-status" data-state={s.data.state}>
          Data: {s.data.detail}
        </li>
        <li data-testid="fonts-status" data-state={s.fonts.state}>
          Fonts: {s.fonts.detail}
        </li>
      </ul>
    </main>
  );
}
