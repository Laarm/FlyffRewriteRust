// The buff map of a mover (`CBuffMgr`, `src:_Common/buff.cpp:623-870`, Plan/45 §12, Plan/17 §7.2; WP-CFEAT-30): the buffs keyed by
// `MAKELONG(id, type)` - iteration is by type, then id - with the client-side add rules
//
//   a buff of the same (type, id) and the same level      `AddTotal`: the total becomes max(new total, remaining) and the clock restarts
//                                                          (the feed pocket 02 and the pet tonics ADD the remaining time first)
//   ... of a lower level                                   the old one is removed and the new one added
//   ... of a higher level                                  the new one is ignored
//   a new skill buff while 28 skill buffs are there         the first skill buff that is no evil spell goes (`PrepareBS`)
//
// and the local expiry of `CBuffMgr::Process` (`IBuff::Timeover`; item2 buffs hold a unix expiry as their level; pet and equip buffs
// never expire on their own, a permanent item buff - `skill_time` 999999999 - neither).

import {
  II_SYS_SYS_SCR_PET_FEED_POCKET02,
  II_SYS_SYS_SCR_PET_TONIC_A,
  II_SYS_SYS_SCR_PET_TONIC_B,
} from '../../gen/defs/item';
import {
  BUFF_EQUIP,
  BUFF_ITEM,
  BUFF_ITEM2,
  BUFF_PET,
  BUFF_SKILL,
  MAX_SKILLBUFF_COUNT,
} from './types';

/** One buff. `total` and `inst` are client milliseconds; for `BUFF_ITEM2` `level` is the unix second of the expiry. */
export interface Buff {
  readonly type: number;
  readonly id: number;
  readonly level: number;
  readonly total: number;
  readonly inst: number;
}

/** What the buff rules need to know about the props (the module wires the data bundles). */
export interface BuffRules {
  /** `nEvildoing < 0` of the skill prop; `undefined` when the prop is unknown (such a buff is never evicted). */
  skillIsEvil(skillId: number): boolean | undefined;
  /** `dwSkillTime == 999999999` of the item prop: the buff never times out. */
  itemIsPermanent(itemId: number): boolean;
}

/** `MAKELONG(id, type)`. */
export function buffKey(type: number, id: number): number {
  return (((type & 0xffff) << 16) | (id & 0xffff)) >>> 0;
}

/** `IBuff::GetRemain()`: the milliseconds left, `0` when the clock never started or ran out. */
export function remainOf(b: Buff, now: number): number {
  if (b.inst === 0) return 0;
  const elapsed = now - b.inst;
  if (elapsed > b.total) return 0;
  return b.total - elapsed;
}

const ADDS_REMAINING = new Set<number>([
  II_SYS_SYS_SCR_PET_FEED_POCKET02,
  II_SYS_SYS_SCR_PET_TONIC_A,
  II_SYS_SYS_SCR_PET_TONIC_B,
]);

export class BuffSet {
  readonly #map = new Map<number, Buff>();
  /** Bumps on every change of the set (not of the time: the clock is the readers'). */
  version = 0;

  get size(): number {
    return this.#map.size;
  }

  /** The buffs in the order of the C++ map: by type, then id. */
  list(): Buff[] {
    return [...this.#map.entries()].sort((a, b) => a[0] - b[0]).map((e) => e[1]);
  }

  get(type: number, id: number): Buff | undefined {
    return this.#map.get(buffKey(type, id));
  }

  has(type: number, id: number): boolean {
    return this.#map.has(buffKey(type, id));
  }

  /** `CBuffMgr::AddBuff(type, id, level, total)` with the clock at `now`; returns whether a new entry was added. */
  add(
    type: number,
    id: number,
    level: number,
    total: number,
    now: number,
    rules: BuffRules,
  ): boolean {
    // `IsAddable`: the total, read as an `int`, must not be negative
    if ((total | 0) < 0) return false;
    const key = buffKey(type, id);
    // `m_tmInst == 0` means "the clock never started" (`GetRemain`): a clock reading of 0 is moved to 1 ms
    const stamp = Math.max(1, now);
    const old = this.#map.get(key);
    if (old) {
      if (old.level === level) {
        // `AddTotal` (the three pet items add what is left first)
        const add =
          ADDS_REMAINING.has(id) && type === BUFF_ITEM ? total + remainOf(old, now) : total;
        const next: Buff = { ...old, total: Math.max(add, remainOf(old, now)), inst: stamp };
        this.#map.set(key, next);
        this.version += 1;
        return false;
      }
      if (old.level < level) this.#map.delete(key);
      else return false; // a higher level stays
    }
    if (type === BUFF_SKILL) this.#prepareSkillSlot(rules);
    this.#map.set(key, { type, id, level, total: total >>> 0, inst: stamp });
    this.version += 1;
    return true;
  }

  /** `PrepareBS`: a full skill row makes room by dropping the first skill buff that is no evil spell. */
  #prepareSkillSlot(rules: BuffRules): void {
    let skills = 0;
    for (const b of this.#map.values()) if (b.type === BUFF_SKILL) skills += 1;
    if (skills < MAX_SKILLBUFF_COUNT) return;
    for (const b of this.list()) {
      if (b.type !== BUFF_SKILL) continue;
      if (rules.skillIsEvil(b.id) === false) {
        this.#map.delete(buffKey(b.type, b.id));
        return;
      }
    }
  }

  /** `RemoveBuff(type, id)`. */
  remove(type: number, id: number): boolean {
    const had = this.#map.delete(buffKey(type, id));
    if (had) this.version += 1;
    return had;
  }

  /** `RemoveCommonBuffs`: skill buffs and the uncharged item buffs go; pet, equip and charged item buffs stay. */
  removeCommon(isCharged: (itemId: number) => boolean): number {
    let n = 0;
    for (const b of this.list()) {
      const common =
        b.type === BUFF_SKILL ||
        ((b.type === BUFF_ITEM || b.type === BUFF_ITEM2) && !isCharged(b.id));
      if (common && this.#map.delete(buffKey(b.type, b.id))) n += 1;
    }
    if (n > 0) this.version += 1;
    return n;
  }

  clear(): void {
    if (this.#map.size === 0) return;
    this.#map.clear();
    this.version += 1;
  }

  /** `IBuff::Expire`: the buffs whose time ran out are removed; returns them. */
  process(now: number, nowUnix: number, rules: BuffRules): Buff[] {
    const gone: Buff[] = [];
    for (const b of this.list()) {
      if (b.type === BUFF_PET || b.type === BUFF_EQUIP) continue;
      if (b.type === BUFF_ITEM && rules.itemIsPermanent(b.id)) continue;
      const over = b.type === BUFF_ITEM2 ? nowUnix >= b.level : now - b.inst >= b.total;
      if (over) {
        this.#map.delete(buffKey(b.type, b.id));
        gone.push(b);
      }
    }
    if (gone.length > 0) this.version += 1;
    return gone;
  }
}
