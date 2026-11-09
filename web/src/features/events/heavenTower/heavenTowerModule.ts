// The `heavenTower` feature (Plan/45 §28.3; WP-CFEAT-52; `src:_Interface/WndWorld.cpp:5303-5318`, `WndField.cpp:22172-22445`): the menu entry of the "Forsaken Tower Manager" (`MaFl_HEAVENMAN`).
//
//   MMI_HEAVEN_TOWER (97)   while the window is open: the system line `TID_GAME_WND_OVERLAPED` ("You are already using this window!") and nothing else; else the window (ui-quest's shell, `ui.quest.openHeavenTower`)
//   enter( floor )          OK of the entrance confirmation for the floors 1 .. 5: `HeavenTowerTeleport{floor}` (the shell calls it for 1 .. 5 only; the guard here does not trust that)
//
// The client checks no gold (the server answers `TID_GAME_LACKMONEY`) and needs no quest (the tower is unconditional in V15).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { APP_HEAVEN_TOWER } from '../../../gen/defs/resdata';
import { MMI_HEAVEN_TOWER } from '../../../gen/defs/neuz';
import { TID_GAME_WND_OVERLAPED } from '../../../gen/defs/text';
import { HeavenTowerTeleportSchema } from '../../../gen/proto/flyff/v1/npc_pb';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { sysChat } from '../../common/text';
import { menuOf } from '../../npc/menuModule';
import { isTowerFloor } from './floors';

/** The public face of the Heaven Tower feature of a context. */
export interface HeavenTowerFeature {
  /** The floor of the last `HeavenTowerTeleport` sent, 0 before. */
  lastFloor(): number;
  /** OK of the confirmation: sends the floor (1 .. 5 only). */
  enter(floor: number): boolean;
  /** `MMI_HEAVEN_TOWER`. */
  openMenu(): void;
}

const features = new WeakMap<ClientContext, HeavenTowerFeature>();

/** The Heaven Tower feature of a context; `undefined` before the module registered. */
export function heavenTowerOf(ctx: ClientContext): HeavenTowerFeature | undefined {
  return features.get(ctx);
}

export function registerHeavenTower(fc: FeatureContext): HeavenTowerFeature {
  const ctx = fc.app;
  let lastFloor = 0;

  const enter = (floor: number): boolean => {
    if (!isTowerFloor(floor)) return false;
    lastFloor = floor;
    // legacy-send: SendTeleportToHeavenTower
    ctx.net.sender.send({
      case: 'heavenTowerTeleport',
      value: create(HeavenTowerTeleportSchema, { floor }),
    });
    return true;
  };

  const openMenu = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (ui.quest.isOpen(APP_HEAVEN_TOWER)) {
      // `TID_GAME_WND_OVERLAPED` in the colour of its text, nothing else
      sysChat(fc.texts, ui.chat, TID_GAME_WND_OVERLAPED);
      return;
    }
    ui.quest.openHeavenTower({
      enter: (floor) => {
        enter(floor);
      },
    });
  };

  menuOf(ctx)?.registerAction(MMI_HEAVEN_TOWER, () => {
    openMenu();
  });

  const feature: HeavenTowerFeature = {
    lastFloor: () => lastFloor,
    enter,
    openMenu,
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'heavenTower', () => ({
    open: fullUi(ctx)?.quest.isOpen(APP_HEAVEN_TOWER) ?? false,
    lastFloor,
  }));
  return feature;
}
