// The loading screen and the "Preparing assets" line (Plan/45 §6, F-318, PL-130): a full-screen layer above the UI. The
// image is letter-boxed (aspect kept, black bars) over the whole viewport; while the state is `Loading` the image is
// opaque, after the gate opened it fades out (`LoadingState.alpha`). The preparing line follows `sessionStore.prepare`
// (`active` = a wave is running AND an own request has been pending for more than a second): under the image while loading,
// in a corner panel in game; it disappears with `finished`. Login is never blocked by it.

import { useStore } from 'zustand';
import type { ClientContext } from '../../app/context';
import { useStoreSel } from '../../stores/framework';
import type { PrepareState } from '../../stores/session';
import { loadingImageUrl, type LoadingStore } from './loadingController';

/** The text of the line (a new English text, 45 §6.2). */
export function preparingText(p: Pick<PrepareState, 'percent' | 'kind'>): string {
  const detail = p.kind === '' ? '' : ` (${p.kind})`;
  return `Preparing assets... ${String(Math.max(0, Math.min(100, Math.round(p.percent))))} %${detail}`;
}

export function LoadingLayer({ ctx, store }: { ctx: ClientContext; store: LoadingStore }) {
  const s = useStore(store);
  const prepare = useStoreSel(ctx.stores.sessionStore, (st) => st.prepare);
  const screen = useStoreSel(ctx.stores.sessionStore, (st) => st.screen.s);
  const showPrepare = prepare?.active === true;
  if (s.phase === 'hidden') {
    // in game (or on the title screens) the line is a small panel in a corner
    if (!showPrepare) return null;
    return (
      <div
        className="preparing-panel"
        data-testid="preparing"
        role="status"
        style={{
          position: 'fixed',
          right: 8,
          bottom: 56,
          zIndex: 99000,
          padding: '4px 8px',
          background: 'rgba(0,0,0,0.7)',
          color: '#fff',
          font: '12px "Flyff DejaVu Sans", sans-serif',
          pointerEvents: 'none',
        }}
      >
        {preparingText(prepare)}
      </div>
    );
  }
  const url = loadingImageUrl(ctx, s.image);
  return (
    <div
      className="loading-screen"
      data-testid="loading-screen"
      data-phase={s.phase}
      data-reason={s.reason ?? ''}
      data-image={s.image?.file ?? ''}
      data-world={s.worldId}
      data-state={screen}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 100000,
        background: '#000',
        opacity: s.alpha / 255,
        pointerEvents: s.phase === 'showing' ? 'auto' : 'none',
      }}
    >
      {url !== null && (
        <img
          alt=""
          src={url}
          data-testid="loading-image"
          style={{ width: '100%', height: '100%', objectFit: 'contain', display: 'block' }}
        />
      )}
      {showPrepare && (
        <p
          data-testid="preparing"
          role="status"
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            bottom: 24,
            margin: 0,
            textAlign: 'center',
            color: '#fff',
            textShadow: '1px 1px 0 #000',
            font: '14px "Flyff DejaVu Sans", sans-serif',
          }}
        >
          {preparingText(prepare)}
        </p>
      )}
    </div>
  );
}
