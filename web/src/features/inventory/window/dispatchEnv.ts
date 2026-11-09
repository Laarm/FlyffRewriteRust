// The wiring of the item dispatch into the inventory module (Plan/45 §9; WP-CFEAT-13, -14): the `DispatchEnv` over the stores of the
// client, `IsUseItemReadyTime`, the sfx of the upgrade wait and the windows of `dispatch/windows.ts`.

import type { ClientContext } from '../../../app/context';
import { peekFxRuntime } from '../../../game/fx';
import { XI_INT_INCHANT } from '../../../gen/defs/obj';
import { PARTS_RIDE } from '../../../gen/defs/neuz';
import {
  TID_ERROR_NOFLY,
  TID_GAME_USEAIRCRAFT,
  TID_PK_BLINK_LIMIT,
  TID_PK_FLIGHT_NOUSE,
  TID_PK_STAND_LIMIT,
} from '../../../gen/defs/text';
import { OBJSTA_STAND } from '../../../input/worldTick';
import type { ClientItem } from '../../../data/clientData';
import { buffsOf } from '../../buffs/buffsModule';
import { fullUi, type FeatureContext } from '../../common/module';
import { sysChat } from '../../common/text';
import { selfIsPartyLeader } from '../../party/store';
import { ItemDispatch } from '../dispatch';
import type { DispatchEnv, DispatchWindows, SelfFacts } from '../dispatch/env';
import { createDispatchSenders, type ItemUseSenders } from '../dispatch/senders';
import { createUpgradeRules } from '../dispatch/upgradeRules';
import { createDispatchWindows } from '../dispatch/windows';
import type { InvItem } from '../items';
import type { InventorySenders } from '../senders';
import type { InventoryState } from '../state';

/** `MODE_TRANSPARENT`. */
const MODE_TRANSPARENT = 0x0000_0002;
/** `OBJSTA_JUMP_ALL`: any jump state means the mover is off the ground. */
const OBJSTA_JUMP_ALL = 0xf000;
/** `SM_REVIVAL`. */
const SM_REVIVAL = 20;
/** `JOB_VAGRANT`. */
const JOB_VAGRANT = 0;
/** `II_GEN_*` ground item `dwFlightLimit` default. */
const DEFAULT_FLIGHT_LIMIT = 1;

export interface DispatchGlue {
  readonly dispatch: ItemDispatch;
  readonly senders: ItemUseSenders;
  readonly windows: DispatchWindows;
  readonly env: DispatchEnv;
}

export interface DispatchGlueDeps {
  readonly fc: FeatureContext;
  state(): InventoryState;
  readonly baseSenders: InventorySenders;
  /** Windows that later work packages add (`removeElem`, `restateConfirm`, ...). */
  readonly extraWindows?: Partial<DispatchWindows>;
}

/** Builds the dispatch of a context. */
export function createDispatchGlue(deps: DispatchGlueDeps): DispatchGlue {
  const { fc } = deps;
  const ctx: ClientContext = fc.app;
  const { selfStore } = ctx.stores;
  const prop = (id: number): ClientItem | undefined => ctx.data?.item(id);
  const say = (tid: number, args: readonly (string | number)[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };

  const senders = createDispatchSenders(ctx, deps.baseSenders);
  const windows = createDispatchWindows(
    { ui: () => fullUi(ctx), texts: fc.texts, prop },
    deps.extraWindows,
  );

  const self = (): SelfFacts => {
    const s = selfStore.getState();
    const rec = ctx.entities.self();
    const world = ctx.data?.world(s.worldId);
    return {
      present: rec !== undefined,
      dead: s.dead,
      flying: s.isFlying,
      onGround: rec ? (rec.state & OBJSTA_JUMP_ALL) === 0 : true,
      transparent: (s.modeFlags & MODE_TRANSPARENT) !== 0,
      vagrant: s.job === JOB_VAGRANT,
      skillPoints: s.skillPoints,
      // `GetCurrentMaxSkillPoint` is the skill rules' (M5)
      maxSkillPoints: null,
      smRevival: s.smTimers.some((t) => t.kind === SM_REVIVAL && t.endUnix > 0),
      partyLeader: selfIsPartyLeader(ctx),
      worldId: s.worldId,
      worldName: world?.folder ?? '',
    };
  };

  /**
   * `CMover::IsUseItemReadyTime` (`Mover.cpp:2830`): a worn item is always ready; a ride needs the flight level; a timed item cannot be
   * started while a shop is open, a cast runs, the mover is not standing or flies.
   */
  const ready = (item: InvItem, p: ClientItem): boolean => {
    if (deps.state().container.isEquip(item.objId)) return true;
    if (p.parts === PARTS_RIDE) {
      let limit = p.flight_limit;
      if (limit === 0xffff_ffff || limit < 0) limit = DEFAULT_FLIGHT_LIMIT;
      if (selfStore.getState().flightLevel < limit) {
        say(TID_GAME_USEAIRCRAFT);
        return false;
      }
      // `pWorld->m_bFly` / `HATTR_NOFLY`: the world settings say whether flying is allowed
      const folder = ctx.data?.world(selfStore.getState().worldId)?.folder;
      const settings = folder ? ctx.data?.worldFolder(folder)?.settings : undefined;
      if (settings && !settings.fly) {
        say(TID_ERROR_NOFLY);
        return false;
      }
    }
    const timed = p.skill_ready_type !== 0 && p.skill_ready_type !== 0xffff_ffff;
    if (timed) {
      if (fullUi(ctx)?.wm.isVendorWindowOpen() === true) return false;
      if (buffsOf(ctx)?.castActive() === true) {
        say(TID_PK_BLINK_LIMIT);
        return false;
      }
      const rec = ctx.entities.self();
      if (rec && (rec.state & OBJSTA_STAND) === 0) {
        say(TID_PK_STAND_LIMIT);
        return false;
      }
      if (selfStore.getState().isFlying) {
        say(TID_PK_FLIGHT_NOUSE);
        return false;
      }
    }
    return true;
  };

  const env: DispatchEnv = {
    state: () => deps.state(),
    prop,
    self,
    focusId: () => ctx.input.targetView?.().focus ?? null,
    say: (tid, args) => {
      say(tid, args ?? []);
    },
    now: () => ctx.now(),
    isOpen: (app) => fullUi(ctx)?.wm.isOpen(app) ?? false,
    ownVendorOpen: () => fullUi(ctx)?.wm.isVendorWindowOpen() ?? false,
    hasBuff: (type, id) => buffsOf(ctx)?.has(type, id) ?? false,
    hasBuffByIk3: (ik3) => buffsOf(ctx)?.hasByIk3(ik3) ?? false,
    ready,
    playUpgradeSfx: () => {
      const fx = peekFxRuntime(ctx)?.fx();
      const src = ctx.entities.self()?.objId;
      if (!fx || src === undefined) return null;
      const handle = fx.sfx(XI_INT_INCHANT, { src });
      return () => {
        fx.removeSfx(handle);
      };
    },
    enchantTimeMs: () => ctx.features.gameplay.enchant_time_s * 1000,
    quizWorld: () => false,
    features: () => ({
      better_safe_upgrades: ctx.features.features.better_safe_upgrades,
      randomoption_retry: ctx.features.features.randomoption_retry,
    }),
    rules: createUpgradeRules({ noPiercing: () => ctx.features.features.no_piercing }),
    send: senders,
    windows,
  };
  return { dispatch: new ItemDispatch(env), senders, windows, env };
}
