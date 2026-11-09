// The `secretRoom` feature (Plan/45 §28.0, §28.6; WP-CFEAT-52; 22 §11; ref/09b §3.16; `src:Neuz/DPClient.cpp:17518-17845`, `_Interface/WndSecretRoom.cpp`, `WndWorld.cpp`): the weekly guild contest for the
// hunting grounds of the continents. The server's five messages land in `eventStore.secretRoom` through the pure reducers of `reducers.ts` (every one is applied whether or not a window is open).
//
//   SecretRoomMngState          the state, the captions (SMALL caption font) and the one HUD timer line of the world (`eventOf( ctx ).setHudTimer( .., 'secretRoom' )`)
//   SecretRoomInfo              the guild list of a continent, the kill counts and the war states (the board of the war)
//   TenderOpenWnd / LineUpOpenWnd / TenderView     the bid window (700), the line-up editor (702) and the status window (704) over `ui.secretRoom`
//   menu                        the secret-room entries of the NPC menu, ids 84-90, 100 and 103 (`menu.ts`)
//   HUD board                   `SecretRoomHud.tsx`: the guilds of the war with their kill counters and the pulsing outlines; the kill counters are there (the 754 window lists members only)
//   quick window                754: opened by itself during the war for a member of the line-up (`quick.ts`)
//   seams                       `secretRoomOf( ctx )`, the probe members `secretRoom`, `secretRoomWindows`, `secretRoomPoints`
//
// The slice is reset when the character leaves the world (not on a world change or a reconnect). The tax is `../tax`, the Heaven Tower `../heavenTower`.

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../../app/context';
import { EntityKind, type EntityRecord } from '../../../game/entities';
import { APP_SECRETROOM_MSG, APP_SECRETROOM_QUICK } from '../../../gen/defs/resdata';
import { WI_DUNGEON_SECRET_1, WI_DUNGEON_SECRET_L } from '../../../gen/defs/world';
import type { InputInstance } from '../../../input/input';
import { themePath } from '../../../ui/skin/images';
import { MISSING } from '../../../ui/skin/images';
import type { QuickRowView, QuickView } from '../../../ui/windows/secretroom/ports';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { playerDataOf } from '../../common/playerData';
import { formatTid } from '../../common/text';
import { derivedView } from '../../common/view';
import { guildOf } from '../../guild/api';
import { guildLogoPath } from '../../guild/logos';
import { hudOf } from '../../hud/hudModule';
import { menuOf } from '../../npc/menuModule';
import { deepEqual } from '../../party/view';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import { withHelpPanels } from '../helpPanels';
import { taxOf } from '../tax/taxModule';
import { initialSecretRoom, type EventStore, type SecretRoomGuild } from '../store';
import { memberRowOf, onlineMembersByLevel } from '../siege/view';
import { type BoardEnv, hudBoard, type HudBoard } from './hudModel';
import { BOARD_PANEL, DESC_PANEL, menuTable } from './menu';
import { registerSecretRoomProbe } from './probe';
import { quickAction, quickMembers, quickRow } from './quick';
import {
  reduceInfo,
  reduceLineUpOpenWnd,
  reduceMngState,
  reduceTenderOpenWnd,
  reduceTenderView,
  type Reduced,
  type SecretRoomEffect,
  type SecretRoomEnv,
  type SecretRoomOpen,
} from './reducers';
import { secretRoomSenders, type SecretRoomSenders } from './senders';
import { SecretRoomHud, SecretRoomHudState } from './SecretRoomHud';

/** The refresh period of the HUD board and the check of the quick window. */
export const SECRETROOM_REFRESH_MS = 250;

/** The public face of the secret-room feature of a context. */
export interface SecretRoomFeature {
  readonly store: EventStore;
  readonly senders: SecretRoomSenders;
  /** `secretRoom.state`: the manager states 1 .. 6 and the continent states 101 .. 104. */
  state(): number;
  /** The character stands in a secret room (the worlds 152 .. 167, `WI_DUNGEON_SECRET_1 .. L`). */
  inRoom(): boolean;
  /** The entry of the own guild in the list of its continent, `null` without one. */
  ownTender(): SecretRoomGuild | null;
  /** The kills needed in a war state (0 when unknown). */
  totalOf(war: number): number;
  /** The war board (the state of its overlay; tests read it). */
  readonly hud: SecretRoomHudState;
  /** The model of the board now, `null` when it is not drawn. */
  board(): HudBoard | null;
  /** Recomputes the board and checks the quick window now. */
  refresh(): void;
  /** The check of the quick window (opens or closes it). */
  checkQuick(): void;
  /** Unmounts the HUD overlay (tests; the page never does). */
  dispose(): void;
}

const features = new WeakMap<ClientContext, SecretRoomFeature>();

/** The secret-room feature of a context; `undefined` before the module registered. */
export function secretRoomOf(ctx: ClientContext): SecretRoomFeature | undefined {
  return features.get(ctx);
}

/** Mounts the HUD container between the world canvas and the UI; returns the unmount. */
export function mountSecretRoomHud(ctx: ClientContext, state: SecretRoomHudState): () => void {
  const el = document.createElement('div');
  el.dataset['testid'] = 'sr-hud-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const st = fullUi(ctx)?.scaler.get();
    root.render(
      createElement(SecretRoomHud, {
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

/** Registers the secret room (after the events feature). */
export function registerSecretRoom(fc: FeatureContext): SecretRoomFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const playerData = playerDataOf(ctx);
  const senders = secretRoomSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const selfChar = (): number => selfStore.getState().idPlayer;
  const jobName = (job: number): string => ctx.data?.job(job)?.name ?? '';
  const sr = () => store.getState().secretRoom;

  const inRoom = (): boolean => {
    const w = selfStore.getState().worldId;
    return w >= WI_DUNGEON_SECRET_1 && w <= WI_DUNGEON_SECRET_L;
  };
  const ownEntry = (): SecretRoomGuild | null => {
    const id = guildOf(ctx)?.ownGuildId() ?? 0;
    return id === 0 ? null : (sr().guilds.find((g) => g.guildId === id) ?? null);
  };

  // ---- the facts the reducers read --------------------------------------------------------------------------------------------------

  const env = (): SecretRoomEnv => {
    const g = guildOf(ctx);
    return {
      isMaster: g?.rankOf(selfChar()) === 0,
      guildName: (id) => g?.nameOf(id) ?? null,
    };
  };

  // ---- the windows -----------------------------------------------------------------------------------------------------------------

  const open = (o: SecretRoomOpen): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const g = guildOf(ctx);
    switch (o.window) {
      case 'offer':
        withHelpPanels(ctx, (panels) => {
          fullUi(ctx)?.secretRoom.openOffer({
            min: o.min,
            req: o.req,
            info: panels[DESC_PANEL] ?? '',
            tender: (penya) => {
              senders.tender(penya);
            },
          });
        });
        break;
      case 'selection':
        if (!g) break;
        ui.secretRoom.openSelection({
          min: o.min,
          max: o.max,
          selected: o.ids,
          members: () => onlineMembersByLevel(g.members(), jobName),
          memberOf: (id) => memberRowOf(g.members(), id, jobName),
          isMaster: (id) => g.isMaster(id),
          submit: (ids) => {
            senders.lineUpMember(ids);
          },
        });
        break;
      case 'offerState':
        ui.secretRoom.openOfferState({
          secondsToNext: o.secondsToNext,
          guilds: o.names,
          rank: o.rank,
          myTender: o.myTender,
          apply: () => {
            senders.tenderOpenWnd();
          },
          refresh: () => {
            senders.tenderView();
          },
        });
        break;
      case 'infoBox':
        ui.secretRoom.openInfoBox({
          text: text(o.tid),
          yes: () => {
            senders.teleportToNpc();
          },
        });
        break;
    }
  };

  // ---- carrying out the effects of a reducer -----------------------------------------------------------------------------------------

  const run = (effects: readonly SecretRoomEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'caption':
          // `AddCaption` of the secret room uses `m_pFontAPICaption`: the SMALL caption
          hudOf(ctx).caption(formatTid(fc.texts, e.tid, e.args), true, ctx.now());
          break;
        case 'hudTimer':
          events.setHudTimer(text(e.tid), e.ms, 'secretRoom');
          break;
        case 'clearHudTimer':
          events.clearHudTimer('secretRoom');
          break;
        case 'open':
          open(e);
          break;
      }
    }
  };
  const apply = (r: Reduced): void => {
    if (r.state !== store.getState()) store.setState(r.state);
    run(r.effects);
  };

  fc.net.on('secretRoomMngState', (m) => {
    apply(reduceMngState(store.getState(), m, env()));
  });
  fc.net.on('secretRoomInfo', (m) => {
    apply(reduceInfo(store.getState(), m, env()));
  });
  fc.net.on('secretRoomTenderOpenWndNtf', (m) => {
    apply(reduceTenderOpenWnd(store.getState(), m));
  });
  fc.net.on('secretRoomLineUpOpenWndNtf', (m) => {
    apply(reduceLineUpOpenWnd(store.getState(), m));
  });
  fc.net.on('secretRoomTenderViewNtf', (m) => {
    apply(reduceTenderView(store.getState(), m, env()));
  });

  // ---- the NPC menu ------------------------------------------------------------------------------------------------------------------

  const hud = new SecretRoomHudState();
  const table = menuTable({
    tenderOpenWnd: () => {
      senders.tenderOpenWnd();
    },
    tenderView: () => {
      senders.tenderView();
    },
    lineUpOpenWnd: () => {
      senders.lineUpOpenWnd();
    },
    entrance: () => {
      // `MMI_SECRET_ENTRANCE`: the digits go back to "000", the question box goes, then the entrance
      hud.resetDigits();
      fullUi(ctx)?.secretRoom.close(APP_SECRETROOM_MSG);
      senders.entrance();
    },
    checkTax: () => {
      taxOf(ctx)?.openCheck();
    },
    openBoard: () => {
      withHelpPanels(ctx, (panels) => {
        fullUi(ctx)?.secretRoom.openBoard({ text: panels[BOARD_PANEL] ?? '' });
      });
    },
    teleportToDungeon: () => {
      senders.teleportToDungeon();
    },
    openCancelConfirm: () => {
      fullUi(ctx)?.secretRoom.openCancelConfirm({
        yes: () => {
          senders.tenderCancelReturn();
        },
      });
    },
  });
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      fn();
    });
  }

  // ---- the movers ---------------------------------------------------------------------------------------------------------------------

  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  const playerByCharacter = (id: number): EntityRecord | null => {
    let out: EntityRecord | null = null;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (valid(rec) && rec.idPlayer === id) out = rec;
    });
    return out;
  };
  /** The mover of a character when he is in view; the own character always is. */
  const moverOf = (id: number): { hp: number; maxHp: number } | null => {
    if (id === selfChar()) {
      const s = selfStore.getState();
      return { hp: s.hp, maxHp: s.maxHp };
    }
    const rec = playerByCharacter(id);
    return rec ? { hp: rec.hp, maxHp: rec.maxHp ?? 0 } : null;
  };

  // ---- the quick window (754) ---------------------------------------------------------------------------------------------------------

  /** The members the open window lists, fixed when it opened (KEEP). */
  let quickIds: readonly number[] = [];
  const tickBus = new Set<() => void>();
  const tickSource = {
    subscribe: (fn: () => void) => {
      tickBus.add(fn);
      return () => {
        tickBus.delete(fn);
      };
    },
  };
  const quickSources = [
    selfStore,
    store,
    tickSource,
    ...(guildOf(ctx) ? [guildOf(ctx)?.store] : []),
  ].filter((x): x is NonNullable<typeof x> => x !== undefined);
  const quickView = derivedView<QuickView>(
    () => {
      const g = guildOf(ctx);
      const roster = g?.members() ?? [];
      const masterId = g ? (g.briefOf(g.ownGuildId())?.idMaster ?? 0) : 0;
      const rows: QuickRowView[] = quickIds.map((id, i) => {
        const r = roster.find((x) => x.id === id);
        return quickRow(
          {
            id,
            name: r?.name ?? playerData.nameOf(id),
            level: r?.level ?? 1,
            job: r?.job ?? 0,
            online: r?.online ?? false,
            first: i === 0 && id === masterId,
            self: id === selfChar(),
            mover: moverOf(id),
          },
          text,
        );
      });
      return { rows };
    },
    quickSources,
    deepEqual,
  );
  const targeting = (): InputInstance['core']['targeting'] | null =>
    (ctx.input as Partial<InputInstance>).core?.targeting ?? null;
  const executing = (): number =>
    (ctx.input as Partial<InputInstance>).core?.deps.actionSlot?.executing() ?? 0;
  const quickClick = (id: number): boolean => {
    const t = targeting();
    if (!t) return false;
    // nothing while the character auto-attacks or the action queue of the taskbar runs (`m_nExecute != 0`, `WndSecretRoom.cpp:2099-2101`)
    if (t.autoAttack || executing() !== 0) return false;
    if (id === selfChar()) {
      t.setFocus(null);
      return true;
    }
    const rec = playerByCharacter(id);
    if (!rec) return false;
    t.setFocus(rec.objId);
    return true;
  };
  const checkQuick = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const entry = ownEntry();
    const action = quickAction({
      state: sr().state,
      inRoom: inRoom(),
      hasOwnEntry: entry !== null,
      open: ui.secretRoom.isOpen(APP_SECRETROOM_QUICK),
    });
    if (action === 'open' && entry) {
      const g = guildOf(ctx);
      quickIds = quickMembers(
        g ? (g.briefOf(g.ownGuildId())?.idMaster ?? 0) : 0,
        entry.lineup,
        g?.members() ?? [],
      );
      ui.secretRoom.openQuick({ view: quickView, actions: { click: quickClick } });
    } else if (action === 'close') {
      ui.secretRoom.close(APP_SECRETROOM_QUICK);
    }
  };

  // ---- the war board -------------------------------------------------------------------------------------------------------------------

  const logoUrls = new Map<number, string | null>();
  hud.logoUrl = (logo) => {
    if (logoUrls.has(logo)) return logoUrls.get(logo) ?? null;
    const ui = fullUi(ctx);
    const path = guildLogoPath(logo);
    if (!ui || path === null) return null;
    logoUrls.set(logo, null);
    const full = themePath(path);
    ui.images.get(full, 'exact').then(
      (img) => {
        if (img === MISSING) return;
        logoUrls.set(logo, ui.images.url(full, 'exact'));
        hud.bump();
      },
      () => undefined,
    );
    return null;
  };
  const boardEnv = (): BoardEnv => {
    const g = guildOf(ctx);
    return {
      inRoom: inRoom(),
      ownGuildId: g?.ownGuildId() ?? 0,
      guildName: (id) => g?.nameOf(id) ?? null,
      logoOf: (id) => g?.briefOf(id)?.logo ?? 0,
      text,
    };
  };
  const currentBoard = (): HudBoard | null => hudBoard(sr(), boardEnv());
  const refresh = (): void => {
    hud.set(currentBoard());
    checkQuick();
    for (const fn of [...tickBus]) fn();
  };
  store.subscribe(refresh);
  selfStore.subscribe(refresh);
  guildOf(ctx)?.store.subscribe(refresh);
  let last = Number.NEGATIVE_INFINITY;
  ctx.tickSystems.register('secretRoom.hud', () => {
    const now = ctx.now();
    if (now - last < SECRETROOM_REFRESH_MS) return;
    last = now;
    refresh();
  });
  let unmount: (() => void) | null = null;
  if (typeof document !== 'undefined') {
    hud.subscribe(() => {
      if (unmount === null && hud.model !== null) unmount = mountSecretRoomHud(ctx, hud);
    });
  }

  // ---- the lifetime: the character ---------------------------------------------------------------------------------------------------

  // the secret room of a character that left the world (logout, kick, back to the character list) goes with it; a world change keeps it, the join batch sends it again
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    fullUi(ctx)?.secretRoom.closeAll();
    quickIds = [];
    hud.resetDigits();
    events.clearHudTimer('secretRoom');
    store.setState({ secretRoom: initialSecretRoom() });
  });

  // ---- the public face ------------------------------------------------------------------------------------------------------------------

  const feature: SecretRoomFeature = {
    store,
    senders,
    state: () => sr().state,
    inRoom,
    ownTender: ownEntry,
    totalOf: (war) => sr().totals.get(war) ?? 0,
    hud,
    board: currentBoard,
    refresh,
    checkQuick,
    dispose: () => {
      unmount?.();
      unmount = null;
    },
  };
  features.set(ctx, feature);

  registerSecretRoomProbe(ctx);
  addProbeMember(ctx, 'secretRoom', () => {
    const s = store.getState();
    const quick = fullUi(ctx)?.secretRoom.quick() ?? null;
    return {
      state: s.secretRoom.state,
      continent: s.secretRoom.continent,
      guilds: s.secretRoom.guilds.map((g) => ({ ...g, lineup: [...g.lineup] })),
      totals: [...s.secretRoom.totals],
      hudTimer: s.hudTimer,
      inRoom: inRoom(),
      windows: fullUi(ctx)?.secretRoom.openApps() ?? [],
      quick: { open: quick !== null, members: [...quickIds] },
      board: (hud.model?.rows ?? []).map((r) => ({
        index: r.index,
        guildId: r.guildId,
        own: r.own,
        war: r.war,
        kills: r.kills,
        rank: r.rank.text,
        name: r.name.text,
        state: r.state?.text ?? null,
        count: r.count
          ? `${r.count.digits.map((d) => d.text).join('')}/${r.count.total.text}`
          : null,
      })),
    };
  });
  return feature;
}
