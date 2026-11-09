// The create-monster placement mode (Plan/43 §7.13, `groundControl` step 5; WP-INPUT-12; `WndWorldControlPlayer.cpp:240-290`).
//
// A create-monster item (`IK3_CREATE_MONSTER`) that was used from the bag waits for a ground click (`g_Neuz.m_pCreateMonItem`; the item-use router
// of 45 holds it and exposes it as `hooks.createMonster`). While it waits, the control step of the world looks at the keys and the left button:
//
//   - a movement key or Space without the left button: the mode ends and nothing is sent;
//   - the left button: the cursor ground point `p` is read (the original refreshed it on every control tick),
//       `|self - p| > 15`                                                    -> `TID_GAME_CREATEMON_F_15`
//       `height_attr(p)` is NOWALK or NOMOVE, the own player stands in a safe region, the world is the guild-war map, or the world is not
//       Madrigal                                                              -> `TID_GAME_CREATEMON_F_AREA`
//       otherwise `CreateMonster` is sent (by the hook) and the mode ends.
//     `L.down` is cleared in every left-button case; a refused attempt keeps the mode, so the next click may try again.
//
// The client rules are presentation: the server repeats the range and the area checks (Plan/18 §14, I9). Queries run under the cap of an
// iteration (Plan/40 §7.3): a ground pick or a `height_attr` without a free slot keeps the button and tries again with the next tick.

import { WI_WORLD_GUILDWAR, WI_WORLD_MADRIGAL } from '../gen/defs/world';
import { TID_GAME_CREATEMON_F_15, TID_GAME_CREATEMON_F_AREA } from '../gen/defs/text';
import type { Vec3LH } from '../render/types';
import type { SelfInfo } from './ports';
import type { InputCore } from './worldTick';

/** The range of the placement in metres (`15.f < fDist`, the 3-D distance of the player and the cursor point). */
export const CREATE_MONSTER_RANGE = 15;
/** `HATTR_NOWALK` / `HATTR_NOMOVE` of `height_attr` (`flyff-sim` `types.rs`: NONE 0, NOWALK 1, NOFLY 2, NOMOVE 3). */
export const HATTR_NOWALK = 1;
export const HATTR_NOMOVE = 3;

/** What the step did (the tests and the probe read it). */
export type PlacementOutcome =
  | 'idle' // no mode, or nothing held
  | 'left' // a key ended the mode
  | 'retry' // no query slot: the button stays down, the next tick tries again
  | 'tooFar'
  | 'badArea'
  | 'placed';

/**
 * The placement step of `groundControl` (step 5). One instance per input layer: it remembers the ground point of a click whose attribute
 * query found no slot, so that a cap of one query per iteration still finishes (the ray took the slot every time otherwise).
 */
export class CreateMonsterPlacement {
  /** The ground point of the click being served (LH), `null` between clicks. */
  #point: Vec3LH | null = null;

  constructor(private readonly c: InputCore) {}

  reset(): void {
    this.#point = null;
  }

  /**
   * `keyed` = a movement key or Space is held. Clears `L.down` in every left-button case (so the click destination of step 6 does not run)
   * except `'retry'`, which keeps the button for the next tick.
   */
  step(self: SelfInfo, keyed: boolean): PlacementOutcome {
    const c = this.c;
    const hook = c.deps.hooks?.createMonster;
    const m = c.mouse;
    if (hook?.active() !== true) {
      this.#point = null;
      return 'idle';
    }
    if (!m.L.down) {
      this.#point = null;
      if (!keyed) return 'idle';
      hook.leave(); // a key without the button: the mode ends, nothing is sent
      return 'left';
    }
    // the cursor ground point; a miss is the far plane in the original, which is farther than 15 m
    let p = this.#point;
    if (p === null) {
      const hit = c.pickGround?.();
      if (hit === undefined) return this.retry();
      if (hit === null) return this.refuse(TID_GAME_CREATEMON_F_15, 'tooFar');
      p = [hit.pos[0], hit.pos[1], hit.pos[2]];
    }
    if (
      Math.hypot(self.pos[0] - p[0], self.pos[1] - p[1], self.pos[2] - p[2]) > CREATE_MONSTER_RANGE
    ) {
      return this.refuse(TID_GAME_CREATEMON_F_15, 'tooFar');
    }
    const sim = c.deps.sim;
    if (self.regionSafe === true || sim.worldId === WI_WORLD_GUILDWAR) {
      return this.refuse(TID_GAME_CREATEMON_F_AREA, 'badArea');
    }
    const attr = attrAt(c, p[0], p[2]);
    if (attr === undefined) {
      this.#point = p; // only the attribute is missing
      return this.retry();
    }
    if (attr === HATTR_NOWALK || attr === HATTR_NOMOVE || sim.worldId !== WI_WORLD_MADRIGAL) {
      return this.refuse(TID_GAME_CREATEMON_F_AREA, 'badArea');
    }
    this.#point = null;
    m.L.down = false;
    hook.place(p);
    return 'placed';
  }

  /** No query slot: keep the button, and do not let step 6 turn the click into a destination. */
  private retry(): PlacementOutcome {
    this.c.mouse.freeMove = false;
    return 'retry';
  }

  private refuse(tid: number, why: PlacementOutcome): PlacementOutcome {
    this.#point = null;
    this.c.mouse.L.down = false;
    this.c.notify(tid);
    return why;
  }
}

/**
 * `GetHeightAttribute`: `undefined` = no query slot in this iteration. A bridge without the export (the module is older) answers `undefined`
 * with a slot left: the attribute counts as open and the server decides.
 */
function attrAt(c: InputCore, x: number, z: number): number | undefined {
  const sim = c.deps.sim;
  if (sim.heightAttr === undefined) return 0;
  const budget = sim.queryBudget?.() ?? 1;
  const attr = sim.heightAttr(x, z);
  if (attr === undefined && budget > 0) return 0;
  return attr;
}
