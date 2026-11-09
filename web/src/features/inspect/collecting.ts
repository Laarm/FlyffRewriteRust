// The `collecting` feature (Plan/45 §9.8, 19 §14; `CCollectingWnd`, `CMover::ProcessCollecting`, `CDPClient::OnStartCollecting`; ref/99 §4.7;
// WP-CFEAT-29): the panel of the collector weapon.
//
//   window         `APP_COLLECTING` opens whenever the right hand holds a collector (`ProcessCollecting` calls `OpenCollecting()` every frame)
//                  and closes when it does not (`CloseCollecting()`)
//   button         `QueryStartCollecting` (refused while flying: `TID_GAME_COLLECT_FLY`) or `QueryStopCollecting`; the caption follows the
//                  server's `StartCollecting` / `StopCollecting` of the own character (FIX: the legacy flipped a local flag on every click,
//                  so a refused start left the button saying Stop)
//   StartCollecting  the own character: the line `TID_GAME_START_COLLECTING` and the Stop caption
//   RestartCollecting one item was collected: the line `TID_GAME_REAPITEM` with the item name
//   battery        `m_nHitPoint` of the collector over `nMaxBattery` of `collecting.inc` (1800); the level is its `GetAbilityOption()` over 5

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { PARTS_RWEAPON } from '../../gen/defs/neuz';
import {
  TID_GAME_COLLECT_FLY,
  TID_GAME_REAPITEM,
  TID_GAME_START_COLLECTING,
} from '../../gen/defs/text';
import {
  QueryStartCollectingSchema,
  QueryStopCollectingSchema,
} from '../../gen/proto/flyff/v1/inventory_pb';
import { APP_COLLECTING } from '../../gen/defs/resdata';
import type { CollectingPorts, CollectingView, ViewSource } from '../../ui/windows/nav/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { formatTid, SYSTEM_CHAT, sysChat } from '../common/text';
import { isCollector } from '../inventory/info';
import { inventoryOf } from '../inventory/window/inventoryModule';

/** `nMaxBattery` of `Resource/collecting.inc` (`CCollectingProperty::GetMaxBattery`): not in any client bundle. */
export const COLLECTOR_MAX_BATTERY = 1800;

export interface CollectingFeature {
  readonly ports: CollectingPorts;
  /** The button: start or stop, as the server's last answer says. Returns what was sent (`flying` = refused). */
  toggle(): 'start' | 'stop' | 'flying';
  view(): CollectingView;
}

const features = new WeakMap<ClientContext, CollectingFeature>();

/** The collecting feature of a context; `undefined` before the module registered. */
export function collectingOf(ctx: ClientContext): CollectingFeature | undefined {
  return features.get(ctx);
}

export const collectingModule: FeatureModule = {
  id: 'inventory',
  milestone: 4,
  register(fc) {
    registerCollecting(fc);
  },
};

function registerCollecting(fc: FeatureContext): CollectingFeature {
  const ctx = fc.app;
  let collecting = false;
  let version = 0;
  const listeners = new Set<() => void>();
  const changed = (): void => {
    version += 1;
    for (const fn of [...listeners]) fn();
  };

  const collector = () => {
    const it = inventoryOf(ctx)?.equipped(PARTS_RWEAPON) ?? null;
    return it && isCollector(it.itemId) ? it : null;
  };

  let last: CollectingView | null = null;
  const view = (): CollectingView => {
    const it = collector();
    const next: CollectingView = {
      version,
      hasCollector: it !== null,
      level: it?.abilityOption ?? 0,
      battery: Math.max(0, it?.hitPoint ?? 0),
      batteryMax: COLLECTOR_MAX_BATTERY,
      collecting,
    };
    if (
      last?.hasCollector === next.hasCollector &&
      last.level === next.level &&
      last.battery === next.battery &&
      last.collecting === next.collecting
    ) {
      return last;
    }
    last = next;
    return next;
  };

  const source: ViewSource<CollectingView> = {
    get: view,
    subscribe: (fn) => {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
  };

  const toggle = (): 'start' | 'stop' | 'flying' => {
    if (collecting) {
      // legacy-send: SendQueryStopCollecting
      ctx.net.sender.send({
        case: 'queryStopCollecting',
        value: create(QueryStopCollectingSchema, {}),
      });
      return 'stop';
    }
    if (ctx.stores.selfStore.getState().isFlying) {
      sysChat(fc.texts, fc.chat, TID_GAME_COLLECT_FLY);
      return 'flying';
    }
    // legacy-send: SendQueryStartCollecting
    ctx.net.sender.send({
      case: 'queryStartCollecting',
      value: create(QueryStartCollectingSchema, {}),
    });
    return 'start';
  };

  const ports: CollectingPorts = { view: source, actions: { toggle } };

  // ---- the server's answers --------------------------------------------------------------------------------------------------

  const isSelf = (objId: number): boolean => ctx.entities.get(objId)?.isSelf === true;

  fc.net.on('startCollecting', (_m, objId) => {
    if (!isSelf(objId)) return;
    sysChat(fc.texts, fc.chat, TID_GAME_START_COLLECTING);
    collecting = true;
    changed();
  });
  fc.net.on('stopCollecting', (_m, objId) => {
    if (!isSelf(objId)) return;
    collecting = false;
    changed();
  });
  fc.net.on('restartCollecting', (m) => {
    const prop = ctx.data?.item(m.itemId);
    if (!prop) return;
    fc.chat?.putString(
      formatTid(fc.texts, TID_GAME_REAPITEM, [prop.name]),
      null,
      fc.texts.textColor(TID_GAME_REAPITEM),
      SYSTEM_CHAT,
    );
  });

  // ---- the window follows the collector in the right hand ---------------------------------------------------------------------------

  const sync = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (collector()) {
      if (!ui.wm.isOpen(APP_COLLECTING) && ctx.machine.screen.s === 'InGame')
        ui.wm.open(APP_COLLECTING);
    } else {
      if (collecting) {
        collecting = false;
        changed();
      }
      if (ui.wm.isOpen(APP_COLLECTING)) ui.wm.close(APP_COLLECTING);
    }
  };

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.nav.setPorts({ collecting: ports });
  };
  const inv = inventoryOf(ctx);
  inv?.store.subscribe(() => {
    sync();
    for (const fn of [...listeners]) fn();
  });
  ctx.tickSystems.register('collecting.ui', () => {
    ensureUi();
  });
  ctx.machine.subscribe((screen, previous) => {
    ensureUi();
    if (screen.s === 'InGame' && previous.s !== 'InGame') sync();
    if (previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
      collecting = false;
      changed();
    }
  });
  ensureUi();

  addProbeMember(ctx, 'collecting', () => {
    const w = fullUi(ctx)?.nav.collecting() ?? null;
    return { open: w !== null, view: view(), button: w?.startButton?.state.get().texture ?? null };
  });

  const feature: CollectingFeature = { ports, toggle, view };
  features.set(ctx, feature);
  return feature;
}
