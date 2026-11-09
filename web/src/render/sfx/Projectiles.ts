// Projectile visuals (Plan/41 section 17.4, WP-REND-21; Sfx.cpp:560-1110, 1360-1510, 2464-2700, 3150-3720; ref/20 section 7.4).
//
// The lifecycle belongs to doc 17 (`game/combat/projectiles.ts`): `fx.projectileVisual(id, sfxId, kind, target)` when the flight starts,
// `fx.projectileEnd(id, at, hitSfx)` when the server ends it. The shared sim flies the projectile; the renderer only DRAWS it:
//
//   placement    every frame from the render-view projectile record (`pos` interpolated, `dir` a unit vector, LH like the movers); the
//                effect instance is created by the first record and keeps its last place when the record vanishes
//   orientation  directional classes (the shots, arrows, wand bolts, the wave) turn the model's forward axis `(0, 0, 1)` to `dir` with
//                the quaternion path and are drawn through the 3-axis `Render2`; the wind / boomerang / missile skills use the yaw of
//                that rotation only. Everything happens in LH space, the finished matrix is mapped once by the SFX draw list
//   animation    the effect loops until the projectile ends (`finished -> frame 0`), or inside the window of its class
//   ribbons      `RANGE_ATK1`: one `etc_Tail2.bmp` ribbon (fade speed 0.35, half width 0.05), `JunkBow`: three at y - 0.5, 0, + 0.5. FIX of
//                the original: the strip is perpendicular to the flight direction AND to the view, so that it faces the camera (the
//                original used the effect object's never-set angle and stayed aligned with the world X axis, ref/20 section 9.1)
//   wand dots    `WAND_ATK_AIR` makes one point sprite per tick: type 0 (red) when it flies at the active mover, else type 1 (blue)
//   afterimages  `SHOOT_WAVE`: the model is also drawn at the 8 positions sampled every 4 ticks
//   end          the ribbons vanish at once; a hit effect starts at `at` with the yaw of the last flight direction
//
// Out of scope: the yo-yo flights (`yoyo.ts`) and the Hot Air effect with its pain-tick timer (`hotAir.ts`): REND-24 (the game layer hands `MAG_HOT_AIR` to
// `RenderFx.hotAir`, the yo-yo skills to `RenderFx.skillYoyo`).

import { XI_NPC_DIR_STEAM } from '../../gen/defs/obj';
import { ProjectileKind } from '../../gen/proto/flyff/v1/combat_pb';
import { RENDER_VIEW } from '../../gen/sim/wire';
import type { SfxHandle } from '../fx';
import { directionRot3 } from './orient';
import type { ParticleMng } from './ParticleMng';
import type { FrameLoop, ProjectileSfx, SfxSystem } from './SfxSystem';
import type { TailBelt, TailMng } from './Tails';

export type V3 = [number, number, number];

/** The texture of the arrow ribbons (`Model/Texture/etc_Tail2.bmp`). */
export const ARROW_RIBBON_TEXTURE = 'model/texture/etc_tail2.bmp';
export const ARROW_RIBBON_TYPE = 2;
export const ARROW_RIBBON_FADE = 0.35;
export const ARROW_RIBBON_HALF_WIDTH = 0.05;
/** The vertical offsets of the three ribbons of the junk bow. */
export const JUNK_BOW_OFFSETS = [-0.5, 0, 0.5] as const;
/** `MAX_SHOOTWAVE_TAIL` and the sampling period. */
export const WAVE_TAIL = 8;
export const WAVE_PERIOD = 4;

interface KindSpec {
  /** Drawn through the 3-axis path. */
  readonly directional: boolean;
  /** Offsets (y) of the ribbons, empty for none. */
  readonly ribbons: readonly number[];
  readonly loop?: FrameLoop;
  readonly dots?: boolean;
  readonly wave?: boolean;
}

const NONE: readonly number[] = [];
/** `ProjectileKind.YOYO_ATK` as the number the wire carries. */
const YOYO: number = ProjectileKind.YOYO_ATK;

const KINDS = new Map<number, KindSpec>([
  [ProjectileKind.SHOOT, { directional: true, ribbons: NONE }],
  [ProjectileKind.WAND_ATK1, { directional: true, ribbons: NONE, loop: { at: 20, to: 0 } }],
  [
    ProjectileKind.WAND_ATK_AIR,
    { directional: true, ribbons: NONE, loop: { at: 20, to: 0 }, dots: true },
  ],
  [ProjectileKind.RANGE_ATK1, { directional: true, ribbons: [0] }],
  [ProjectileKind.RANGE_ATK_JUNKBOW, { directional: true, ribbons: JUNK_BOW_OFFSETS }],
  [ProjectileKind.MAG_STRONG_WIND, { directional: false, ribbons: NONE, loop: { at: 60, to: 30 } }],
  [
    ProjectileKind.MAG_SWORD_WIND,
    { directional: false, ribbons: NONE, loop: { at: 30, to: 0, ge: true } },
  ],
  [
    ProjectileKind.MAG_FIRE_BOOMERANG,
    { directional: false, ribbons: NONE, loop: { at: 20, to: 0 } },
  ],
  [ProjectileKind.MAG_ICE_MISSILE, { directional: false, ribbons: NONE }],
  [ProjectileKind.MAG_LIGHTNING_BALL, { directional: false, ribbons: NONE }],
  [ProjectileKind.MAG_SPIKE_STONE, { directional: false, ribbons: NONE }],
  [ProjectileKind.SHOOT_WAVE, { directional: true, ribbons: NONE, wave: true }],
  [ProjectileKind.ATK_STRAIGHT, { directional: true, ribbons: NONE }],
  [ProjectileKind.MAG_HOT_AIR, { directional: false, ribbons: NONE }],
]);

/** The projectile area of the render view (`RenderViewReader.projectiles`, stride 16, 31 section 17.7). */
export interface ProjectileView {
  readonly projectiles: Float32Array;
  readonly projCount: number;
}

/** What the visuals ask the world. */
export interface ProjectileWorld {
  /** The logical position (LH) of a mover; `false` when unknown. */
  posOf(id: number, out: V3): boolean;
  /** The target is the active (own) mover. */
  isActive(id: number): boolean;
}

export interface ProjectileDeps {
  readonly sfx: Pick<
    SfxSystem,
    'createProjectile' | 'placeProjectile' | 'setGhosts' | 'remove' | 'create'
  >;
  /** The sound hook for effects this class makes itself (T41; `SkillFxHooks.sfxCreated`). */
  readonly created?: (xi: number, pos: readonly [number, number, number] | undefined) => void;
  readonly tails: Pick<TailMng, 'add' | 'remove'>;
  readonly particles: Pick<ParticleMng, 'create'>;
  readonly world: () => ProjectileWorld;
  /** One 60 Hz step of the ribbons, the particles and the sparks (the renderer's cosmetic world). */
  readonly step: () => void;
}

export interface ProjectileStats {
  /** Live visuals. */
  visuals: number;
  /** Visuals with a record (placed). */
  placed: number;
  created: number;
  ended: number;
  /** Hit effects started by `end`. */
  hits: number;
  ribbonNodes: number;
  dots: number;
}

interface Visual {
  readonly id: number;
  readonly kind: number;
  readonly xi: number;
  readonly target: number;
  readonly spec: KindSpec | undefined;
  handle: SfxHandle;
  readonly pos: V3;
  readonly dir: V3;
  hasRecord: boolean;
  /** The position at the end of the last cosmetic tick batch (the tick positions interpolate from it). */
  readonly from: V3;
  hasFrom: boolean;
  frame: number;
  readonly belts: (TailBelt | null)[];
  /** The afterimage ring of the wave, newest first (`count` valid). */
  readonly wave: V3[];
  waveCount: number;
}

const FORWARD: V3 = [0, 0, 1];

/** The yaw (degrees) of the model after the quaternion path turned `(0, 0, 1)` to `dir`. */
export function flightYaw(dir: readonly number[]): number {
  return directionRot3([0, 0, 0], dir)[1];
}

/**
 * The two ribbon points of an arrow at `pos` flying along `dir` seen from `cam` (LH, all three): half a ribbon width to either side
 * along `dir x (cam - pos)`, which is perpendicular to the flight AND the line of sight, so that the strip faces the camera. Falls
 * back to the world up axis when the arrow flies straight at (or away from) the camera.
 */
export function ribbonPoints(
  pos: readonly number[],
  dir: readonly number[],
  cam: readonly number[],
  halfWidth = ARROW_RIBBON_HALF_WIDTH,
): [V3, V3] {
  const dx = dir[0] ?? 0;
  const dy = dir[1] ?? 0;
  const dz = dir[2] ?? 0;
  let cx = (cam[0] ?? 0) - (pos[0] ?? 0);
  let cy = (cam[1] ?? 0) - (pos[1] ?? 0);
  let cz = (cam[2] ?? 0) - (pos[2] ?? 0);
  let sx = dy * cz - dz * cy;
  let sy = dz * cx - dx * cz;
  let sz = dx * cy - dy * cx;
  let len = Math.hypot(sx, sy, sz);
  if (len < 1e-6) {
    // flying along the line of sight: any perpendicular of the flight will do
    cx = 0;
    cy = 1;
    cz = 0;
    sx = dy * cz - dz * cy;
    sy = dz * cx - dx * cz;
    sz = dx * cy - dy * cx;
    len = Math.hypot(sx, sy, sz);
    if (len < 1e-6) {
      sx = 1;
      sy = 0;
      sz = 0;
      len = 1;
    }
  }
  const k = halfWidth / len;
  sx *= k;
  sy *= k;
  sz *= k;
  const px = pos[0] ?? 0;
  const py = pos[1] ?? 0;
  const pz = pos[2] ?? 0;
  return [
    [px - sx, py - sy, pz - sz],
    [px + sx, py + sy, pz + sz],
  ];
}

export class ProjectileVisuals {
  private readonly visuals = new Map<number, Visual>();
  /** Projectile ids that are anchored jets (`XI_NPC_DIR_STEAM`), not flights. */
  private readonly steam = new Set<number>();
  readonly stats: ProjectileStats = {
    visuals: 0,
    placed: 0,
    created: 0,
    ended: 0,
    hits: 0,
    ribbonNodes: 0,
    dots: 0,
  };
  private cam: V3 = [0, 0, 0];
  private readonly index = new Map<number, number>();
  private readonly tmp: V3 = [0, 0, 0];

  constructor(private readonly deps: ProjectileDeps) {}

  /** `fx.projectileVisual`: a projectile started to fly. */
  visual(id: number, xi: number, kind: number, target = 0, attacker = 0): void {
    if (this.visuals.has(id)) return;
    if (kind === YOYO) return; // the yo-yo flies as `YoyoFlights` (REND-24), launched by the skill runtime
    // ---- render-audio-m7 (WP-REND-27): the steamwalker's jet is not a flight but `CSfxNpcDirSteam`, anchored to its attacker and the victim
    if (xi === XI_NPC_DIR_STEAM) {
      if (attacker !== 0 && target !== 0) {
        this.deps.created?.(xi, undefined);
        this.deps.sfx.create(xi, { src: attacker, dest: target, cls: 'normal' });
        this.stats.created++;
      }
      this.steam.add(id);
      return;
    }
    this.visuals.set(id, {
      id,
      kind,
      xi,
      target,
      spec: KINDS.get(kind),
      handle: 0,
      pos: [0, 0, 0],
      dir: [...FORWARD],
      hasRecord: false,
      from: [0, 0, 0],
      hasFrom: false,
      frame: 0,
      belts: [],
      wave: [],
      waveCount: 0,
    });
    this.stats.created++;
    this.stats.visuals = this.visuals.size;
  }

  /** `fx.projectileEnd`: the visual is removed; `at` with a `hitSfx` starts the hit effect there. */
  end(id: number, at: readonly [number, number, number] | null, hitSfx: number): void {
    // the jet plays out by itself (it ends with its attacker or victim) and has no hit effect
    if (this.steam.delete(id)) {
      this.stats.ended++;
      return;
    }
    const v = this.visuals.get(id);
    const yaw = v ? flightYaw(v.dir) : 0;
    if (v) this.drop(v);
    this.stats.ended++;
    if (at && hitSfx > 0) {
      this.deps.sfx.create(hitSfx, { pos: at, angle: yaw, cls: 'hit' });
      this.stats.hits++;
    }
  }

  /** Whether a visual exists (tests, probes). */
  has(id: number): boolean {
    return this.visuals.has(id);
  }

  /** The effect handle of a visual, `0` until its first record arrived. */
  handleOf(id: number): SfxHandle {
    return this.visuals.get(id)?.handle ?? 0;
  }

  clear(): void {
    for (const v of [...this.visuals.values()]) this.drop(v);
    this.index.clear();
  }

  private drop(v: Visual): void {
    this.visuals.delete(v.id);
    if (v.handle !== 0) this.deps.sfx.remove(v.handle);
    for (const b of v.belts) if (b) this.deps.tails.remove(b);
    v.belts.length = 0;
    this.stats.visuals = this.visuals.size;
  }

  // ---- the frame -------------------------------------------------------------------------------------------------

  /** Places the visuals from the render view of the frame (`cam`: the camera position, LH). */
  update(view: ProjectileView | null, cam: readonly [number, number, number]): void {
    this.cam[0] = cam[0];
    this.cam[1] = cam[1];
    this.cam[2] = cam[2];
    const stride = RENDER_VIEW.PROJ_STRIDE;
    const f = view?.projectiles;
    const n = view ? Math.min(view.projCount, f ? Math.floor(f.length / stride) : 0) : 0;
    const index = this.index;
    index.clear();
    if (f && n > 0) {
      const u = new Uint32Array(f.buffer, f.byteOffset, f.length);
      for (let i = 0; i < n; i++) index.set(u[i * stride] ?? 0, i);
    }
    let placed = 0;
    for (const v of this.visuals.values()) {
      const i = index.get(v.id);
      if (f && i !== undefined) {
        const o = i * stride;
        v.pos[0] = f[o + 3] ?? 0;
        v.pos[1] = f[o + 4] ?? 0;
        v.pos[2] = f[o + 5] ?? 0;
        const dx = f[o + 6] ?? 0;
        const dy = f[o + 7] ?? 0;
        const dz = f[o + 8] ?? 0;
        if (dx * dx + dy * dy + dz * dz > 1e-12) {
          v.dir[0] = dx;
          v.dir[1] = dy;
          v.dir[2] = dz;
        }
        v.hasRecord = true;
      }
      if (!v.hasRecord) continue;
      placed++;
      const directional = v.spec?.directional ?? true;
      const rot3 = directional ? directionRot3([0, 0, 0], v.dir) : null;
      const yaw = rot3 ? rot3[1] : flightYaw(v.dir);
      if (v.handle === 0) {
        const req: ProjectileSfx = {
          pos: v.pos,
          rot3,
          yaw,
          ...(v.spec?.loop ? { loop: v.spec.loop } : {}),
        };
        v.handle = this.deps.sfx.createProjectile(v.xi, req);
      } else {
        this.deps.sfx.placeProjectile(v.handle, v.pos, rot3, yaw);
      }
    }
    this.stats.placed = placed;
  }

  // ---- the 60 Hz step --------------------------------------------------------------------------------------------

  /** `steps` cosmetic ticks: ribbons age and grow, the wand dots and the wave samples are made, positions interpolate within the batch. */
  tick(steps: number): void {
    for (let k = 1; k <= steps; k++) {
      this.deps.step();
      const t = k / steps;
      for (const v of this.visuals.values()) this.tickOne(v, t);
    }
    for (const v of this.visuals.values()) {
      if (!v.hasRecord) continue;
      v.from[0] = v.pos[0];
      v.from[1] = v.pos[1];
      v.from[2] = v.pos[2];
      v.hasFrom = true;
    }
  }

  private tickOne(v: Visual, t: number): void {
    if (!v.hasRecord) return;
    const spec = v.spec;
    if (!v.hasFrom) {
      v.from[0] = v.pos[0];
      v.from[1] = v.pos[1];
      v.from[2] = v.pos[2];
      v.hasFrom = true;
    }
    const p = this.tmp;
    p[0] = v.from[0] + (v.pos[0] - v.from[0]) * t;
    p[1] = v.from[1] + (v.pos[1] - v.from[1]) * t;
    p[2] = v.from[2] + (v.pos[2] - v.from[2]) * t;
    if (spec && spec.ribbons.length > 0) this.ribbon(v, spec.ribbons, p);
    if (spec?.dots === true) this.dot(v, p);
    if (spec?.wave === true) this.sampleWave(v, p);
    v.frame++;
  }

  private ribbon(v: Visual, offsets: readonly number[], p: V3): void {
    const [a, b] = ribbonPoints(p, v.dir, this.cam);
    for (let i = 0; i < offsets.length; i++) {
      let belt = v.belts[i] ?? null;
      if (!belt && v.belts.length <= i) {
        belt = this.deps.tails.add(ARROW_RIBBON_TEXTURE, ARROW_RIBBON_TYPE, ARROW_RIBBON_FADE);
        v.belts[i] = belt;
      }
      if (!belt) continue;
      const dy = offsets[i] ?? 0;
      if (belt.push([a[0], a[1] + dy, a[2]], [b[0], b[1] + dy, b[2]])) this.stats.ribbonNodes++;
    }
  }

  private dot(v: Visual, p: V3): void {
    // `IsValidObj(pObjDest)`: a bolt without a (known) target makes no dots
    if (v.target === 0) return;
    const world = this.deps.world();
    const out: V3 = [0, 0, 0];
    if (!world.posOf(v.target, out)) return;
    const type = world.isActive(v.target) ? 0 : 1;
    this.deps.particles.create(type, [p[0], p[1], p[2]], [0, 0, 0], out[1]);
    this.stats.dots++;
  }

  private sampleWave(v: Visual, p: V3): void {
    if ((v.frame & (WAVE_PERIOD - 1)) !== 0) return;
    // shift back by one, newest first
    if (v.waveCount < WAVE_TAIL) v.waveCount++;
    for (let i = v.waveCount - 1; i > 0; i--) {
      const prev = v.wave[i - 1];
      if (prev) v.wave[i] = prev;
    }
    v.wave[0] = [p[0], p[1], p[2]];
    if (v.handle !== 0) this.deps.sfx.setGhosts(v.handle, v.wave.slice(0, v.waveCount));
  }
}
