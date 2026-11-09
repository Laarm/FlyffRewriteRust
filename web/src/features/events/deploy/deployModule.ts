// The `deploy` feature (Plan/45 §28.10, §28.11, Plan/43 §7.13; WP-CFEAT-46, WP-INPUT-17): wires the placement controller to the game.
//
//   controller   `deployOf( ctx )` (`api.ts`): the housing list and the guild storage start a placement, the click of the world confirms it
//   input        `inputProvidersOf( ctx ).deploy`: `app/inputHost.ts` hands it to `InputHooks.deploy` (the routing is `input/deploy.ts`)
//   renderer     `ctx.render.deploy` (`render/objects/deployGhost.ts`), read at the moment of use
//   end          every `Replace`, a screen other than InGame, the destruction of `APP_GH_MAIN` and a change of the own guild (the original left a dangling ghost on a world change)
//   probe        `__flyffDebug.features.deploy()` (`scene`: the collision scene of the renderer is known); on `?e2e=1` pages and in dev builds also `deployBeginHousing( itemId )`, `deployBeginGuild( slot, mode )`, `deployMoveTo( [x, y, z] )`, `deployClick()`

import { inputProvidersOf } from '../../../app/inputProviders';
import type { Vec3LH } from '../../../render/types';
import { addProbeMember } from '../../common/probe';
import { sysChat } from '../../common/text';
import type { FeatureContext } from '../../common/module';
import { guildHouseOf } from '../guildHouse/guildHouseModule';
import { housingOf } from '../housing/housingModule';
import { type DeployFeature, deployOf, setDeployOf } from './api';
import { createDeployController, type DeployRow } from './controller';

export function registerDeploy(fc: FeatureContext): DeployFeature {
  const ctx = fc.app;
  const known = deployOf(ctx);
  if (known) return known;
  const guildRow = (slot: number): DeployRow | null => {
    const r = guildHouseOf(ctx)?.rowAt(slot);
    return r
      ? {
          itemId: r.itemId,
          pos: r.pos,
          angle: r.angle,
          ctrlObjId: r.ctrlObjId,
          expireAt: r.expireAt,
        }
      : null;
  };
  const controller = createDeployController({
    chat: (tid) => {
      sysChat(fc.texts, fc.chat, tid, []);
    },
    render: () => ctx.render?.deploy,
    heightAttr: (x, z) => ctx.sim.heightAttr(x, z),
    linkKind: (itemId) => ctx.data?.item(itemId)?.link_kind,
    placeHousing: (itemId, pos, angle) => {
      housingOf(ctx)?.place(itemId, pos, angle);
    },
    guild: {
      row: guildRow,
      selectedObjId: () => guildHouseOf(ctx)?.state().selectedObjId ?? null,
      send: (op, slot, info) => {
        guildHouseOf(ctx)?.senders.packet(op, slot, info);
      },
      select: (objId) => {
        guildHouseOf(ctx)?.select(objId);
      },
      setMode: (mode) => {
        guildHouseOf(ctx)?.setMode(mode);
      },
    },
    log: (message) => {
      ctx.log.debug('deploy', message);
    },
  });
  setDeployOf(ctx, controller);

  // the placement ends with the world
  fc.net.on('replace', () => {
    controller.cancel();
  });
  ctx.machine.subscribe((screen) => {
    if (screen.s !== 'InGame') controller.cancel();
  });

  // the input layer
  inputProvidersOf(ctx).deploy = {
    active: () => controller.active(),
    cursor: (p) => {
      controller.cursor(p);
    },
    confirm: () => {
      controller.confirm();
    },
    cancel: () => controller.cancel(),
    wheel: (n) => {
      controller.wheel(n);
    },
    pageKey: (code) => {
      if (code === 'PageUp' || code === 'PageDown') controller.pageKey(code);
    },
  };

  addProbeMember(ctx, 'deploy', () => {
    const s = controller.state();
    return {
      active: s !== null,
      kind: s?.kind ?? null,
      mode: s?.mode ?? 0,
      itemId: s?.itemId ?? 0,
      ctrlId: s?.ctrlId ?? 0,
      slot: s?.slot ?? -1,
      pos: s ? [...s.pos] : null,
      angle: s?.angle ?? 0,
      keyAngle: controller.keyAngle(),
      collide: s?.collide ?? false,
      scene: controller.sceneReady(),
      sent: controller.sent().map((x) => ({ ...x, pos: [...x.pos] })),
    };
  });
  if (ctx.probe.test !== undefined) {
    addProbeMember(ctx, 'deployBeginHousing', (itemId: number) => controller.beginHousing(itemId));
    addProbeMember(ctx, 'deployBeginGuild', (slot: number, mode: 2 | 4) =>
      controller.beginGuild(slot, mode),
    );
    addProbeMember(ctx, 'deployMoveTo', (p: Vec3LH) => {
      controller.moveTo([p[0], p[1], p[2]]);
    });
    addProbeMember(ctx, 'deployClick', () => {
      controller.click();
    });
  }
  return controller;
}
