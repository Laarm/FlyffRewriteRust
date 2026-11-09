// The `loading` feature module (Plan/45 §6, WP-CFEAT-05).
import type { FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import { createLoadingController, defineLoadingStore } from './loadingController';

export const loadingModule: FeatureModule = {
  id: 'loading',
  milestone: 2,
  register(fc) {
    const store = defineLoadingStore(fc.app.stores.registry);
    const controller = createLoadingController(fc, store);
    fc.app.machine.subscribe((screen, previous) => {
      controller.onScreen(screen, previous);
    });
    addProbeMember(fc.app, 'loading', () => {
      const s = store.getState();
      return {
        phase: s.phase,
        alpha: s.alpha,
        image: s.image?.file ?? null,
        worldId: s.worldId,
        reason: s.reason,
      };
    });
  },
};

export { LoadingLayer, preparingText } from './LoadingLayer';
export {
  createLoadingController,
  defineLoadingStore,
  loadingImageUrl,
  themePath,
} from './loadingController';
export type { LoadingController, LoadingState, LoadingStore } from './loadingController';
export {
  FADE_STEP,
  fadeStep,
  isGuildHouseWorld,
  loadingImageFor,
  RANDOM_LOADING_IMAGES,
} from './images';
export type { LoadingImage, LoadingImageKind } from './images';
