// The `npc` feature, menu part (Plan/45 §11.1-§11.2; WP-CFEAT-18): what a click on a player or an NPC does. The input layer
// reports the click (`InputHooks.showMoverMenu`, wired through `inputProvidersOf(ctx).moverMenu`); this part resolves the menu
// (`menu.ts`), shows the popup of 42 at the top-right corner of the target's bound rect, and runs the action of an id through
// a table that the features of the windows fill (`registerAction(MMI_TRADE, ...)`). An id without a registered action is an
// action of a window that is not built yet: it does nothing (and says so in the debug log).

import type { ClientContext } from '../../app/context';
import { inputProvidersOf } from '../../app/inputProviders';
import { EntityKind } from '../../game/entities';
import { JOB_VAGRANT } from '../../gen/defs/job';
import type { SpawnNpc } from '../../gen/proto/flyff/v1/world_pb';
import type { ScreenRect } from '../../render/types';
import { SC } from '../../ui/dnd/shortcut';
import { type FeatureContext, fullUi } from '../common/module';
import { eventOf } from '../events/api';
import { addProbeMember } from '../common/probe';
import {
  EVENT,
  menuLabel,
  playerMenuIds,
  resolveNpcClick,
  type NpcMenuEnv,
  type PlayerMenuEnv,
} from './menu';

/** What an action is told: the target the menu was opened for. */
export interface MenuActionArgs {
  readonly targetId: number;
  /** The `character.inc` key of an NPC target, `null` for a player. */
  readonly npcKey: string | null;
}
export type MenuAction = (a: MenuActionArgs) => void;

/** The public face of the menu feature of a context. */
export interface MenuFeature {
  /**
   * Registers the action of an `MMI_*` id (the window features call it at registration); returns the unregister function. Several features may
   * register the same id (`MMI_TRADE` is the shop of an NPC and the trade request of a player): every action runs, each one looks at the target.
   */
  registerAction(mmi: number, fn: MenuAction): () => void;
  /**
   * Registers a fallback for the ids that have no action: `CWndWorld::OnCommand`'s `default:` opens the item-exchange window (`__TRADESYS`) for every
   * id it does not know. The fallbacks are asked in the order they were added until one answers `true`; returns the unregister function.
   */
  registerFallback(fn: (mmi: number, args: MenuActionArgs) => boolean): () => void;
  /** Runs the action of `mmi` for a target now (`CWndWorld::OnCommand(mmi)`). */
  run(mmi: number, targetId: number): void;
  /** `ShowMoverMenu`: a click on a mover within reach. */
  onMoverClick(id: number, anchor: ScreenRect | null): void;
  /** `host.now()` of the last item dropped on the ground (`m_dwDropTime`), `null` for none. */
  lastDropAt(): number | null;
  /** Replaces what the menus read of the rest of the client (guild rank, party, campus, event flags); later features call it. */
  setPlayerEnv(fn: (targetId: number) => Partial<PlayerMenuEnv>): void;
  /**
   * Adds a provider next to `setPlayerEnv`'s (the providers are merged in the order they were added, the added ones over the replaceable
   * one): a feature that owns ONE field of the player menu (the duel rule of `RA_FIGHT`, the party leader) uses this so that it does not
   * replace what another feature told the menu. Returns the function that removes the provider.
   */
  addPlayerEnv(fn: (targetId: number) => Partial<PlayerMenuEnv>): () => void;
  setNpcEnv(env: Partial<NpcMenuEnv>): void;
}

const features = new WeakMap<ClientContext, MenuFeature>();

/** The menu feature of a context; `undefined` before the module registered. */
export function menuOf(ctx: ClientContext): MenuFeature | undefined {
  return features.get(ctx);
}

export function registerMenu(fc: FeatureContext): MenuFeature {
  const ctx = fc.app;
  const actions = new Map<number, MenuAction[]>();
  const fallbacks: ((mmi: number, args: MenuActionArgs) => boolean)[] = [];
  let dropAt: number | null = null;
  let playerEnv: (targetId: number) => Partial<PlayerMenuEnv> = () => ({});
  const addedEnvs: ((targetId: number) => Partial<PlayerMenuEnv>)[] = [];
  let npcOverrides: Partial<NpcMenuEnv> = {};
  const stats = { clicks: 0, popups: 0, actions: 0, unhandled: 0 };
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);

  const npcEnv = (): NpcMenuEnv => ({
    guilded: () => ctx.stores.selfStore.getState().guildId !== 0,
    vagrant: () => ctx.stores.selfStore.getState().job === JOB_VAGRANT,
    // the flags of the server (`FlyffEvent`, `SetLocalEvent`); while the events feature is absent both default on (`events.eve_18`, `events.guild_bank`) and the 1:1 guild combat is off
    eventOn: (id) => {
      const events = eventOf(ctx);
      return events ? events.isEventOn(id) : id === EVENT.EVE_18 || id === EVENT.GUILD_INVENTORY;
    },
    ...npcOverrides,
  });

  /** The added providers merged in the order they were added. */
  const addedEnv = (targetId: number): Partial<PlayerMenuEnv> => {
    const merged: Partial<PlayerMenuEnv> = {};
    for (const f of addedEnvs) Object.assign(merged, f(targetId));
    return merged;
  };

  const run = (mmi: number, targetId: number): void => {
    const rec = ctx.entities.get(targetId);
    const npcKey =
      rec?.kind === EntityKind.Npc ? ((rec.spawn as SpawnNpc).npc?.characterKey ?? '') : null;
    const fns = actions.get(mmi);
    if (!fns || fns.length === 0) {
      if (fallbacks.some((f) => f(mmi, { targetId, npcKey }))) {
        stats.actions += 1;
        return;
      }
      stats.unhandled += 1;
      ctx.log.debug('npc', `menu action ${String(mmi)} has no window yet`);
      return;
    }
    stats.actions += 1;
    for (const fn of [...fns]) fn({ targetId, npcKey });
  };

  /** The anchor of the popup in virtual UI pixels: the top-right corner of the bound rect (CSS pixels / the UI scale). */
  const anchorOf = (anchor: ScreenRect | null): { x: number; y: number } => {
    const ui = fullUi(ctx);
    const scale = ui?.scaler.scale ?? 1;
    if (!anchor) {
      const l = ui?.wm.layoutRect;
      return { x: l ? Math.trunc((l.l + l.r) / 2) : 0, y: l ? Math.trunc((l.t + l.b) / 2) : 0 };
    }
    return { x: Math.trunc((anchor.x + anchor.w) / scale), y: Math.trunc(anchor.y / scale) };
  };

  const popup = (ids: readonly number[], targetId: number, anchor: ScreenRect | null): void => {
    const ui = fullUi(ctx);
    if (!ui || ids.length === 0) return;
    const menu = ui.npc.moverMenu;
    menu.onPick = (id) => {
      run(id, targetId);
    };
    menu.show(
      ids.map((id) => ({ id, text: menuLabel(id, text) })),
      anchorOf(anchor),
    );
    stats.popups += 1;
  };

  const onMoverClick = (id: number, anchor: ScreenRect | null): void => {
    stats.clicks += 1;
    const rec = ctx.entities.get(id);
    if (!rec) return;
    if (rec.kind === EntityKind.Player) {
      const vendorOpen = rec.visual?.vendorTitle != null;
      const env: PlayerMenuEnv = {
        targetVendorOpen: vendorOpen,
        canInviteToGuild: false,
        eve18: true,
        duelAllowed: false,
        partyLeader: false,
        campus: 'none',
        ...playerEnv(id),
        ...addedEnv(id),
      };
      popup(playerMenuIds(env), id, anchor);
      return;
    }
    if (rec.kind !== EntityKind.Npc) return;
    // a flying player does not talk to NPCs (45 §11.1)
    if (ctx.stores.selfStore.getState().isFlying) return;
    const key = (rec.spawn as SpawnNpc).npc?.characterKey ?? '';
    const row = key === '' ? undefined : ctx.data?.npc(key);
    if (!row) return;
    const click = resolveNpcClick(row.menus, npcEnv());
    if (click.kind === 'action') run(click.id, id);
    else if (click.kind === 'popup') popup(click.ids, id, anchor);
  };

  // `m_dwDropTime`: an item dropped on the ground blocks the conversation for three seconds
  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.wm.drag.onEnd((e) => {
      if (e.world && e.accepted && e.shortcut.kind === SC.ITEM) dropAt = ui.host.now();
    });
  };
  ctx.tickSystems.register('npc.menu.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  const feature: MenuFeature = {
    registerAction(mmi, fn) {
      const list = actions.get(mmi) ?? [];
      list.push(fn);
      actions.set(mmi, list);
      return () => {
        const cur = actions.get(mmi);
        const i = cur?.indexOf(fn) ?? -1;
        if (cur && i >= 0) cur.splice(i, 1);
        if (cur?.length === 0) actions.delete(mmi);
      };
    },
    registerFallback(fn) {
      fallbacks.push(fn);
      return () => {
        const i = fallbacks.indexOf(fn);
        if (i >= 0) fallbacks.splice(i, 1);
      };
    },
    run,
    onMoverClick,
    lastDropAt: () => dropAt,
    setPlayerEnv(fn) {
      playerEnv = fn;
    },
    addPlayerEnv(fn) {
      addedEnvs.push(fn);
      return () => {
        const i = addedEnvs.indexOf(fn);
        if (i >= 0) addedEnvs.splice(i, 1);
      };
    },
    setNpcEnv(env) {
      npcOverrides = { ...npcOverrides, ...env };
    },
  };
  features.set(ctx, feature);
  inputProvidersOf(ctx).moverMenu = onMoverClick;

  // Playwright runs an action of the menu without a click (the guild creation dialog of Helena has no entry in her click menu, KQ-26)
  addProbeMember(ctx, 'npcMenuRun', (mmi: number, targetId: number) => {
    feature.run(mmi, targetId);
  });
  addProbeMember(ctx, 'npcMenu', () => ({
    ...stats,
    actions: [...actions.keys()].sort((a, b) => a - b),
    popupOpen: fullUi(ctx)?.npc.moverMenu.shown ?? false,
  }));
  return feature;
}
