// The buff loop effects (Plan/17 11.1 "SetSkillState ... loop effect `sfx_obj4` for the buff duration (0 = once)", Plan/41 17.3;
// `IBuff::CreateSFX`, buff.cpp:201-215; WP-SKILL-12 "buffs.ts"): a buff whose prop has an effect `sfx_obj4` shows it on the buffed mover for
// the duration of the buff.
//
// The buff mirror itself (the map with the equal / lower / higher level rules, the 28-skill cap, the timers) is `features/buffs`
// (`BuffSet`, WP-CFEAT-30), which the buff windows read; this module only owns the EFFECTS, with the same entry rules:
//
//   a new buff                          -> the effect is created once (`m_bSFX`), `nSec = total seconds` (0 for a permanent buff = once)
//   the same buff again, same level     -> nothing (the original keeps the object and only adds the time)
//   the same buff, a higher level       -> the old effect ends, a new one starts
//   the same buff, a lower level        -> ignored (the server re-sends the old values)
//   RemoveSkillInfluence                -> the effect ends (`RemoveAllSkillInfluence`: every skill buff and every uncharged item buff)
//   a mover that leaves / a new world   -> its effects are forgotten
//
// The buffs in a spawn list (`SpawnSelf`, `SpawnPlayer`, `SpawnNpc`) create their effects like a fresh `SetSkillState` (the client's
// `CBuffMgr::Process` runs for every mover).

import type { ClientContext } from '../../app/context';
import { Phase } from '../../net/registry';
import { validEffect } from '../fx/hitSpark';
import { BUFF } from './constants';

const BUFF_ITEM = BUFF.ITEM;
const BUFF_ITEM2 = BUFF.ITEM2;
const BUFF_SKILL = BUFF.SKILL;

const OWNER = 'buffFx';

interface Loop {
  readonly level: number;
  readonly handle: number;
}

export interface BuffFxStats {
  created: number;
  removed: number;
  /** Buffs whose prop has no loop effect. */
  noEffect: number;
}

export interface BuffFxRuntime {
  readonly stats: BuffFxStats;
  /** The movers with at least one live loop effect, and how many (the probe and the tests read it). */
  active(): ReadonlyMap<number, number>;
  /** A buff of `type` / `id` of mover `mover` got its effect. */
  has(mover: number, type: number, id: number): boolean;
}

const runtimes = new WeakMap<ClientContext, BuffFxRuntime>();

export function peekBuffFx(ctx: ClientContext): BuffFxRuntime | undefined {
  return runtimes.get(ctx);
}

/** `(int)(total / 1000.0)` seconds of the effect; a permanent buff (total 0) plays once. */
export function loopSeconds(totalMs: number): number {
  return totalMs <= 0 ? 0 : Math.trunc(totalMs / 1000);
}

function keyOf(type: number, id: number): number {
  return (((type & 0xffff) << 16) | (id & 0xffff)) >>> 0;
}

export function registerBuffFx(ctx: ClientContext): BuffFxRuntime {
  const known = runtimes.get(ctx);
  if (known) return known;
  const { registry } = ctx.net;
  const stats: BuffFxStats = { created: 0, removed: 0, noEffect: 0 };
  const loops = new Map<number, Map<number, Loop>>();
  const tmp: [number, number, number] = [0, 0, 0];

  /** The prop that carries the loop effect: a skill for skill buffs, an item for item buffs. */
  const effectOf = (type: number, id: number): number | undefined => {
    const prop =
      type === BUFF_SKILL
        ? ctx.data?.skill(id)
        : type === BUFF_ITEM || type === BUFF_ITEM2
          ? ctx.data?.item(id)
          : undefined;
    const xi = prop?.sfx_obj[3];
    return validEffect(xi) ? xi : undefined;
  };

  const posOf = (id: number): [number, number, number] | undefined => {
    const view = ctx.sim.view;
    if (view?.valid === true && view.posOf(id, tmp)) return [tmp[0], tmp[1], tmp[2]];
    const spawn = ctx.entities.get(id)?.spawn;
    const p = spawn && 'obj' in spawn ? spawn.obj?.pos : undefined;
    return p ? [p.x, p.y, p.z] : undefined;
  };

  const drop = (mover: number, key: number): void => {
    const m = loops.get(mover);
    const l = m?.get(key);
    if (!m || !l) return;
    ctx.render?.fx?.removeSfx(l.handle);
    m.delete(key);
    if (m.size === 0) loops.delete(mover);
    stats.removed += 1;
  };

  const add = (mover: number, type: number, id: number, level: number, totalMs: number): void => {
    const xi = effectOf(type, id);
    if (xi === undefined) {
      stats.noEffect += 1;
      return;
    }
    const key = keyOf(type, id);
    const old = loops.get(mover)?.get(key);
    if (old) {
      // same or lower level: nothing (the buff object stays); a higher level replaces the effect
      if (level <= old.level) return;
      drop(mover, key);
    }
    const fx = ctx.render?.fx;
    if (!fx) return;
    const pos = posOf(mover);
    const handle = fx.sfx(xi, {
      src: mover,
      dest: mover,
      ...(pos ? { pos } : {}),
      nSec: loopSeconds(totalMs),
      cls: 'skill',
    });
    let m = loops.get(mover);
    if (!m) {
      m = new Map();
      loops.set(mover, m);
    }
    m.set(key, { level, handle });
    stats.created += 1;
  };

  const forget = (mover: number): void => {
    const m = loops.get(mover);
    if (!m) return;
    for (const key of [...m.keys()]) drop(mover, key);
  };

  registry.on('setSkillState', Phase.Fx, OWNER, (objId, m) => {
    const mover = m.target !== 0 ? m.target : objId;
    add(mover, m.buffType, m.buffId, m.level, m.totalMs);
  });
  registry.on('removeSkillInfluence', Phase.Fx, OWNER, (objId, m) => {
    drop(objId, keyOf(m.buffType, m.buffId));
  });
  registry.on('removeAllSkillInfluence', Phase.Fx, OWNER, (objId) => {
    const m = loops.get(objId);
    if (!m) return;
    for (const key of [...m.keys()]) {
      const type = key >>> 16;
      const id = key & 0xffff;
      // `RemoveCommonBuffs`: skill buffs and the uncharged item buffs; the charged ones stay
      if (type === BUFF_SKILL || (ctx.data?.item(id)?.charged ?? 0) === 0) drop(objId, key);
    }
  });
  const spawnList = (
    mover: number,
    list: readonly { type: number; id: number; level: number; remainMs: number }[],
  ): void => {
    forget(mover);
    for (const b of list) add(mover, b.type, b.id, b.level, b.remainMs);
  };
  registry.on('spawnSelf', Phase.Fx, OWNER, (objId, m) => {
    spawnList(objId, m.buffs);
  });
  registry.on('spawnPlayer', Phase.Fx, OWNER, (objId, m) => {
    spawnList(objId, m.buffs);
  });
  registry.on('spawnNpc', Phase.Fx, OWNER, (objId, m) => {
    spawnList(objId, m.buffs);
  });
  registry.on('delObj', Phase.Fx, OWNER, (objId) => {
    forget(objId);
  });
  const clearAll = (): void => {
    for (const mover of [...loops.keys()]) forget(mover);
  };
  registry.on('replace', Phase.Fx, OWNER, clearAll);
  registry.on('worldReadInfo', Phase.Fx, OWNER, clearAll);

  const rt: BuffFxRuntime = {
    stats,
    active: () => new Map([...loops].map(([k, v]) => [k, v.size])),
    has: (mover, type, id) => loops.get(mover)?.has(keyOf(type, id)) === true,
  };
  runtimes.set(ctx, rt);
  return rt;
}
