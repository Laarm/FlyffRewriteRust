// The DPS meter (`features.dps_meter`, Plan/45 §7.10, 17 §10; WP-CFEAT-36): `DpsUpdate` mode 0 and 1 replace the list (sorted by damage),
// mode 2 shows or hides the meter (hiding empties the list). The overlay (`DpsMeter.tsx`) is drawn at the left edge when the meter is shown
// and the list is not empty; its rows are recomputed when the list changes and four times a second (a player comes into view, changes the job).
// With `features.dps_meter` off the messages are ignored and nothing is drawn (both branches are tested).

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../../app/context';
import { EntityKind } from '../../../game/entities';
import type { SpawnPlayer } from '../../../gen/proto/flyff/v1/world_pb';
import { type FeatureContext, type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { definePvpStore, type DpsEntry } from '../../pvp/store';
import { DpsMeter, DpsState } from './DpsMeter';
import { dpsModel, type DpsEnv, type DpsModel, sortDps } from './model';

/** The rows are recomputed at most this often (a player enters the view, a job changes). */
export const DPS_REFRESH_MS = 250;

/** The public face of the DPS meter of a context. */
export interface DpsFeature {
  readonly state: DpsState;
  /** The overlay model now (`null` when the meter is hidden). */
  model(): DpsModel | null;
  /** Recomputes the model now. */
  refresh(): void;
  /** Unmounts the overlay (tests; the page never does). */
  dispose(): void;
}

const features = new WeakMap<ClientContext, DpsFeature>();
export function dpsOf(ctx: ClientContext): DpsFeature | undefined {
  return features.get(ctx);
}

export const dpsModule: FeatureModule = {
  id: 'hud',
  milestone: 5,
  register(fc) {
    registerDps(fc);
  },
};

/** Mounts the overlay container between the world canvas and the UI; returns the unmount. */
export function mountDps(ctx: ClientContext, state: DpsState): () => void {
  const el = document.createElement('div');
  el.dataset['testid'] = 'dps-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const st = fullUi(ctx)?.scaler.get();
    root.render(
      createElement(DpsMeter, {
        state,
        viewport: st?.virtual ?? { width: 1024, height: 768 },
        scale: st?.scale ?? 1,
      }),
    );
  };
  render();
  const off = fullUi(ctx)?.scaler.subscribe(render);
  return () => {
    off?.();
    root.unmount();
    el.remove();
  };
}

function registerDps(fc: FeatureContext): DpsFeature {
  const ctx = fc.app;
  const store = definePvpStore(ctx.stores.registry);
  const state = new DpsState();
  /** `JobChanged` after the spawn (the spawn message carries the job at that time). */
  const jobs = new Map<number, number>();
  fc.net.on('jobChanged', (m, objId) => {
    if (objId !== 0) jobs.set(objId, m.job);
  });
  ctx.entities.onRemove((rec) => {
    jobs.delete(rec.objId);
  });

  const env: DpsEnv = {
    jobOf: (idPlayer) => {
      let job: number | null = null;
      ctx.entities.forEach(EntityKind.Player, (rec) => {
        if (job !== null || rec.removed || rec.idPlayer !== idPlayer) return;
        job = rec.isSelf
          ? ctx.stores.selfStore.getState().job
          : (jobs.get(rec.objId) ?? (rec.spawn as SpawnPlayer).player?.job ?? 0);
      });
      return job;
    },
  };

  const compute = (): DpsModel | null => {
    if (!ctx.features.features.dps_meter) return null;
    const { show, entries } = store.getState().dps;
    return dpsModel(entries, show, env);
  };
  const refresh = (): void => {
    state.set(compute());
  };

  fc.net.on('dpsUpdate', (m) => {
    if (!ctx.features.features.dps_meter) return;
    const s = store.getState().dps;
    const sub = m.sub;
    if (sub.case === 'dps' || sub.case === 'totals') {
      const entries: DpsEntry[] = sub.value.entries.map((e) => ({
        idPlayer: e.idPlayer,
        name: e.name,
        damage: e.value,
      }));
      store.setState({ dps: { show: s.show, entries: sortDps(entries) } });
    } else if (sub.case === 'show') {
      // hiding empties the list (`m_aDPSPlayer.clear()`)
      store.setState({ dps: { show: sub.value.show, entries: sub.value.show ? s.entries : [] } });
    }
    refresh();
  });
  // the store is reset with the world: the overlay follows
  store.subscribe(refresh);
  let last = Number.NEGATIVE_INFINITY;
  ctx.tickSystems.register('hud.dps', () => {
    const now = ctx.now();
    if (now - last < DPS_REFRESH_MS) return;
    last = now;
    if (state.model !== null) refresh();
  });

  // the overlay is mounted with the first list to show (title screens and tests without a UI pay nothing)
  let unmount: (() => void) | null = null;
  if (typeof document !== 'undefined') {
    state.subscribe(() => {
      if (unmount === null && state.model !== null) unmount = mountDps(ctx, state);
    });
  }
  const feature: DpsFeature = {
    state,
    model: () => state.model,
    refresh,
    dispose: () => {
      unmount?.();
      unmount = null;
    },
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'dps', () => {
    const { show, entries } = store.getState().dps;
    return { show, entries, model: state.model };
  });
  return feature;
}
