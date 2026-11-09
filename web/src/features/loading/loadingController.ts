// The loading screen controller (Plan/45 §6.2, WP-CFEAT-05; F-318, PL-130): doc 40's gate decides when `InGame` starts; this
// module shows the loading image while the state is `Loading{reason}`, fades it out after the gate opened (legacy
// `m_nAlpha -= 6` per rendered frame) and closes the transactional windows at a world change (`Replace`).
//
//  * `Loading{enter | reattach}`: the image appears at once (random of `MapLoading2..4`, or the instance / siege / guild-house
//    image of the destination world),
//  * `Loading{worldChange}` (`Replace{world_id}`, 15 W13): FIRST `closeTransactional('WorldChange')` (legacy
//    `CloseBoundWindow`), then the image immediately, before any loading starts; the same-world `SetPos` shows no screen,
//  * `Loading -> InGame`: the image fades 6 alpha per frame, `1` frame = one `requestAnimationFrame`.

import type { StoreApi } from 'zustand/vanilla';
import type { ClientContext } from '../../app/context';
import type { ScreenState } from '../../stores/screen';
import type { StoreRegistry } from '../../stores/registry';
import type { FeatureContext } from '../common/module';
import { fadeStep, isGuildHouseWorld, loadingImageFor, type LoadingImage } from './images';

export type LoadingPhase = 'hidden' | 'showing' | 'fading';

export interface LoadingState {
  readonly phase: LoadingPhase;
  /** 0-255: the opacity of the image (255 while loading). */
  readonly alpha: number;
  readonly image: LoadingImage | null;
  readonly worldId: number;
  readonly reason: 'enter' | 'worldChange' | 'reattach' | null;
}

export const initialLoadingState = (): LoadingState => ({
  phase: 'hidden',
  alpha: 0,
  image: null,
  worldId: 0,
  reason: null,
});

export type LoadingStore = StoreApi<LoadingState>;

export function defineLoadingStore(registry: StoreRegistry): LoadingStore {
  const existing = registry.get('loading');
  if (existing) return existing as unknown as LoadingStore;
  return registry.defineStore<LoadingState>({
    name: 'loading',
    scope: 'never',
    owner: '45',
    initial: initialLoadingState,
  });
}

/** The theme file as an asset key path (`theme/maploading2.tga`). */
export function themePath(file: string): string {
  return `theme/${file.toLowerCase()}`;
}

/** The URL of a loading image through the asset manifest (`null` when it is not client-visible). */
export function loadingImageUrl(ctx: ClientContext, image: LoadingImage | null): string | null {
  if (!image) return null;
  return ctx.assetLayer.source.url({ ns: 'res', path: themePath(image.file) });
}

export interface LoadingControllerDeps {
  readonly requestFrame?: (fn: () => void) => unknown;
  readonly cancelFrame?: (id: unknown) => void;
  readonly random?: () => number;
}

export interface LoadingController {
  readonly store: LoadingStore;
  onScreen(screen: ScreenState, previous: ScreenState | null): void;
  /** Runs one fade frame now (tests). */
  fadeFrame(): void;
  dispose(): void;
}

export function createLoadingController(
  fc: FeatureContext,
  store: LoadingStore,
  deps: LoadingControllerDeps = {},
): LoadingController {
  const ctx = fc.app;
  const request = deps.requestFrame ?? ((fn: () => void) => requestAnimationFrame(fn));
  const cancel =
    deps.cancelFrame ??
    ((id: unknown) => {
      cancelAnimationFrame(id as number);
    });
  let raf: unknown = null;

  const stopFade = (): void => {
    if (raf !== null) cancel(raf);
    raf = null;
  };

  const fadeFrame = (): void => {
    raf = null;
    const s = store.getState();
    if (s.phase !== 'fading') return;
    const alpha = fadeStep(s.alpha);
    if (alpha <= 0) {
      store.setState({ phase: 'hidden', alpha: 0 });
      return;
    }
    store.setState({ alpha });
    raf = request(fadeFrame);
  };

  const hide = (): void => {
    stopFade();
    if (store.getState().phase !== 'hidden') store.setState(initialLoadingState());
  };

  return {
    store,
    fadeFrame,
    onScreen(screen, previous) {
      if (screen.s === 'Loading') {
        stopFade();
        if (screen.reason === 'worldChange' && previous?.s === 'InGame') {
          // the legacy `CloseBoundWindow` + the end of housing placement: every transactional window first
          fc.transactional.closeTransactional('WorldChange');
        }
        const own =
          isGuildHouseWorld(screen.worldId) && ctx.stores.selfStore.getState().guildId !== 0;
        // keep the image of this loading while only the world id of the same Loading state changed (WorldReadInfo)
        const prev = store.getState();
        const keep =
          prev.phase === 'showing' &&
          prev.reason === screen.reason &&
          prev.worldId === screen.worldId;
        if (keep) return;
        const reuseRandom =
          prev.phase === 'showing' &&
          prev.reason === screen.reason &&
          prev.image?.kind === 'random' &&
          loadingImageFor(screen.worldId, { ownGuildHouse: own }).kind === 'random';
        store.setState({
          phase: 'showing',
          alpha: 255,
          image: reuseRandom
            ? prev.image
            : loadingImageFor(screen.worldId, {
                ownGuildHouse: own,
                ...(deps.random ? { random: deps.random } : {}),
              }),
          worldId: screen.worldId,
          reason: screen.reason,
        });
        return;
      }
      if (
        screen.s === 'InGame' &&
        previous?.s === 'Loading' &&
        store.getState().phase === 'showing'
      ) {
        // the gate opened: fade the image out, 6 alpha per rendered frame
        store.setState({ phase: 'fading' });
        raf = request(fadeFrame);
        return;
      }
      if (screen.s === 'Reconnecting' || screen.s === 'Leaving') return; // the image is not involved
      if (screen.s !== 'InGame') hide();
    },
    dispose: stopFade,
  };
}
