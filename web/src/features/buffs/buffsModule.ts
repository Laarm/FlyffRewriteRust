// The `buffs` feature module (Plan/45 §12, Plan/42 §19.5; WP-CFEAT-30): the buff maps of the movers from the messages
// (`SetSkillState`, `RemoveSkillInfluence`, `RemoveAllSkillInfluence`, the buff lists of `SpawnSelf` / `SpawnPlayer` / `SpawnNpc`),
// the cast of a timed item (`StateModeNtf`), and the ports of the buff windows of 42 (`ui.itemUse.setBuffs`): the world strip, the
// status window of the skill buffs (opened while the player has one), the tooltips and the double-click specials of the icons, and the
// rows of the item duration list.
//
// Other features reach it with `buffsOf(ctx)`: `has(type, id)`, `hasByIk3(ik3)` of the own buffs (the item router asks for the
// protection scrolls), `castActive()` (`IsStateMode(STATE_BASEMOTION_MODE)`), `openDurations()`.

import type { ClientContext } from '../../app/context';
import {
  II_SYS_SYS_FEED_01,
  II_SYS_SYS_SCR_PARTYSUMMON,
  II_SYS_SYS_SCR_PET_FEED_POCKET,
  II_SYS_SYS_SCR_RETURN,
} from '../../gen/defs/item';
import { petIconTier } from '../../game/pets/petData';
import { angelBuffMsg, isAngelIcon } from '../../input/buffIcons';
import { OBJSTA_STAND } from '../../input/worldTick';
import { wallNow } from '../../platform/time';
import type {
  BuffActions,
  BuffHudView,
  BuffIcon,
  BuffStatusView,
  CommItemView,
} from '../../ui/windows/itemuse/ports';
import type { RichRun } from '../../ui/text/markup';
import { addProbeMember } from '../common/probe';
import { type FeatureModule, fullUi } from '../common/module';
import type { ViewSource } from '../common/view';
import { defineOptionsStore } from '../options';
import { inventoryOf } from '../inventory';
import { isExpired } from '../inventory/info';
import { petsOf } from '../pets/petsModule';
import { activeTicketEnd } from '../tickets/active';
import { createComposeEnv } from '../tooltip/env';
import { type Buff, BuffSet, type BuffRules, remainOf } from './buffSet';
import { buildHud, buildStatus, type DisplayEnv, PERMANENT_TIME, smRemaining } from './display';
import { buildDurationRows, type DurationEnv } from './durations';
import { expTooltip, hudTooltip, smTooltip, statusTooltip, type TooltipEnv } from './tooltips';
import { BUFF_ITEM2, BUFF_SKILL } from './types';

/** `STATE_BASEMOTION_MODE` and its flags in `StateModeNtf`. */
const STATEMODE_BASEMOTION_ON = 0;

/** The cast of a timed item (`m_nReadyTime`, `m_dwUseItemId`). */
export interface CastState {
  readonly active: boolean;
  /** `timeGetTime()` of the start. */
  readonly startMs: number;
  /** The `ItemProp` id being cast. */
  readonly itemId: number;
}

export interface BuffsFeature {
  /** The own buff map. */
  own(): BuffSet;
  /** The buff map of another mover, `undefined` when none was sent. */
  of(objId: number): BuffSet | undefined;
  has(type: number, id: number): boolean;
  hasByIk3(ik3: number): boolean;
  cast(): CastState;
  /** `IsStateMode(STATE_BASEMOTION_MODE)`. */
  castActive(): boolean;
  /** Opens the item duration list (`APP_COMM_ITEM`). */
  openDurations(): void;
  /** Where a party summon double click goes (the window of `APP_SUMMON_PARTY_USE`; unset = nothing happens). */
  onPartySummon: (() => void) | null;
  /** The party-skill buffs (`CWndWorld::m_buffs`) drawn on the left of the strip: the party feature installs its set (`null` = none). */
  partyBuffs: BuffSet | null;
  /** The ports the UI gets. */
  readonly actions: BuffActions;
  readonly durations: ViewSource<CommItemView>;
  /** The icons as the world strip draws them (tests, the probe). */
  hud(width: number): BuffHudView;
}

const features = new WeakMap<ClientContext, BuffsFeature>();

/** The buff feature of a context; `undefined` before the module registered. */
export function buffsOf(ctx: ClientContext): BuffsFeature | undefined {
  return features.get(ctx);
}

const NO_CAST: CastState = { active: false, startMs: 0, itemId: 0 };

function sameIcons(a: readonly BuffIcon[], b: readonly BuffIcon[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const p = a[i];
    const q = b[i];
    if (!p || !q) return false;
    if (
      p.key !== q.key ||
      p.icon !== q.icon ||
      p.alpha !== q.alpha ||
      p.flash !== q.flash ||
      p.evil !== q.evil ||
      p.timer !== q.timer ||
      p.timerFont !== q.timerFont
    ) {
      return false;
    }
  }
  return true;
}

export const buffsModule: FeatureModule = {
  id: 'buffs',
  milestone: 4,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const options = defineOptionsStore(ctx.stores.registry);
    const nowUnix = (): number => Math.floor(wallNow() / 1000);

    const own = new BuffSet();
    const others = new Map<number, BuffSet>();
    let cast: CastState = NO_CAST;
    let onPartySummon: (() => void) | null = null;
    let partyBuffs: BuffSet | null = null;

    const composeEnv = createComposeEnv({
      data: () => ctx.data,
      features: () => ctx.features,
      colorSet: () => options.getState().ui.tooltipColorSet,
      nowSec: nowUnix,
      feedCount: () => inventoryOf(ctx)?.countOf(II_SYS_SYS_FEED_01) ?? 0,
    });

    const rules: BuffRules = {
      skillIsEvil: (id) => {
        const p = ctx.data?.skill(id);
        return p ? p.evildoing < 0 : undefined;
      },
      itemIsPermanent: (id) => ctx.data?.item(id)?.skill_time === PERMANENT_TIME,
    };

    // ---- the environments of the display functions -----------------------------------------------------------------------

    const display: DisplayEnv = {
      text: (tid) => composeEnv.text(tid),
      item: (id) => ctx.data?.item(id),
      skill: (id) => ctx.data?.skill(id),
      partySkill: (id) => ctx.data?.partySkill(id),
      betterBuffDisplay: () => ctx.features.features.better_buff_display,
      // `g_Option.m_bVisibleBuffTimeRender` (default on) has no option in the schema yet
      timeRender: () => true,
      now: () => ctx.now(),
      nowUnix,
      // `GetSystemPetTextureKey`: the tier of the SUMMONED pet's level (not of the buff's item); no pet or an egg: tier 0
      petIconTier: () => petIconTier(petsOf(ctx)?.petItem()?.pet?.level) ?? 0,
    };
    const pocketEnd = (): number | null => {
      const inv = inventoryOf(ctx);
      if (!inv) return null;
      const c = inv.state().container;
      for (let slot = 0; slot < c.indexNum; slot += 1) {
        const it = c.getAtSlot(slot);
        if (
          it?.itemId === II_SYS_SYS_SCR_PET_FEED_POCKET &&
          it.keepTimeAt > 0 &&
          !isExpired(it) &&
          it.extra === 0
        ) {
          return it.keepTimeAt;
        }
      }
      return null;
    };
    // `CMover::GetTicket()` (task ui-minigames, WP-CFEAT-45): the own ticket buff names the ticket item of the bag whose keep end the duration list shows
    const ticketEnd = (): number | null => {
      const inv = inventoryOf(ctx);
      if (!inv) return null;
      const c = inv.state().container;
      return activeTicketEnd({
        buffs: () => own.list(),
        kind3Of: (id) => ctx.data?.item(id)?.item_kind3,
        items: function* () {
          for (let slot = 0; slot < c.indexNum; slot += 1) {
            const it = c.getAtSlot(slot);
            if (it) yield { itemId: it.itemId, keepTimeAt: it.keepTimeAt, expired: isExpired(it) };
          }
        },
      });
    };
    const tip: TooltipEnv = {
      ...display,
      addSkill: (sub, level) => composeEnv.addSkill(sub, level),
      playerLevel: () => selfStore.getState().level,
      feedCount: () => inventoryOf(ctx)?.countOf(II_SYS_SYS_FEED_01) ?? 0,
      feedPocketEnd: pocketEnd,
    };
    const durationEnv: DurationEnv = {
      ...tip,
      ticketEnd,
      feedPocketEnd: pocketEnd,
    };

    // ---- the set of a mover ----------------------------------------------------------------------------------------------------

    const setOf = (objId: number, create: boolean): BuffSet | undefined => {
      const rec = objId === 0 ? ctx.entities.self() : ctx.entities.get(objId);
      if (rec?.isSelf === true || objId === 0) return own;
      let s = others.get(objId);
      if (!s && create) {
        s = new BuffSet();
        others.set(objId, s);
      }
      return s;
    };
    const unixLevel = (type: number, level: number): number =>
      type === BUFF_ITEM2 ? nowUnix() + level : level;

    fc.net.on('setSkillState', (m) => {
      const s = setOf(m.target, true);
      s?.add(m.buffType, m.buffId, unixLevel(m.buffType, m.level), m.totalMs, ctx.now(), rules);
    });
    fc.net.on('removeSkillInfluence', (m, objId) => {
      setOf(objId, false)?.remove(m.buffType, m.buffId);
    });
    fc.net.on('removeAllSkillInfluence', (_m, objId) => {
      setOf(objId, false)?.removeCommon((id) => (ctx.data?.item(id)?.charged ?? 0) !== 0);
    });
    const fill = (
      s: BuffSet,
      list: readonly {
        type: number;
        id: number;
        level: number;
        remainMs: number;
        expireAt: bigint;
      }[],
    ): void => {
      s.clear();
      const now = ctx.now();
      for (const b of list) {
        const level = b.type === BUFF_ITEM2 ? Number(b.expireAt) : b.level;
        s.add(b.type, b.id, level, b.remainMs, now, rules);
      }
    };
    fc.net.on('spawnSelf', (m) => {
      fill(own, m.buffs);
    });
    fc.net.on('spawnPlayer', (m, objId) => {
      if (objId === 0) return;
      const s = setOf(objId, true);
      if (s && s !== own) fill(s, m.buffs);
    });
    fc.net.on('spawnNpc', (m, objId) => {
      if (objId === 0) return;
      const s = setOf(objId, true);
      if (s && s !== own) fill(s, m.buffs);
    });
    fc.net.on('delObj', (_m, objId) => {
      others.delete(objId);
    });
    fc.net.on('stateModeNtf', (m, objId) => {
      if (objId !== 0 && ctx.entities.get(objId)?.isSelf !== true) return;
      if (m.flag === STATEMODE_BASEMOTION_ON) {
        cast = { active: true, startMs: ctx.now(), itemId: m.itemPropId };
        // casting is only allowed standing: anything else cancels (`OnStateMode`)
        const self = ctx.entities.self();
        if (self && (self.state & OBJSTA_STAND) === 0) inventoryOf(ctx)?.itemSenders.cancelCast();
      } else {
        cast = NO_CAST;
      }
    });

    // ---- the clock: the expiry of the buffs ---------------------------------------------------------------------------------

    ctx.tickSystems.register('buffs.expire', () => {
      own.process(ctx.now(), nowUnix(), rules);
      for (const s of others.values()) s.process(ctx.now(), nowUnix(), rules);
      for (const id of [...others.keys()]) if (!ctx.entities.has(id)) others.delete(id);
    });

    // ---- the ports of the UI ----------------------------------------------------------------------------------------------------

    const buffByKey = (key: string): Buff | undefined => {
      const [t, i] = key.split(':');
      if (t === undefined || i === undefined || t === 'exp' || t === 'sm' || t === 'party')
        return undefined;
      return own.get(Number(t), Number(i));
    };
    const hud = (width: number, avoid?: Parameters<typeof buildHud>[1]['avoid']): BuffHudView => {
      const smTimers = selfStore.getState().smTimers;
      const icons = buildHud(display, {
        width,
        own: own.list(),
        party: partyBuffs?.list() ?? [],
        sm: smRemaining(smTimers, nowUnix()),
        ...(avoid ? { avoid } : {}),
      });
      let fraction: number | null = null;
      if (cast.active) {
        const ready = ctx.data?.item(cast.itemId)?.skill_ready_type ?? 0;
        if (ready > 0) fraction = Math.min(1, Math.max(0, (ctx.now() - cast.startMs) / ready));
      }
      return { icons, cast: fraction === null ? null : { fraction } };
    };

    let lastStatus: BuffStatusView = { icons: [] };
    const statusListeners = new Set<() => void>();
    const status: ViewSource<BuffStatusView> = {
      get() {
        const icons = buildStatus(display, own.list());
        if (!sameIcons(icons, lastStatus.icons)) lastStatus = { icons };
        return lastStatus;
      },
      subscribe(fn) {
        statusListeners.add(fn);
        return () => {
          statusListeners.delete(fn);
        };
      },
    };

    const tooltip = (key: string): readonly RichRun[] | null => {
      const [kind, arg, arg2] = key.split(':');
      if (kind === 'party') {
        const pb = partyBuffs?.get(Number(arg), Number(arg2));
        return pb ? hudTooltip(tip, pb, true) : null;
      }
      if (kind === 'exp') return expTooltip(tip, own.list(), Number(arg));
      if (kind === 'sm') {
        const k = Number(arg);
        const rem = smRemaining(selfStore.getState().smTimers, nowUnix()).find((s) => s.kind === k);
        return rem ? smTooltip(tip, k, rem.remainSec) : null;
      }
      const b = buffByKey(key);
      if (!b) return null;
      return b.type === BUFF_SKILL ? statusTooltip(tip, b) : hudTooltip(tip, b);
    };

    const activate = (key: string): void => {
      const b = buffByKey(key);
      if (!b) return;
      const senders = inventoryOf(ctx)?.itemSenders;
      if (b.id === II_SYS_SYS_SCR_RETURN) senders?.returnScrollReq(-1);
      else if (b.id === II_SYS_SYS_SCR_PARTYSUMMON) onPartySummon?.();
      else if (b.id === II_SYS_SYS_SCR_PET_FEED_POCKET) senders?.feedPocketInactive();
    };

    /** The single click (`OnLButtonDown`, 43 §4.4): an angel buff icon redeems the finished buff for a 100 % angel item. */
    const click = (key: string): void => {
      const b = buffByKey(key);
      if (b && b.type !== BUFF_SKILL && isAngelIcon(b.id)) fc.net.send(angelBuffMsg());
    };

    const actions: BuffActions = {
      hud,
      status,
      tooltip,
      activate,
      click,
      mode: () => options.getState().ui.buffStatusMode,
      setMode: (mode) => {
        options.setState({
          ...options.getState(),
          ui: { ...options.getState().ui, buffStatusMode: mode },
        });
      },
      subscribe: (fn) => {
        statusListeners.add(fn);
        return () => {
          statusListeners.delete(fn);
        };
      },
    };

    // the duration rows change with the clock (once a second) and with the buffs
    let durationsCache: { key: string; view: CommItemView } = { key: '', view: { rows: [] } };
    const durations: ViewSource<CommItemView> = {
      get() {
        const sm = selfStore.getState().smTimers;
        const key = `${String(own.version)}|${String(Math.floor(ctx.now() / 1000))}|${sm.map((t) => `${String(t.kind)}:${String(t.endUnix)}`).join(',')}`;
        if (key !== durationsCache.key) {
          durationsCache = { key, view: { rows: buildDurationRows(durationEnv, own.list(), sm) } };
        }
        return durationsCache.view;
      },
      subscribe: (fn) => {
        statusListeners.add(fn);
        return () => {
          statusListeners.delete(fn);
        };
      },
    };

    const api: BuffsFeature = {
      own: () => own,
      of: (objId) => others.get(objId),
      has: (type, id) => own.has(type, id),
      hasByIk3: (ik3) =>
        own.list().some((b) => ctx.data?.item(b.id)?.item_kind3 === ik3 && b.type !== BUFF_SKILL),
      cast: () => cast,
      castActive: () => cast.active,
      openDurations: () => {
        fullUi(ctx)?.itemUse.openCommItem({ view: durations });
      },
      get onPartySummon() {
        return onPartySummon;
      },
      set onPartySummon(fn) {
        onPartySummon = fn;
      },
      get partyBuffs() {
        return partyBuffs;
      },
      set partyBuffs(set) {
        partyBuffs = set;
      },
      actions,
      durations,
      hud,
    };
    features.set(ctx, api);

    // ---- the windows: the strip, the status window while there is a skill buff ----------------------------------------------

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui) return;
      if (!installed.has(ui)) {
        installed.add(ui);
        ui.itemUse.setBuffs(actions);
        ui.itemUse.setCommItemView(durations);
      }
      // the buff windows belong to the world: the strip under the windows, the status window while there is a skill buff
      const inGame = ctx.machine.screen.s === 'InGame';
      if (inGame !== (ui.itemUse.buffStrip() !== null)) ui.itemUse.showBuffStrip(inGame);
      const hasSkill = inGame && own.list().some((b) => b.type === BUFF_SKILL);
      if (hasSkill !== (ui.itemUse.buffStatus() !== null)) ui.itemUse.showBuffStatus(hasSkill);
    };
    ctx.tickSystems.register('buffs.ports', ensureUi);
    const inWorld = (s: string): boolean => s === 'InGame' || s === 'Loading' || s === 'Entering';
    ctx.machine.subscribe((screen) => {
      // the lobby and the login have no buffs: what the last world left behind goes
      if (!inWorld(screen.s)) {
        own.clear();
        others.clear();
        cast = NO_CAST;
      }
      ensureUi();
    });
    ensureUi();

    addProbeMember(ctx, 'buffs', () => ({
      own: own.list().map((b) => ({
        type: b.type,
        id: b.id,
        level: b.level,
        remainMs: Math.round(remainOf(b, ctx.now())),
      })),
      cast: { ...cast },
      statusOpen: fullUi(ctx)?.itemUse.buffStatus() != null,
      hud: hud(fullUi(ctx)?.wm.layoutRect.r ?? 1280).icons.map((i) => ({
        key: i.key,
        x: i.x,
        y: i.y,
        timer: i.timer,
        flash: i.flash,
      })),
    }));
  },
};
