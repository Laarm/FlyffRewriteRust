// The `guildHouse` feature (Plan/45 §28.11; WP-CFEAT-46; server half `event/guild_house/`): the small guild house.
//
//   messages    `GuildHouseAllInfo`, `GuildHousePacketNtf` (ops 0-7), `GuildHouseRemove`, `GuildHouseRestPoint` -> `eventStore.guildHouse` (`reducers.ts`, pure); the slice is reset when the character leaves the world and
//               when the own guild is left
//   windows     `APP_GH_MAIN` (opened with the world 212-214, closed when it is left), the storage applet `APP_GH_FURNITURE_STORAGE` (rows from the slice, the row buttons), `APP_GH_SHOWONE` (the item being
//               placed), the upkeep box (on `APP_CONFIRM_BUYEX`, own instance key): requests and ports over `ui.housing`
//   menu        147 sale, 148 upkeep (power `0x40`), 149 enter, 150 leave (`menu.ts`); the furniture menu of a clicked piece (`ctrlMenu.ts`, `inputProvidersOf( ctx ).ctrlMenu`)
//   input       `inputProvidersOf( ctx ).guildFurniture`: a click on a furniture selects its row (the open storage window follows), Esc clears the selection
//   status      `eventStore.guildHouse.restPercent` is the rest gauge of the status window (`features/hud/status/view.ts`)
//   probe       `__flyffDebug.features.guildHouse()`

import type { ClientContext } from '../../../app/context';
import { AUTH_GAMEMASTER } from '../../../app/hosts';
import { inputProvidersOf } from '../../../app/inputProviders';
import { IK3_CARPET, IK3_TELEPORTER, IK3_WALLPAPER } from '../../../gen/defs/itemkind';
import {
  APP_GH_FURNITURE_STORAGE,
  APP_GH_MAIN,
  APP_GUILD,
  APP_GUILD_BANK,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_GUILDHOUSE_INSIDE_INSTALL_CHANNEL,
  TID_GAME_GUILDHOUSE_TAX_LEVEL,
} from '../../../gen/defs/text';
import type { GhFntrInfo } from '../../../gen/proto/flyff/v1/housing_pb';
import { wallNow } from '../../../platform/time';
import type { ScreenRect } from '../../../render/types';
import type { GhMainRequest, StoragePort } from '../../../ui/windows/housing/ports';
import {
  type GhRowView,
  PF_GUILDHOUSE_FURNITURE,
  PF_GUILDHOUSE_UPKEEP,
} from '../../../ui/windows/housing/rows';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { sysChat } from '../../common/text';
import { guildOf } from '../../guild/api';
import { isGuildHouseWorld } from '../../loading/images';
import { menuOf } from '../../npc/menuModule';
import { deployOf } from '../deploy/api';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import type { EventStore } from '../store';
import { ctrlMenuItems, runCtrlMenu } from './ctrlMenu';
import { guildHouseMenuTable } from './menu';
import {
  type GhEffect,
  type GhMsg,
  type GuildHouseEnv,
  type PropFacts,
  reduceGuildHouse,
} from './reducers';
import { bareInfo, guildHouseSenders, type GuildHouseSenders, type PacketInfo } from './senders';
import { type GhHouse, type GhRow, GH_OP, type GuildHouseSlice } from './slice';
import { ghRowViews, slotOfObj } from './storageView';

/** The public face of the guild-house feature of a context. */
export interface GuildHouseFeature {
  readonly store: EventStore;
  readonly senders: GuildHouseSenders;
  state(): GuildHouseSlice;
  /** The character stands in a guild-house world (212-214). */
  inHouse(): boolean;
  /** `hasPower( PF_GUILDHOUSE_FURNITURE )` inside the house: the furniture buttons work. */
  canManage(): boolean;
  /** The rows of the house with their props. */
  rowViews(): GhRowView[];
  /** `m_dwSelectedObjID` (`null` = `NULL_ID`). */
  select(objId: number | null): void;
  /** `m_iMode`. */
  setMode(mode: 0 | 2 | 4): void;
  /** A row button of the storage window. */
  rowButton(row: GhRowView): void;
  /** The packet of an operation for the row at `slot` with the stored info (`SendClientToWorld( op, *pInfo, slot )`). */
  sendPacket(op: number, slot: number, over?: Partial<PacketInfo>): void;
  /** The row at `slot` of the house. */
  rowAt(slot: number): GhRow | null;
  /** The upkeep window (power check, replaces an open one). */
  openUpkeep(): void;
}

const features = new WeakMap<ClientContext, GuildHouseFeature>();

/** The guild-house feature of a context; `undefined` before the module registered. */
export function guildHouseOf(ctx: ClientContext): GuildHouseFeature | undefined {
  return features.get(ctx);
}

const rowOf = (i: GhFntrInfo): GhRow => ({
  itemId: i.itemId,
  setup: i.setup,
  pos: [i.pos?.x ?? 0, i.pos?.y ?? 0, i.pos?.z ?? 0],
  angle: i.angle,
  ctrlObjId: i.ctrlObjId,
  expireAt: Number(i.expireAt),
});

export function registerGuildHouse(fc: FeatureContext): GuildHouseFeature {
  const ctx = fc.app;
  const known = guildHouseOf(ctx);
  if (known) return known;
  const { worldStore, sessionStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const senders = guildHouseSenders((msg) => ctx.net.sender.send(msg));
  const nowSec = (): number => Math.floor(wallNow() / 1000);

  const env = (): GuildHouseEnv => ({
    prop: (itemId): PropFacts | undefined => {
      const p = ctx.data?.item(itemId);
      return p ? { name: p.name, kind2: p.item_kind2, kind3: p.item_kind3 } : undefined;
    },
    isGm: sessionStore.getState().authority >= AUTH_GAMEMASTER,
  });

  // ---- carrying out the effects --------------------------------------------------------------------------------------------
  const run = (effects: readonly GhEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'chat':
          sysChat(fc.texts, fc.chat, e.tid, e.args);
          break;
        case 'chatText':
          fc.chat?.putString(e.text, null, e.color, 0x600);
          break;
        case 'refresh':
          fullUi(ctx)?.housing.refresh(e.app);
          break;
        case 'endDeploy': {
          const d = deployOf(ctx);
          if (d?.state()?.kind === 'guild') d.cancel();
          break;
        }
      }
    }
  };
  const apply = (msg: GhMsg): void => {
    const cur = store.getState();
    const r = reduceGuildHouse(cur.guildHouse, msg, env());
    if (r.state !== cur.guildHouse) store.setState({ guildHouse: r.state });
    run(r.effects);
  };

  fc.net.on('guildHouseAllInfo', (m) => {
    const h = m.house;
    const house: GhHouse | null = h
      ? {
          idGuild: h.idGuild,
          worldId: h.worldId,
          upkeepAt: Number(h.upkeepAt),
          rows: h.furniture.map(rowOf),
        }
      : null;
    apply({ kind: 'allInfo', bankEnabled: m.bankEnabled, house });
  });
  fc.net.on('guildHousePacketNtf', (m) => {
    if (m.info) apply({ kind: 'packet', op: m.op, index: m.index, info: rowOf(m.info) });
  });
  fc.net.on('guildHouseRemove', () => {
    apply({ kind: 'remove' });
  });
  fc.net.on('guildHouseRestPoint', (m) => {
    apply({ kind: 'restPoint', restPoint: m.restPoint });
  });

  // the house of a character that left the world goes with it (a world change keeps it, the join batch sends it once)
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    fullUi(ctx)?.housing.closeAll();
    apply({ kind: 'reset' });
  });
  // a guild that is left takes its house with it
  let lastGuild = guildOf(ctx)?.ownGuildId() ?? 0;
  guildOf(ctx)?.store.subscribe(() => {
    const g = guildOf(ctx)?.ownGuildId() ?? 0;
    if (g === lastGuild) return;
    const left = g === 0 || lastGuild !== 0;
    lastGuild = g;
    if (left) apply({ kind: 'reset' });
  });

  // ---- the feature ---------------------------------------------------------------------------------------------------------------
  const inHouse = (): boolean => isGuildHouseWorld(worldStore.getState().worldId);
  const canManage = (): boolean =>
    inHouse() && guildOf(ctx)?.hasPower(PF_GUILDHOUSE_FURNITURE) === true;
  const rowViews = (): GhRowView[] =>
    ghRowViews(store.getState().guildHouse.house?.rows ?? [], ctx.data);
  const rowAt = (slot: number): GhRow | null =>
    store.getState().guildHouse.house?.rows[slot] ?? null;
  const select = (objId: number | null): void => {
    apply({ kind: 'select', objId });
  };
  const setMode = (mode: 0 | 2 | 4): void => {
    apply({ kind: 'mode', mode });
  };
  const sendPacket = (op: number, slot: number, over: Partial<PacketInfo> = {}): void => {
    const r = rowAt(slot);
    if (!r) return;
    senders.packet(op, slot, { ...r, ...over });
  };
  const chatTid = (tid: number): void => {
    sysChat(fc.texts, fc.chat, tid, []);
  };

  const openUpkeep = (): void => {
    // `IsAuthority( PF_GUILDHOUSE_UPKEEP )`
    if (guildOf(ctx)?.hasPower(PF_GUILDHOUSE_UPKEEP) !== true) {
      chatTid(TID_GAME_GUILDHOUSE_TAX_LEVEL);
      return;
    }
    // `SAFE_DELETE( m_pWndUpkeep ); new CWndGHUpkeep`: an open box is replaced
    fullUi(ctx)?.housing.openUpkeep({
      pay: (days) => {
        // legacy sends NULL_ID as the index; the server ignores it (guild-house seam), 0 works
        senders.packet(GH_OP.Upkeep, 0, bareInfo(days));
      },
    });
  };

  /** A row button of the storage window (`CWndGuildHousing::OnChildNotify`). */
  const rowButton = (row: GhRowView): void => {
    // a channel that cannot install: the line and nothing else
    if (!store.getState().guildHouse.furnitureChannel) {
      chatTid(TID_GAME_GUILDHOUSE_INSIDE_INSTALL_CHANNEL);
      return;
    }
    if (row.installed) {
      sendPacket(GH_OP.Remove, row.slot);
      return;
    }
    if (row.kind3 === IK3_TELEPORTER || row.kind3 === IK3_WALLPAPER || row.kind3 === IK3_CARPET) {
      sendPacket(GH_OP.Setup, row.slot);
      return;
    }
    // a furniture: the placement; the world takes the focus and the window hides
    if (deployOf(ctx)?.beginGuild(row.slot, 2) === true) fullUi(ctx)?.wm.root.setFocus();
  };

  const feature: GuildHouseFeature = {
    store,
    senders,
    state: () => store.getState().guildHouse,
    inHouse,
    canManage,
    rowViews,
    select,
    setMode,
    rowButton,
    sendPacket,
    rowAt,
    openUpkeep,
  };
  features.set(ctx, feature);

  // ---- the NPC menu ----------------------------------------------------------------------------------------------------------------
  const table = guildHouseMenuTable({
    buy: () => {
      senders.buy();
    },
    charge: openUpkeep,
    enter: () => {
      senders.enter();
    },
    goOut: () => {
      senders.goOut();
    },
  });
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      fn();
    });
  }

  // ---- the furniture menu and the selection by click -----------------------------------------------------------------------------
  const scale = (): number => fullUi(ctx)?.scaler.scale ?? 1;
  inputProvidersOf(ctx).ctrlMenu = (id: number, anchor: ScreenRect | null): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const g = store.getState().guildHouse;
    const slot = g.house ? slotOfObj(g.house.rows, id) : -1;
    // a ctrl without a row of the house gets no menu (`FindIndex < 0`)
    if (slot < 0) return;
    const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
    const menu = ui.npc.moverMenu;
    menu.onPick = (pick) => {
      runCtrlMenu(pick, slot, {
        info: () => {
          if (!ui.wm.isOpen(APP_GH_FURNITURE_STORAGE)) ui.housing.openStorage('furniture');
        },
        reinstall: (s) => {
          // `GuildHouse->Reset()`: the row selected by the click, its position and angle
          if (deployOf(ctx)?.beginGuild(s, 4) === true) ui.wm.root.setFocus();
        },
        recall: (s) => {
          sendPacket(GH_OP.Remove, s);
          select(null);
          setMode(0);
        },
      });
    };
    const at = anchor
      ? { x: Math.trunc((anchor.x + anchor.w) / scale()), y: Math.trunc(anchor.y / scale()) }
      : { x: 0, y: 0 };
    menu.show(ctrlMenuItems(text), at);
  };
  inputProvidersOf(ctx).guildFurniture = {
    select: (id) => {
      select(id);
      fullUi(ctx)?.housing.refresh(APP_GH_FURNITURE_STORAGE);
    },
    clear: () => {
      if (store.getState().guildHouse.selectedObjId !== null) select(null);
      fullUi(ctx)?.housing.refresh(APP_GH_FURNITURE_STORAGE);
    },
  };

  // ---- the windows ------------------------------------------------------------------------------------------------------------------
  const mainRequest: GhMainRequest = {
    upkeepAt: () => store.getState().guildHouse.house?.upkeepAt ?? 0,
    nowSec,
    toggleSetupPower: () => {
      const ui = fullUi(ctx);
      if (!ui) return;
      if (ui.wm.isOpen(APP_GUILD)) ui.wm.close(APP_GUILD);
      else guildOf(ctx)?.openGuild(4);
    },
    toggleStorage: (section) => {
      fullUi(ctx)?.housing.toggleStorage(section);
    },
    toggleBank: () => {
      const ui = fullUi(ctx);
      if (!ui) return;
      // the window's `onDestroy` sends `GuildBankWndClose` once; calling `senders.closeBank()` as well would send it twice
      if (ui.wm.isOpen(APP_GUILD_BANK)) ui.guild.closeBank();
      else guildOf(ctx)?.senders.openBank();
    },
    toggleUpkeep: () => {
      const ui = fullUi(ctx);
      if (!ui) return;
      if (guildOf(ctx)?.hasPower(PF_GUILDHOUSE_UPKEEP) !== true) {
        chatTid(TID_GAME_GUILDHOUSE_TAX_LEVEL);
        return;
      }
      if (ui.housing.isUpkeepOpen()) ui.housing.closeUpkeep();
      else openUpkeep();
    },
    destroyed: () => {
      const ui = fullUi(ctx);
      ui?.housing.storage()?.destroy();
      if (ui?.wm.isOpen(APP_GUILD) === true) ui.wm.close(APP_GUILD);
      if (ui?.wm.isOpen(APP_GUILD_BANK) === true) ui.guild.closeBank();
      const d = deployOf(ctx);
      if (d?.state()?.kind === 'guild') d.cancel();
    },
  };
  const deployingRow = (): GhRowView | null => {
    const d = deployOf(ctx)?.state();
    if (d?.kind !== 'guild' || d.mode !== 2) return null;
    return rowViews().find((r) => r.slot === d.slot) ?? null;
  };
  const storagePort: StoragePort = {
    rows: rowViews,
    nowSec,
    upkeepAt: () => store.getState().guildHouse.house?.upkeepAt ?? 0,
    channelOn: () => store.getState().guildHouse.furnitureChannel,
    canManage,
    selectedObjId: () => store.getState().guildHouse.selectedObjId,
    select: (objId) => {
      select(objId);
    },
    rowButton,
    deployingRow,
    closed: () => undefined,
  };

  /** Opens the menu with the world 212-214 and closes it when the world is left (`InProcessing` / `OutProcessing`). */
  const syncMain = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const want = ctx.machine.screen.s === 'InGame' && inHouse();
    const open = ui.wm.isOpen(APP_GH_MAIN);
    if (want && !open) ui.housing.openMain(mainRequest);
    else if (!want && open) ui.housing.closeMain();
  };
  worldStore.subscribe(syncMain);
  store.subscribe((s, prev) => {
    // a placement that starts or ends hides or shows the storage window (hidden frames are not processed)
    if (s.guildHouse.mode !== prev.guildHouse.mode) fullUi(ctx)?.housing.syncDeploy();
  });
  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (!installed.has(ui)) {
      installed.add(ui);
      ui.housing.setStoragePort(storagePort);
    }
    syncMain();
  };
  ctx.tickSystems.register('events.guildHouse.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // ---- the probe ---------------------------------------------------------------------------------------------------------------------
  addProbeMember(ctx, 'guildHouse', () => {
    const s = store.getState().guildHouse;
    return {
      furnitureChannel: s.furnitureChannel,
      house: s.house
        ? { ...s.house, rows: s.house.rows.map((r) => ({ ...r, pos: [...r.pos] })) }
        : null,
      restPoint: s.restPoint,
      restPercent: s.restPercent,
      selectedObjId: s.selectedObjId,
      mode: s.mode,
    };
  });
  return feature;
}
