// The `inspect` feature (Plan/45 §9.8, 15 §10.7; ref/09b `OnQueryEquip`; WP-CFEAT-29): looking at another player's equipment.
//
//   MMI_QUERYEQUIP        the player menu entry sends `QueryEquipReq{target}` (only for a player)
//   QueryEquipNtf         the answer: the base of each part is the mover's visual equipment, the answer adds the awakening word, the sockets
//                         and the element; `APP_QUERYEQUIP` opens (an open inspection is replaced); its tooltips use the flag `APP_QUERYEQUIP`
//                         and evaluate the requirement lines for the INSPECTED player (the tooltip feature's `setInspected`)
//   QueryEquipSetting     the option "allow inspection" (`setAllow`) flips `EQUIP_DENIAL_MODE` on the server
//
// The inspected player leaving the view closes the window (the window polls `gone`).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { EntityKind } from '../../game/entities';
import { PARTS_RWEAPON } from '../../gen/defs/neuz';
import { MMI_QUERYEQUIP } from '../../gen/defs/neuz';
import { QueryEquipReqSchema } from '../../gen/proto/flyff/v1/inventory_pb';
import { QueryEquipSettingSchema } from '../../gen/proto/flyff/v1/character_pb';
import type { SpawnPlayer } from '../../gen/proto/flyff/v1/world_pb';
import { mutableView, type QueryEquipPorts, type QueryEquipView } from '../../ui/windows/nav/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { menuOf } from '../npc/menuModule';
import { corePreviewPortOf } from '../character/window/previews';
import { tooltipOf } from '../tooltip/module';
import { jobTypeByLayout } from '../tooltip/jobs';
import { buildEquip, type EquipExtra, type WornPart } from './model';

export interface InspectFeature {
  /** `SendQueryEquip( objId )`; returns `false` for a target that is not a player in view. */
  inspect(objId: number): boolean;
  /** `SendQueryEquipSetting( allow )`: the option "allow inspection". */
  setAllow(allow: boolean): void;
  /** The object id of the player whose window is open, `null` for none. */
  current(): number | null;
}

const features = new WeakMap<ClientContext, InspectFeature>();

/** The inspect feature of a context; `undefined` before the module registered. */
export function inspectOf(ctx: ClientContext): InspectFeature | undefined {
  return features.get(ctx);
}

export const inspectModule: FeatureModule = {
  id: 'inventory',
  milestone: 4,
  register(fc) {
    registerInspect(fc);
  },
};

function registerInspect(fc: FeatureContext): InspectFeature {
  const ctx = fc.app;
  let currentId: number | null = null;
  let answers = 0;

  const wornOf = (objId: number): Map<number, WornPart> => {
    const out = new Map<number, WornPart>();
    const rec = ctx.entities.get(objId);
    for (const [part, e] of rec?.visual?.equip ?? []) {
      out.set(part, { itemId: e.itemId, flag: e.flag, option: e.option });
    }
    return out;
  };

  const inspect = (objId: number): boolean => {
    const rec = ctx.entities.get(objId);
    if (!rec || rec.removed || rec.kind !== EntityKind.Player) return false;
    // legacy-send: SendQueryEquip
    ctx.net.sender.send({
      case: 'queryEquipReq',
      value: create(QueryEquipReqSchema, { target: objId }),
    });
    return true;
  };

  menuOf(ctx)?.registerAction(MMI_QUERYEQUIP, ({ targetId }) => {
    inspect(targetId);
  });

  fc.net.on('queryEquipNtf', (m, objId) => {
    const ui = fullUi(ctx);
    const rec = ctx.entities.get(objId);
    if (!ui || !rec || rec.removed || rec.kind !== EntityKind.Player) return;
    const extras: EquipExtra[] = m.entries.map((e) => ({
      part: e.part,
      randomOptItemId: e.randomOptItemId,
      piercing: e.piercing,
      itemResist: e.itemResist,
      resistAbilityOption: e.resistAbilityOption,
    }));
    answers += 1;
    const tag = `inspect${String(answers)}`;
    const build = (): QueryEquipView => {
      const live = ctx.entities.get(objId);
      const gone = !live || live.removed;
      const equip = buildEquip(wornOf(objId), extras, (id) => ctx.data?.item(id), tag);
      return {
        version: answers,
        name: live?.visual?.name ?? '',
        gone,
        equip,
        subject: live?.visual,
        hideCoat: live?.visual?.hideCoat ?? [false, false, false, false, false],
      };
    };
    // the requirement lines of the tooltips are the inspected player's (while the pointer is over an item of the window)
    const player = (rec.spawn as SpawnPlayer).player;
    const job = player?.job ?? 0;
    const initial = build();
    const owner = {
      info: {
        sex: player?.sex ?? rec.visual?.sex ?? 0,
        job,
        level: player?.level ?? 1,
        jobType: jobTypeByLayout(job) ?? 0,
      },
      worn: initial.equip.flatMap((it) =>
        it ? [{ part: it.part, itemId: it.itemId, expired: it.expired }] : [],
      ),
      rweaponItem: initial.equip[PARTS_RWEAPON]?.itemId ?? 0,
    };
    const view = mutableView<QueryEquipView>(initial);
    const ports: QueryEquipPorts = {
      view: {
        get: () => {
          // the mover may have left: the window closes on `gone`
          const live = ctx.entities.get(objId);
          const g = !live || live.removed;
          if (g !== view.get().gone) view.set({ ...view.get(), gone: g });
          return view.get();
        },
        subscribe: (fn) => view.subscribe(fn),
      },
      previews: corePreviewPortOf(ctx),
      tooltipScope: (on) => {
        tooltipOf(ctx)?.setInspected(on ? owner : null);
      },
      closed: () => {
        if (currentId === objId) currentId = null;
      },
    };
    currentId = objId;
    ui.nav.openQueryEquip(ports);
  });

  const feature: InspectFeature = {
    inspect,
    setAllow(allow) {
      // legacy-send: SendQueryEquipSetting
      ctx.net.sender.send({
        case: 'queryEquipSetting',
        value: create(QueryEquipSettingSchema, { allow }),
      });
    },
    current: () => currentId,
  };
  features.set(ctx, feature);

  // the e2e hooks: the fallback of the menu entry "Inspect" (`MMI_QUERYEQUIP` runs the same call) and the option "allow inspection"
  // (a new character refuses it: `EQUIP_DENIAL_MODE` is set)
  addProbeMember(ctx, 'inspectPlayer', (objId: number) => inspect(objId));
  addProbeMember(ctx, 'inspectAllow', (allow: boolean) => {
    feature.setAllow(allow);
    return true;
  });
  addProbeMember(ctx, 'inspect', () => {
    const w = fullUi(ctx)?.nav.queryEquip() ?? null;
    return {
      open: w !== null,
      target: currentId,
      answers,
      /** The name of the mover the window shows (the preview's subject). */
      subject: (w?.equipView?.subject as { name?: string } | undefined)?.name ?? null,
      parts: (w?.equipView?.equip ?? []).flatMap((it) =>
        it ? [{ part: it.part, itemId: it.itemId, name: it.name }] : [],
      ),
    };
  });
  return feature;
}
