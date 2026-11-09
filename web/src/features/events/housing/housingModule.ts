// The `housing` feature (Plan/45 §28.10, §28.11; WP-CFEAT-46; server half `event/housing/`): the personal mini room.
//
//   messages     `HousingAllInfo`, `HousingFurnitureList`, `HousingSetupFurnitureNtf`, `HousingPaperingInfo`, `HousingSetVisitAllowNtf`, `HousingVisitableList` -> `eventStore.housing` (`reducers.ts`, pure); every list is applied
//                whether or not a window is open
//   windows      the applet `APP_HOUSING` (the furniture list: ports over the slice), `APP_MINIROOM_LIST` (menu 123), `APP_QUIT_ROOM` (menu 124): requests over `ui.housing`
//   menu         122 `MMI_VISIT_MYROOM` -> `HousingVisitRoom{ self }` (`myRoom` = true), 123, 124 (`menu.ts`)
//   messenger    `messengerOf( ctx ).setVisitPorts( ... )`: the friend menu allows or forbids my house, the rows of the allowed friends are green
//   deploy       `deployOf( ctx )` places a furniture (the `BUTTON1` of the list); the confirmed click sends `HousingSetupFurnitureReq` through `housing.senders`
//   probe        `__flyffDebug.features.housing()`

import type { ClientContext } from '../../../app/context';
import { APP_HOUSING, APP_MINIROOM_LIST, APP_QUIT_ROOM } from '../../../gen/defs/resdata';
import { type HousingInfo } from '../../../gen/proto/flyff/v1/housing_pb';
import { wallNow } from '../../../platform/time';
import type { HousingPort, RoomListRequest } from '../../../ui/windows/housing/ports';
import type { HousingItemView } from '../../../ui/windows/housing/rows';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { playerDataOf } from '../../common/playerData';
import { sysChat } from '../../common/text';
import { messengerOf } from '../../messenger/api';
import { menuOf } from '../../npc/menuModule';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import type { EventStore } from '../store';
import { deployOf } from '../deploy/api';
import { housingMenuTable } from './menu';
import {
  type HousingEffect,
  type HousingEnv,
  type HousingMsg,
  type PropFacts,
  reduceHousing,
} from './reducers';
import { housingSenders, type HousingSenders } from './senders';
import { type HousingRow, type HousingSlice, initialHousing, WORLD_MINIROOM } from './slice';
import { registerHouseProbe } from './probe';
import { visitListeners, visitPorts } from './visit';

/** The public face of the housing feature of a context. */
export interface HousingFeature {
  readonly store: EventStore;
  readonly senders: HousingSenders;
  /** The slice now. */
  state(): HousingSlice;
  /** `CDeployManager::IsMyRoom` (a client guess). */
  isMyRoom(): boolean;
  /** `MMI_VISIT_MYROOM`. */
  visitMyRoom(): void;
  /** `MMI_VISIT_FRIEND`: the room list window. */
  openRoomList(): void;
  /** `MMI_RETURNTO_WORLD`: the quit box. */
  openQuitRoom(): void;
  /** The confirmed click of a placement: `HousingSetupFurnitureReq{ setup: true, pos, angle }`. */
  place(itemId: number, pos: readonly [number, number, number], angle: number): void;
  /** The rows of the furniture list as the window shows them (the props resolved). */
  itemViews(): HousingItemView[];
}

const features = new WeakMap<ClientContext, HousingFeature>();

/** The housing feature of a context; `undefined` before the module registered. */
export function housingOf(ctx: ClientContext): HousingFeature | undefined {
  return features.get(ctx);
}

const rowOf = (i: HousingInfo): HousingRow => ({
  itemId: i.itemId,
  expireAt: Number(i.expireAt),
  setup: i.setup,
  pos: [i.pos?.x ?? 0, i.pos?.y ?? 0, i.pos?.z ?? 0],
  angle: i.angle,
});

/** Registers the housing feature (after the events feature: `houseM7.ts` registers the groups under the one handler module). */
export function registerHousing(fc: FeatureContext): HousingFeature {
  const ctx = fc.app;
  const known = housingOf(ctx);
  if (known) return known;
  const { selfStore, worldStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const playerData = playerDataOf(ctx);
  const senders = housingSenders((msg) => ctx.net.sender.send(msg));
  const listeners = visitListeners();
  const selfChar = (): number => selfStore.getState().idPlayer;

  // ---- the facts the reducers read ----------------------------------------------------------------------------------------
  const propOf = (itemId: number): PropFacts | undefined => {
    const p = ctx.data?.item(itemId);
    return p ? { name: p.name, kind2: p.item_kind2, kind3: p.item_kind3 } : undefined;
  };
  const env = (): HousingEnv => ({ prop: propOf, nameOf: (id) => playerData.nameOf(id) });

  // ---- carrying out the effects ---------------------------------------------------------------------------------------------
  const run = (effects: readonly HousingEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'chat':
          sysChat(fc.texts, fc.chat, e.tid, e.args);
          break;
        case 'refresh':
          fullUi(ctx)?.housing.refresh(e.app);
          break;
        case 'notifyVisitors':
          listeners.notify();
          break;
        case 'needAll':
          playerData.needAll(e.ids);
          break;
      }
    }
  };
  const apply = (msg: HousingMsg): void => {
    const cur = store.getState();
    const r = reduceHousing(cur.housing, msg, env());
    if (r.state !== cur.housing) store.setState({ housing: r.state });
    run(r.effects);
  };

  fc.net.on('housingAllInfo', (m) => {
    apply({ kind: 'allInfo', items: m.items.map(rowOf), visitors: m.visitors });
  });
  fc.net.on('housingFurnitureList', (m) => {
    if (m.info) apply({ kind: 'furnitureList', info: rowOf(m.info), add: m.add });
  });
  fc.net.on('housingSetupFurnitureNtf', (m) => {
    if (m.info) apply({ kind: 'setup', info: rowOf(m.info) });
  });
  fc.net.on('housingPaperingInfo', (m) => {
    apply({ kind: 'papering', itemId: m.itemId, setup: m.setup });
  });
  fc.net.on('housingSetVisitAllowNtf', (m) => {
    apply({ kind: 'visitAllow', target: m.target, allow: m.allow });
  });
  fc.net.on('housingVisitableList', (m) => {
    apply({ kind: 'visitable', owners: m.owners });
  });
  // `DPClient.cpp:2456-2466`: a world other than the mini room clears `IsMyRoom`
  fc.net.on('replace', (m) => {
    apply({ kind: 'replace', worldId: m.worldId });
  });
  // the room of a character that left the world goes with it (a world change keeps it)
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    fullUi(ctx)?.housing.closeAll();
    store.setState({ housing: initialHousing() });
  });

  // ---- the feature ----------------------------------------------------------------------------------------------------------------
  const itemViews = (): HousingItemView[] => {
    const out: HousingItemView[] = [];
    for (const r of store.getState().housing.items) {
      const p = ctx.data?.item(r.itemId);
      if (!p) continue;
      out.push({
        itemId: r.itemId,
        name: p.name,
        description: p.description,
        kind3: p.item_kind3,
        expireAt: r.expireAt,
        installed: r.setup,
      });
    }
    return out;
  };

  const visitMyRoom = (): void => {
    const self = selfChar();
    apply({ kind: 'visitSent', owner: self, self });
    senders.visitRoom(self);
  };
  const visit = (owner: number): void => {
    apply({ kind: 'visitSent', owner, self: selfChar() });
    senders.visitRoom(owner);
  };
  const roomListRequest = (): RoomListRequest => ({
    owners: () => store.getState().housing.visitable,
    nameOf: (id) => playerData.nameOf(id),
    opened: () => {
      apply({ kind: 'clearVisitable' });
      senders.reqVisitableList();
    },
    refresh: () => {
      senders.reqVisitableList();
    },
    visit,
  });
  const openRoomList = (): void => {
    fullUi(ctx)?.housing.openRoomList(roomListRequest());
  };
  const openQuitRoom = (): void => {
    fullUi(ctx)?.housing.openQuitRoom({
      goOut: () => {
        senders.goOut();
      },
    });
  };
  const place = (itemId: number, pos: readonly [number, number, number], angle: number): void => {
    senders.reqSetup({ itemId, setup: true, pos, angle, expireAt: 0 });
  };

  const feature: HousingFeature = {
    store,
    senders,
    state: () => store.getState().housing,
    isMyRoom: () => store.getState().housing.myRoom,
    visitMyRoom,
    openRoomList,
    openQuitRoom,
    place,
    itemViews,
  };
  features.set(ctx, feature);

  // ---- the NPC menu ----------------------------------------------------------------------------------------------------------------
  const table = housingMenuTable({ visitMyRoom, openRoomList, openQuitRoom });
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      fn();
    });
  }

  // ---- the messenger: who may visit my house ------------------------------------------------------------------------------
  messengerOf(ctx)?.setVisitPorts(visitPorts(() => store.getState().housing, senders, listeners));

  // ---- the applet's port ---------------------------------------------------------------------------------------------------------
  const port: HousingPort = {
    items: itemViews,
    nowSec: () => Math.floor(wallNow() / 1000),
    canEdit: () =>
      worldStore.getState().worldId === WORLD_MINIROOM && store.getState().housing.myRoom,
    deploying: () => deployOf(ctx)?.active() ?? false,
    endDeploy: () => {
      deployOf(ctx)?.cancel();
    },
    installPaper: (itemId) => {
      senders.reqSetup({ itemId, setup: true, pos: [0, 0, 0], angle: 0, expireAt: 0 });
    },
    deploy: (itemId) => {
      deployOf(ctx)?.beginHousing(itemId);
      // `pWndWorld->SetFocus()`: the keyboard and the wheel belong to the world now
      fullUi(ctx)?.wm.root.setFocus();
    },
    remove: (itemId) => {
      senders.reqSetup({ itemId, setup: false, pos: [0, 0, 0], angle: 0, expireAt: 0 });
    },
  };
  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.housing.setHousingPort(port);
  };
  ctx.tickSystems.register('events.housing.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // ---- the probe ----------------------------------------------------------------------------------------------------------------------
  registerHouseProbe(ctx);
  addProbeMember(ctx, 'housing', () => {
    const s = store.getState().housing;
    return {
      items: s.items.map((r) => ({ ...r, pos: [...r.pos] })),
      visitors: [...s.visitors],
      visitable: [...s.visitable],
      papering: { ...s.papering },
      myRoom: s.myRoom,
      windows: [APP_HOUSING, APP_MINIROOM_LIST, APP_QUIT_ROOM].filter((app) =>
        fullUi(ctx)?.wm.isOpen(app),
      ),
    };
  });
  return feature;
}
