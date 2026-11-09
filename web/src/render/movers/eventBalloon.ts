// The event balloon (Plan/22 section 17.2, WP-REND-27; `CMover::BalloonMoveProcess`, `Mover.cpp:4141-4235`, the creation in `Mover.cpp:3716-3800`, the draw in `MoverRender.cpp:851-858`): a balloon
// item (`IK3_BALLOON` buff: `II_SYS_SYS_EVE_BALLOON...`) floats beside the player and follows him lazily.
//
//   model    the `OT_ITEM` model of the buff's item, scale 1.5; created at `pos + (1, 0, 0)` and drawn at `y + yPos` with `yPos = 1.0` and the up flag set
//   follow   per 60 Hz tick, with `d = playerPos - balloonPos` (3-D) and `speed = GetSpeed( m_fSpeed )`:
//              |d|^2 > 100^2   the balloon is DELETED (a teleport); the buff is still there, so the next tick makes it again at the player
//              |d|^2 > 1      `balloonPos += normalize(d) * speed * (|d|^2 / 10)` (an unclamped lerp factor, KEEP: far away it overshoots), then the bob +-0.006 between 0.5 and 1.5
//              else            only the bob, +-0.002 between 0.7 and 1.3
//            one up / down flag for both bands. The matrix of a tick uses the `yPos` of the tick BEFORE the bob step
//   draw     `RotationY( timeGetTime() / 3000 ) * Scaling( 1.5 ) * Translation( balloonPos + (0, yPos, 0) )`: it turns about its own axis with the wall clock (KEEP); only while its
//            player is drawn (a transparent player hides it); it goes with the buff or the entity
//
// LEFT-HANDED maths, mapped to three.js once (`mirrorMatrix`).

import { Matrix4, type Object3D } from 'three';
import { Prio } from '../../workers/priority';
import { multiply, rotationY, scaling, translation } from '../d3dx';
import { mirrorMatrix } from '../space';
import type { ModelGeometry } from '../objects/ModelCache';
import type { ModelInstanceOptions } from '../objects/ModelInstance';

export type V3 = [number, number, number];

/** `m_pBalloon` scale, the creation offset and the start of `m_fBalloonYPos`. */
export const BALLOON_SCALE = 1.5;
export const BALLOON_OFFSET_X = 1;
export const BALLOON_Y_START = 1;
/** The teleport distance (`fDistSq > 100 * 100`) and the follow threshold (`fDistSq > 1`). */
export const BALLOON_TELEPORT_SQ = 100 * 100;
export const BALLOON_FOLLOW_SQ = 1;
/** The follow lerp: `speed * distSq / 10`. */
export const BALLOON_LERP_DIVISOR = 10;
/** The bob while following and while idling: step and band. */
export const BOB_FAR = { step: 0.006, low: 0.5, high: 1.5 } as const;
export const BOB_NEAR = { step: 0.002, low: 0.7, high: 1.3 } as const;
/** `GetSpeed( m_fSpeed )` when the host cannot tell: the base walking step of a player at the usual rate. */
export const BALLOON_DEFAULT_SPEED = 0.12;

/** What a balloon needs of the renderer (`WorldRenderer` implements it, the tests fake it). */
export interface EventBalloonDeps {
  /** The logical position (LH) of a player into `out`; `false` when the render view does not know him. */
  posOf(id: number, out: V3): boolean;
  /** The player was drawn in the last frame (a transparent or culled player hides his balloon). */
  isDrawn(id: number): boolean;
  /** `GetSpeed( m_fSpeed )` of the player (`SkillFxHooks.speedOf`). */
  speedOf(id: number): number | undefined;
  /** The model file of an item (`OT_ITEM` row of the models registry), `undefined` when unknown. */
  modelOf(itemId: number): { readonly file: string | null } | undefined;
  readonly models: { model(path: string, prio?: Prio): Promise<ModelGeometry | null> };
  readonly host: {
    createInstance(
      model: ModelGeometry,
      options: ModelInstanceOptions,
      parent?: Object3D,
    ): { readonly root: Object3D; setTransform(m: Matrix4): void };
    removeInstance(inst: { readonly root: Object3D; setTransform(m: Matrix4): void }): void;
  };
  readonly log?: (message: string) => void;
}

interface Balloon {
  readonly player: number;
  itemId: number;
  /** `m_BalloonPos`; `null` while the balloon is deleted (it is made again at the next tick). */
  pos: V3 | null;
  /** `m_fBalloonYPos` and `m_bBalloon`. */
  y: number;
  up: boolean;
  inst: { readonly root: Object3D; setTransform(m: Matrix4): void } | null;
  state: 'idle' | 'loading' | 'ready' | 'failed';
}

/** The `render.balloons()` probe row. */
export interface BalloonProbeRow {
  readonly player: number;
  readonly item: number;
  readonly pos: V3 | null;
  readonly y: number;
  readonly up: boolean;
  readonly state: string;
  readonly drawn: boolean;
}

export class EventBalloons {
  private readonly balloons = new Map<number, Balloon>();
  private readonly tmp: V3 = [0, 0, 0];
  private readonly matrix = new Matrix4();
  private disposed = false;
  /** Creations since the start (a teleport makes a second one). */
  created = 0;

  constructor(private readonly d: EventBalloonDeps) {}

  /** `fx.eventBalloon`: the balloon item of a player, `null` removes it. */
  set(player: number, itemId: number | null): void {
    if (this.disposed) return;
    const old = this.balloons.get(player);
    if (itemId === null) {
      if (old) this.drop(old);
      return;
    }
    if (old?.itemId === itemId) return;
    if (old) this.drop(old);
    this.balloons.set(player, {
      player,
      itemId,
      pos: null,
      y: BALLOON_Y_START,
      up: true,
      inst: null,
      state: 'idle',
    });
  }

  has(player: number): boolean {
    return this.balloons.has(player);
  }

  get count(): number {
    return this.balloons.size;
  }

  private drop(b: Balloon): void {
    this.balloons.delete(b.player);
    this.destroyInstance(b);
  }

  private destroyInstance(b: Balloon): void {
    if (b.inst) this.d.host.removeInstance(b.inst);
    b.inst = null;
  }

  // ---- the model ---------------------------------------------------------------------------------------------------------

  private load(b: Balloon): void {
    b.state = 'loading';
    const file = this.d.modelOf(b.itemId)?.file ?? null;
    if (file === null) {
      b.state = 'failed';
      this.d.log?.(`event balloon: item ${String(b.itemId)} has no model`);
      return;
    }
    const item = b.itemId;
    void this.d.models.model(file, Prio.Visible).then(
      (geometry) => {
        // the balloon may have been dropped, replaced or deleted (teleport) meanwhile
        if (this.disposed || this.balloons.get(b.player) !== b || b.itemId !== item) return;
        if (!geometry) {
          b.state = 'failed';
          this.d.log?.(`event balloon: ${file} does not exist`);
          return;
        }
        b.inst = this.d.host.createInstance(geometry, {});
        b.inst.root.visible = false;
        b.state = 'ready';
      },
      () => {
        b.state = 'failed';
      },
    );
  }

  // ---- the 60 Hz step ----------------------------------------------------------------------------------------------------

  tick(steps: number): void {
    if (this.disposed) return;
    for (let s = 0; s < steps; s++) for (const b of this.balloons.values()) this.step(b);
  }

  private step(b: Balloon): void {
    const me = this.tmp;
    if (!this.d.posOf(b.player, me)) return;
    if (b.pos === null) {
      // the creation: beside the player, a metre toward +x; the model starts to load
      b.pos = [me[0] + BALLOON_OFFSET_X, me[1], me[2]];
      b.y = BALLOON_Y_START;
      b.up = true;
      this.created++;
      this.load(b);
    }
    const pos = b.pos;
    const dx = me[0] - pos[0];
    const dy = me[1] - pos[1];
    const dz = me[2] - pos[2];
    const distSq = dx * dx + dy * dy + dz * dz;
    if (distSq > BALLOON_TELEPORT_SQ) {
      // `SAFE_DELETE( m_pBalloon )`: made again at the next tick
      b.pos = null;
      b.state = 'idle';
      this.destroyInstance(b);
      return;
    }
    let band: { step: number; low: number; high: number } = BOB_NEAR;
    if (distSq > BALLOON_FOLLOW_SQ) {
      band = BOB_FAR;
      const len = Math.sqrt(distSq);
      const speed =
        (this.d.speedOf(b.player) ?? BALLOON_DEFAULT_SPEED) * (distSq / BALLOON_LERP_DIVISOR);
      pos[0] += (dx / len) * speed;
      pos[1] += (dy / len) * speed;
      pos[2] += (dz / len) * speed;
    }
    // the matrix of this tick used `yPos` as it was; the bob steps after it (see `place`, which reads `b.y` after the step: one step apart, invisible)
    if (b.up) {
      b.y += band.step;
      if (b.y > band.high) b.up = false;
    } else {
      b.y -= band.step;
      if (b.y < band.low) b.up = true;
    }
  }

  // ---- the frame ---------------------------------------------------------------------------------------------------------

  /** Places the models of the frame: `RotationY( nowMs / 3000 ) * Scaling( 1.5 ) * Translation( pos + (0, yPos, 0) )`, only for a player who was drawn. */
  update(nowMs: number): void {
    for (const b of this.balloons.values()) {
      const inst = b.inst;
      if (!inst || b.pos === null) continue;
      const show = this.d.isDrawn(b.player);
      inst.root.visible = show;
      if (!show) continue;
      const m = multiply(
        multiply(rotationY(nowMs / 3000), scaling(BALLOON_SCALE, BALLOON_SCALE, BALLOON_SCALE)),
        translation(b.pos[0], b.pos[1] + b.y, b.pos[2]),
      );
      inst.setTransform(this.matrix.fromArray(mirrorMatrix(m)));
    }
  }

  probe(): BalloonProbeRow[] {
    return [...this.balloons.values()].map((b) => ({
      player: b.player,
      item: b.itemId,
      pos: b.pos ? [b.pos[0], b.pos[1], b.pos[2]] : null,
      y: b.y,
      up: b.up,
      state: b.state,
      drawn: b.inst?.root.visible === true,
    }));
  }

  /** The balloon record of a player (tests). */
  stateOf(player: number): Readonly<Balloon> | undefined {
    return this.balloons.get(player);
  }

  clear(): void {
    for (const b of [...this.balloons.values()]) this.drop(b);
  }

  dispose(): void {
    this.clear();
    this.disposed = true;
  }
}
