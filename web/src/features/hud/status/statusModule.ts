// The `status` feature module (Plan/45 §7.2, §7.3; WP-CFEAT-09): the ports of the status window (`ui.char.setPorts({status})`:
// the gauges, the display texts and the EXP stop button) and the target frame under the render target (a DOM overlay between the
// world canvas and the windows, `hud/targetFrame/` (the plan names it `target/`, which `.gitignore` swallows)).

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../../app/context';
import { TID_GAME_SELECT_OBJECT_INFORMATION_MONSTER } from '../../../gen/defs/text';
import { peekRulesRuntime } from '../../../game/rules';
import { statInputOf } from '../../../game/rules/selfStats';
import type { SpawnPlayer } from '../../../gen/proto/flyff/v1/world_pb';
import { type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { formatTid } from '../../common/text';
import { sendExpStop } from './senders';
import { createStatusView } from './view';
import { type TargetEnv, targetFrameModel, type TargetInput } from '../targetFrame/model';
import { targetInputOf, type TargetOverrides } from '../targetFrame/input';
import { TargetFrame, TargetFrameState } from '../targetFrame/TargetFrame';

/** The target frame is recomputed at most this often (the HP gauge needs no more). */
export const TARGET_PERIOD_MS = 66;
/** The maximum HP of another player is looked up again after this long. */
const MAXHP_TTL_MS = 2000;

/** The target frame of a context: its model and the manual refresh the tests use. */
export interface StatusFeature {
  readonly target: TargetFrameState;
  /** Recomputes the frame now. */
  refreshTarget(): void;
  /** Unmounts the overlay (tests; the page never does). */
  dispose(): void;
}

const features = new WeakMap<ClientContext, StatusFeature>();
export function statusOf(ctx: ClientContext): StatusFeature | undefined {
  return features.get(ctx);
}

export const statusModule: FeatureModule = {
  id: 'status',
  milestone: 3,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const view = createStatusView(ctx);
    const target = new TargetFrameState();
    const level = new Map<number, number>();
    const job = new Map<number, number>();
    const overrides: TargetOverrides = { level, job };
    const maxHpCache = new Map<number, { at: number; level: number; job: number; maxHp: number }>();

    // another player's level and job follow `SetLevel` and `JobChanged` after the spawn
    fc.net.on('setLevel', (m, objId) => {
      if (objId !== 0 && !ctx.entities.get(objId)?.isSelf) level.set(objId, m.level);
    });
    fc.net.on('jobChanged', (m, objId) => {
      if (objId !== 0 && !ctx.entities.get(objId)?.isSelf) job.set(objId, m.job);
    });
    ctx.entities.onRemove((rec) => {
      level.delete(rec.objId);
      job.delete(rec.objId);
      maxHpCache.delete(rec.objId);
    });

    /** The maximum HP of another player through the rules (the replica of that object): the frame needs it for the gauge. */
    const playerMaxHp = (input: TargetInput): number => {
      const rec = ctx.entities.get(input.id);
      const p = (rec?.spawn as SpawnPlayer | undefined)?.player;
      if (!rec || !p) return 0;
      const now = ctx.now();
      const hit = maxHpCache.get(input.id);
      if (hit?.level === input.level && hit.job === input.job && now - hit.at < MAXHP_TTL_MS) {
        return hit.maxHp;
      }
      const rules = peekRulesRuntime(ctx)?.rules();
      if (!rules) return hit?.maxHp ?? 0;
      try {
        const stat = statInputOf(
          {
            level: input.level,
            job: input.job,
            base: { str: p.str, sta: p.sta, dex: p.dex, int: p.int },
          },
          rec.visual,
        );
        const maxHp = rules.derivedStats(input.id, stat).maxHp;
        maxHpCache.set(input.id, { at: now, level: input.level, job: input.job, maxHp });
        return maxHp;
      } catch {
        return hit?.maxHp ?? 0;
      }
    };

    const refreshTarget = (): void => {
      const tv = ctx.input.targetView?.();
      const id = tv ? (tv.renderTarget ?? tv.focus) : null;
      if (id === null) {
        target.set(null);
        return;
      }
      const rec = ctx.entities.get(id);
      if (!rec || rec.removed) {
        target.set(null);
        return;
      }
      let input = targetInputOf(ctx, rec, overrides);
      if (!input) {
        target.set(null);
        return;
      }
      if (input.player && input.maxHp <= 0) input = { ...input, maxHp: playerMaxHp(input) };
      const s = selfStore.getState();
      const ui = fullUi(ctx);
      const env: TargetEnv = {
        viewerAuth: s.authority,
        viewerLevel: s.level,
        additional: ctx.features.features.additional_target_info,
        width: ui?.scaler.get().virtual.width ?? 1024,
        gmInfo: (t) =>
          t.player
            ? ''
            : formatTid(fc.texts, TID_GAME_SELECT_OBJECT_INFORMATION_MONSTER, [t.hp, t.maxHp]),
      };
      target.set(targetFrameModel(input, env));
    };

    let last = Number.NEGATIVE_INFINITY;
    ctx.tickSystems.register('hud.target', () => {
      const now = ctx.now();
      if (now - last < TARGET_PERIOD_MS) return;
      last = now;
      refreshTarget();
    });

    // the overlay is mounted with the first frame to show (title screens and tests without a UI pay nothing)
    let unmount: (() => void) | null = null;
    if (typeof document !== 'undefined') {
      target.subscribe(() => {
        if (unmount === null && target.model !== null) unmount = mountTargetFrame(ctx, target);
      });
    }
    features.set(ctx, {
      target,
      refreshTarget,
      dispose: () => {
        unmount?.();
        unmount = null;
      },
    });

    // ---- the ports of the status window ---------------------------------------------------------------------------------

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.char.setPorts({
        status: {
          view,
          actions: {
            expStop: () => {
              sendExpStop(ctx);
            },
          },
        },
      });
    };
    ctx.tickSystems.register('status.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    ensureUi();

    addProbeMember(ctx, 'status', () => view.get());
    addProbeMember(ctx, 'targetFrame', () => target.model);
  },
};

/** Mounts the overlay container between the world canvas and the UI; returns the unmount. */
export function mountTargetFrame(ctx: ClientContext, state: TargetFrameState): () => void {
  const el = document.createElement('div');
  el.dataset['testid'] = 'target-frame-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const ui = fullUi(ctx);
    const st = ui?.scaler.get();
    root.render(
      createElement(TargetFrame, {
        state,
        viewport: st?.virtual ?? { width: 1024, height: 768 },
        scale: st?.scale ?? 1,
        ui,
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
