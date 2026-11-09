// Projectile visuals (Plan/17 section 11.4, Plan/41 section 17.4; WP-COMBAT-10): the lifecycle on the client side of a
// server-simulated projectile. `ProjectileSpawn` and `ProjectileHit` become sim commands in `game/world/movement.ts`
// (`PROJECTILE_SPAWN` / `PROJECTILE_END`: the shared `projectile_step` flies the twin, holds it at a local contact until the
// verdict arrives and snaps it to the server's position when the verdict comes first); this tracker owns what the renderer
// needs on top: `fx.projectileVisual` when the flight starts, `fx.projectileEnd` when the server ends it.
//
//   spawn           visual created with the XI effect id and the kind; the hit effect id is remembered
//   sim contact     (`ProjectileHit` event) the twin touched the target before the verdict: the visual holds, no hit effect yet
//   hit             the server verdict, `expired = false`: `projectileEnd(id, pos, hit_sfx_id)` spawns the hit effect at `pos`
//   expired         timeout or vanished target: `projectileEnd(id, null, 0)`, removed silently, no sound (44 owns the impact sound)
//
// Ids are never reused within a process run (Plan/17 section 4.1), so a late event of an ended projectile is harmless.

import {
  ProjectileKind,
  type ProjectileHit,
  type ProjectileSpawn,
} from '../../gen/proto/flyff/v1/combat_pb';
import type { RenderFx } from '../../render/fx';

/** The sim keeps at most this many projectiles (`MAX_PROJECTILES`); the tracker follows the same bound. */
export const MAX_TRACKED = 512;

/** The timing of a Hot Air skill at its level. */
export interface HotAirTimes {
  readonly painMs: number;
  readonly skillMs: number;
}

export interface TrackedProjectile {
  readonly id: number;
  readonly kind: number;
  /** The flying effect (`XI_*`). */
  readonly sfxId: number;
  /** The effect spawned on impact (`XI_*`, 0 = none). */
  readonly hitSfxId: number;
  readonly attacker: number;
  readonly target: number;
  /** The twin touched the target before the server's verdict (the visual holds at the contact point). */
  contact: boolean;
}

export interface ProjectileStats {
  spawned: number;
  /** Server verdicts with `expired = false`. */
  hits: number;
  /** Server verdicts with `expired = true`. */
  expired: number;
  /** Verdicts of a projectile the tracker did not know (a late join, an evicted entry). */
  orphanEnds: number;
  /** Local contacts reported by the sim before the verdict. */
  localContacts: number;
  evicted: number;
}

export class ProjectileTracker {
  /** Flying projectiles by id, oldest first (a `Map` keeps insertion order). */
  readonly live = new Map<number, TrackedProjectile>();
  readonly stats: ProjectileStats = {
    spawned: 0,
    hits: 0,
    expired: 0,
    orphanEnds: 0,
    localContacts: 0,
    evicted: 0,
  };

  /**
   * `AddSkillProp.dwPainTime` / `dwSkillTime` of the skill a `MAG_HOT_AIR` projectile belongs to (the skill runtime sets it): the Hot
   * Air effect needs them for its pain-tick timer and its lifetime. `undefined` = unknown skill: the kind flies like any other.
   */
  hotAirTimes: ((attacker: number, skillId: number) => HotAirTimes | undefined) | null = null;

  /** `fx` is read at the moment of each call (the renderer arrives late). */
  constructor(private readonly fx: () => RenderFx | undefined) {}

  /** `ProjectileSpawn` (obj_id: the attacker). */
  spawn(attacker: number, m: ProjectileSpawn): void {
    if (this.live.size >= MAX_TRACKED) {
      const oldest = this.live.keys().next();
      if (oldest.done !== true) {
        this.live.delete(oldest.value);
        this.stats.evicted += 1;
      }
    }
    this.live.set(m.projectileId, {
      id: m.projectileId,
      kind: m.kind,
      sfxId: m.sfxId,
      hitSfxId: m.hitSfxId,
      attacker,
      target: m.targetObjId,
      contact: false,
    });
    this.stats.spawned += 1;
    const fx = this.fx();
    if (m.kind === ProjectileKind.MAG_HOT_AIR && fx?.hotAir) {
      // not a flight: two effects for the skill's duration and the pain-tick timer (`CSfxSkillMagHotAir`)
      const t = this.hotAirTimes?.(attacker, m.skillId);
      if (t) {
        fx.hotAir({
          projectileId: m.projectileId,
          sfxId: m.sfxId,
          attacker,
          target: m.targetObjId,
          painMs: t.painMs,
          skillMs: t.skillMs,
        });
        return;
      }
    }
    fx?.projectileVisual(m.projectileId, m.sfxId, m.kind, m.targetObjId, attacker);
  }

  /** `ProjectileHit` (obj_id: the attacker): the server ended the projectile. */
  end(m: ProjectileHit): void {
    const known = this.live.get(m.projectileId);
    this.live.delete(m.projectileId);
    if (!known) this.stats.orphanEnds += 1;
    if (m.expired) {
      this.stats.expired += 1;
      this.fx()?.projectileEnd(m.projectileId, null, 0);
      return;
    }
    this.stats.hits += 1;
    const p = m.pos;
    this.fx()?.projectileEnd(
      m.projectileId,
      p === undefined ? null : [p.x, p.y, p.z],
      known?.hitSfxId ?? 0,
    );
  }

  /** The sim's `ProjectileHit` event: the twin made contact (or applied the verdict). Only the bookkeeping changes. */
  contact(id: number): void {
    const p = this.live.get(id);
    if (!p || p.contact) return;
    p.contact = true;
    this.stats.localContacts += 1;
  }

  /** The world changed: nothing flies any more. */
  clear(): void {
    this.live.clear();
  }
}
