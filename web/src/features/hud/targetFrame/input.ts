// The target frame's input from the entity table (`targetFrameModel` takes plain records): the mover under the render target (or
// the focus), its level, job and authority, the monster prop and the viewer. The level and job of another player come from
// the spawn message and from the `SetLevel` / `JobChanged` messages that followed (`overrides`).

import type { ClientContext } from '../../../app/context';
import { EntityKind, type EntityRecord } from '../../../game/entities';
import type { SpawnNpc, SpawnPlayer } from '../../../gen/proto/flyff/v1/world_pb';
import type { TargetInput } from './model';

/** `TRANSPARENT_MODE` of the mode word. */
export const TRANSPARENT_MODE = 0x2;

/** Level and job updates of other players since their spawn. */
export interface TargetOverrides {
  readonly level: ReadonlyMap<number, number>;
  readonly job: ReadonlyMap<number, number>;
}

/** The input record of a mover entity; `null` for anything that is not a mover (an item, a ctrl). */
export function targetInputOf(
  ctx: ClientContext,
  rec: EntityRecord,
  overrides: TargetOverrides,
): TargetInput | null {
  const v = rec.visual;
  if (rec.kind !== EntityKind.Player && rec.kind !== EntityKind.Npc) return null;
  const common = {
    id: rec.objId,
    invisible: ((v?.mode ?? 0) & TRANSPARENT_MODE) !== 0,
    hp: rec.hp,
    activeAttack: v?.activeAttack ?? false,
  };
  if (rec.kind === EntityKind.Player) {
    const p = (rec.spawn as SpawnPlayer).player;
    return {
      ...common,
      player: true,
      peaceful: false,
      authority: v?.authorization ?? p?.authorization ?? 0,
      name: v?.name ?? p?.name ?? '',
      level: overrides.level.get(rec.objId) ?? p?.level ?? 1,
      job: overrides.job.get(rec.objId) ?? p?.job ?? 0,
      maxHp: rec.maxHp ?? 0,
      element: 0,
    };
  }
  const prop = ctx.data?.mover(rec.propId);
  const npc = rec.spawn as SpawnNpc;
  return {
    ...common,
    player: false,
    peaceful: v?.peaceful ?? false,
    authority: 0,
    name: v?.name && v.name !== '' ? v.name : (prop?.name ?? ''),
    level: prop?.level ?? 1,
    job: 0,
    maxHp: rec.maxHp ?? Math.max(rec.hp, npc.mover?.hp ?? 0),
    element: prop?.element_type ?? 0,
    hideLevel: prop?.hide_level === 1,
  };
}
