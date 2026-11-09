// The Hot Air visual (`ProjectileKind` 15 `MAG_HOT_AIR`, Plan/41 section 17.4, WP-REND-24; `CSfxSkillMagHotAir`, Sfx.cpp:2749-2832):
// not a flight but two effects that last for the skill's duration - the cast effect at the attacker (turned `-angle`, played once)
// and `XI_SKILL_MAG_FIRE_HOTAIR01_01` at the target (looping). The visual owns the pain-tick timer: at the effect frames
// `0, P, 2P, ...` (`P = trunc(pain_time / 1000 * 60)`, at least 1) it calls the audio cue `hotAirPainTick(pos, first, attacker,
// target)` (`first` only at frame 0); it ends at frame `trunc(skill_time / 1000 * 60)` (the tick of that frame still happens, as in
// the original, where the damage count is `skill_time / pain_time + 1`), when either mover is gone, or when the server's
// `RemoveSfxObj` mark for the effect reaches the target.
//
// The server ends the projectile at its hit frame (`ProjectileHit`), which is the first and only damage; the visual ignores it and
// runs its own clock.

import { XI_SKILL_MAG_FIRE_HOTAIR01, XI_SKILL_MAG_FIRE_HOTAIR01_01 } from '../../gen/defs/obj';
import type { SfxHandle } from '../fx';
import type { SfxSystem } from './SfxSystem';

export type V3 = [number, number, number];

/** `SEC1`. */
const SEC1 = 60;

/** `(int)((ms / 1000.0f) * SEC1)` in single precision. */
export function msToFrames(ms: number): number {
  return Math.trunc(Math.fround(Math.fround(ms / 1000) * SEC1));
}

export interface HotAirRequest {
  readonly projectileId: number;
  /** The cast effect (`XI_SKILL_MAG_FIRE_HOTAIR01`). */
  readonly sfxId: number;
  readonly attacker: number;
  readonly target: number;
  /** `AddSkillProp.dwPainTime` (ms). */
  readonly painMs: number;
  /** `AddSkillProp.dwSkillTime` (ms). */
  readonly skillMs: number;
}

export interface HotAirWorld {
  posOf(id: number, out: V3): boolean;
  angleOf(id: number): number;
}

export interface HotAirDeps {
  readonly sfx: Pick<SfxSystem, 'createProjectile' | 'placeProjectile' | 'remove' | 'takeMark'>;
  readonly world: () => HotAirWorld;
  /** The audio cue (44 T08h); read at the moment of each tick. */
  readonly painTick: () =>
    | ((
        pos: readonly [number, number, number],
        first: boolean,
        attacker: number,
        target: number,
      ) => void)
    | undefined;
}

interface Live {
  readonly req: HotAirRequest;
  readonly pain: number;
  readonly max: number;
  /** `m_nFrame`. */
  frame: number;
  /** `m_nDmgCnt`. */
  count: number;
  /** `GetPos()`: the attacker's place when the effect was made. */
  readonly origin: V3;
  cast: SfxHandle;
  aura: SfxHandle;
}

export interface HotAirStats {
  started: number;
  ended: number;
  painTicks: number;
}

export class HotAirVisuals {
  readonly stats: HotAirStats = { started: 0, ended: 0, painTicks: 0 };
  private readonly live = new Map<number, Live>();
  private readonly tmp: V3 = [0, 0, 0];

  constructor(private readonly deps: HotAirDeps) {}

  has(projectileId: number): boolean {
    return this.live.has(projectileId);
  }

  get count(): number {
    return this.live.size;
  }

  /** The frame of a visual (probe, tests); `-1` when unknown. */
  frameOf(projectileId: number): number {
    return this.live.get(projectileId)?.frame ?? -1;
  }

  /** `ProjectileSpawn` of kind 15. */
  start(req: HotAirRequest): void {
    if (this.live.has(req.projectileId)) return;
    const origin: V3 = [0, 0, 0];
    const world = this.deps.world();
    if (!world.posOf(req.attacker, origin)) {
      // the attacker is not known yet (the original deletes the effect at its first step)
      return;
    }
    // `max(trunc(pain), 1)`: a zero pain time would divide by zero in the original
    const pain = Math.max(msToFrames(req.painMs), 1);
    this.live.set(req.projectileId, {
      req,
      pain,
      max: msToFrames(req.skillMs),
      frame: 0,
      count: 0,
      origin: [origin[0], origin[1] + 0.01, origin[2]],
      cast: 0,
      aura: 0,
    });
    this.stats.started++;
  }

  /** `ProjectileHit` of the projectile: ignored, the visual runs its own clock. */
  end(): void {
    // nothing: the effect runs its own clock
  }

  /** `steps` 60 Hz ticks (`CSfxSkillMagHotAir::Process`). */
  tick(steps: number): void {
    for (let s = 0; s < steps; s++) {
      for (const [id, v] of this.live) {
        if (!this.step(v)) this.drop(id, v);
      }
    }
  }

  /** Puts the two models where their movers are (once per frame). */
  update(): void {
    const world = this.deps.world();
    const p = this.tmp;
    for (const v of this.live.values()) {
      if (world.posOf(v.req.attacker, p)) {
        const yaw = -world.angleOf(v.req.attacker);
        if (v.cast === 0) {
          v.cast = this.deps.sfx.createProjectile(v.req.sfxId || XI_SKILL_MAG_FIRE_HOTAIR01, {
            pos: p,
            rot3: null,
            yaw: Number.isNaN(yaw) ? 0 : yaw,
            once: true,
            cls: 'skill',
          });
        } else {
          this.deps.sfx.placeProjectile(v.cast, p, null, Number.isNaN(yaw) ? 0 : yaw);
        }
      }
      if (world.posOf(v.req.target, p)) {
        if (v.aura === 0) {
          v.aura = this.deps.sfx.createProjectile(XI_SKILL_MAG_FIRE_HOTAIR01_01, {
            pos: p,
            rot3: null,
            yaw: 0,
            cls: 'skill',
          });
        } else {
          this.deps.sfx.placeProjectile(v.aura, p, null, 0);
        }
      }
    }
  }

  clear(): void {
    for (const [id, v] of [...this.live]) this.drop(id, v);
  }

  /** One `Process`: `false` = the effect is deleted. */
  private step(v: Live): boolean {
    const world = this.deps.world();
    if (!world.posOf(v.req.attacker, this.tmp)) return false;
    if (!world.posOf(v.req.target, this.tmp)) return false;
    // `GetRemoveSfxObj(XI_SKILL_MAG_FIRE_HOTAIR01)` of the target, honoured from the second step on
    if (v.frame > 0 && this.deps.sfx.takeMark(v.req.target, XI_SKILL_MAG_FIRE_HOTAIR01))
      return false;
    if (v.frame % v.pain === 0) {
      const first = v.count === 0;
      v.count++;
      this.stats.painTicks++;
      this.deps.painTick()?.(
        [v.origin[0], v.origin[1], v.origin[2]],
        first,
        v.req.attacker,
        v.req.target,
      );
    }
    const end = v.frame >= v.max;
    v.frame++;
    return !end;
  }

  private drop(id: number, v: Live): void {
    this.live.delete(id);
    if (v.cast !== 0) this.deps.sfx.remove(v.cast);
    if (v.aura !== 0) this.deps.sfx.remove(v.aura);
    this.stats.ended++;
  }
}
