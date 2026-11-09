// Ground items (Plan/41 section 11.9, R15, WP-REND-22; `CItem::Process` / `Render`, Item.cpp:515-572, `CDPClient::OnAddObj`,
// DPClient.cpp:1440-1490): the model of a dropped item, its drop bounce, its spin, its shine and its reflect - cosmetic only.
//
//   model      `models.types["4"]` row of the item id -> `.o3d`; a missing entry or file shows `II_ARM_M_VAG_HELMET02` (`GetModelElem`)
//   bounce     an item that DROPS (announced by `SpawnItem` outside the join batch; items of the join batch do not animate):
//                g = groundHeight(pos + 1.5 y) + modelMaxHeight / 2 + 0.12;  when |g - pos.y| < 1 the item starts at pos + (0, 1.5, 0)
//                with vy = +0.15; per tick above g: vy -= 0.0075, y += vy; landing snaps to g and ends the motion (x / z never move)
//              items spawned in the air (|g - pos.y| >= 1) do not animate and stay where the protocol put them
//   ground     `groundHeight` is the sim's `item_height`: ONE `SimBridge.rayCast(p, (0, -1, 0), 3, terrain | static)` per drop; a miss,
//              or a cast the bridge could not run, uses the land `heightAt`. The bounce is cosmetic
//              kinematics under the R15 exception: nothing here is ever written to the sim, the protocol or a store, and the item's
//              logical position (picking, pickup) stays the protocol position
//   spin       `angle += 0.5` degrees per tick, quantised to 1/65536 turn (`SetAngle`)
//   shine      per tick, for an item that was drawn in the last frame: 1 in 50 (`xRandom(50) == 1`) starts `XI_GEN_ITEM_SHINE01` at the item
//              (FIX of the render-rate dependency: the original rolled in the render function)
//   reflect    `ItemProp.nReflect > 0`: `XE_REFLECT` on the whole model
//   gold       a gold seed carries its amount in `hit_point` and `count = 1`: the model is the one of its item id, no count is displayed
//
// This module is importable from `src/render/**` only (R15).

import { Matrix4, Sphere, Vector3, type Frustum, type Object3D } from 'three';
import { II_ARM_M_VAG_HELMET02 } from '../../gen/defs/item';
import { XI_GEN_ITEM_SHINE01 } from '../../gen/defs/obj';
import type { SfxOptions } from '../fx';
import { XE } from '../gl/materials/key';
import type { PickSource } from '../pick/PickSupport';
import type { EntityViewSource, ObjId, PickProxy } from '../types';
import { Prio } from '../../workers/priority';
import { cullDistance } from './distanceTable';
import type { ModelGeometry } from './ModelCache';
import type { ModelInstanceOptions } from './ModelInstance';
import { worldMatrixThree, worldSphere, type Placement } from './placement';

/** The entity kind of ground items (`EntityKind.Item`). */
const ITEM_KIND = 3;
/** The ray mask `terrain | static` (Plan/31 section 10.11). */
export const GROUND_MASK = 3;
export const BOUNCE_LIFT = 1.5;
export const BOUNCE_VY = 0.15;
export const BOUNCE_GRAVITY = 0.0075;
export const BOUNCE_HALF_HEIGHT_PAD = 0.12;
export const SPIN_PER_TICK = 0.5;
export const SHINE_ODDS = 50;

export type V3 = readonly [number, number, number];

/** What the ground of an item comes from (`SimBridge.rayCast`, `HeightField.heightAt`). */
export interface GroundSource {
  /** LH; a hit, `null` (nothing there) or `undefined` (the bridge could not run it: the export is missing). */
  rayCast(
    origin: V3,
    dir: V3,
    maxT: number,
    mask: number,
  ): { readonly pos: readonly [number, number, number] } | null | undefined;
  heightAt(x: number, z: number): number;
}

/** `CObj::SetAngle`: degrees quantised to 1/65536 turn (truncation, as the original's `(int)` cast). */
export function quantizeAngle(deg: number): number {
  return Math.fround((360 / 65536) * (Math.trunc(deg * (65536 / 360)) & 65535));
}

/** `AddAngle(0.5)`. */
export const spin = (angle: number): number => quantizeAngle(angle + SPIN_PER_TICK);

/** The drop bounce of one item (`CItem::Process` with `m_fGroundY` and `m_vDelta`). */
export class DropBounce {
  y: number;
  vy = BOUNCE_VY;
  /** `true` until the item landed. */
  active = true;

  constructor(
    logicalY: number,
    readonly ground: number,
  ) {
    this.y = Math.fround(logicalY + BOUNCE_LIFT);
  }

  /** One 60 Hz tick: gravity above the ground, the snap on landing, then the vertical move. */
  tick(): void {
    if (!this.active) return;
    if (this.y > this.ground) {
      this.vy = Math.fround(this.vy - BOUNCE_GRAVITY);
    } else {
      this.y = this.ground;
      this.vy = 0;
      this.active = false;
      return;
    }
    this.y = Math.fround(this.y + this.vy);
  }
}

/** What a ground item needs of the renderer (`WorldRenderer` implements it). */
export interface ItemInstanceLike {
  readonly root: Object3D;
  setTransform(m: Matrix4): void;
}

export interface GroundItemsDeps {
  /** `ItemProp.nReflect` of an item id. */
  readonly reflectOf: (itemId: number) => number;
  /** The model row of an item id (the registry's fallback for missing rows included), `undefined` when unknown. */
  readonly modelOf: (
    itemId: number,
  ) => { readonly file: string | null; readonly distant?: number } | undefined;
  /** `.o3d` loading; `null` when the file does not exist (the fallback model is tried). */
  readonly models: { model(path: string, prio?: Prio): Promise<ModelGeometry | null> };
  readonly host: {
    createInstance(
      model: ModelGeometry,
      options: ModelInstanceOptions,
      parent?: Object3D,
    ): ItemInstanceLike;
    removeInstance(inst: ItemInstanceLike): void;
  };
  readonly ground: GroundSource;
  /** Starts an effect (`XI_GEN_ITEM_SHINE01`). */
  readonly sfx: (xi: number, o: SfxOptions) => void;
  /** `[0, 1)`. */
  readonly rnd?: () => number;
  /** `RenderOptions.objectDistant`. */
  readonly objectDistant: () => 0 | 1 | 2;
  readonly log?: (message: string) => void;
}

/** What the renderer reads of an item record of the entity table. */
interface ItemRecord {
  readonly objId: number;
  readonly kind?: number;
  readonly propId?: number;
  readonly removed?: boolean;
  readonly spawn?: {
    readonly obj?: {
      readonly index?: number;
      readonly scaleX100?: number;
      readonly angleX10?: number;
      readonly pos?: { readonly x: number; readonly y: number; readonly z: number };
    };
    readonly item?: { readonly itemId?: number };
  };
}

interface Item {
  readonly id: number;
  readonly itemId: number;
  readonly pos: [number, number, number];
  readonly scale: number;
  angle: number;
  inst: ItemInstanceLike | null;
  model: ModelGeometry | null;
  state: 'loading' | 'ready' | 'failed';
  /** The server announced a drop: the bounce starts as soon as the model is known. */
  dropped: boolean;
  /** The bounce still has to be set up (the ground question is asked in the next frame). */
  needsGround: boolean;
  bounce: DropBounce | null;
  /** The y the item is drawn at (the protocol y until a landed bounce moves it to the ground). */
  y: number;
  distant: number;
  drawn: boolean;
  usedFallback: boolean;
  readonly matrix: Matrix4;
}

export interface GroundItemStats {
  items: number;
  /** Items drawn in the last frame. */
  drawn: number;
  /** Bounces in the air right now. */
  bouncing: number;
  /** Bounces started in total. */
  bounces: number;
  /** `rayCast` calls issued in the last frame, and in total. */
  rayCasts: number;
  rayCastsTotal: number;
  /** `heightAt` lookups of the last frame, and in total. */
  heightLookups: number;
  heightLookupsTotal: number;
  shines: number;
  /** Items that show the fallback model. */
  fallbacks: number;
  /** Items whose model is still being loaded. */
  loading: number;
}

const SPHERE = new Sphere();
/** Scratch of the culling sphere (three.js space). */
const WORLD = { center: new Vector3(), radius: 0 };

export class GroundItems {
  private readonly items = new Map<number, Item>();
  private readonly announced = new Set<number>();
  private version = -1;
  private readonly rnd: () => number;
  private disposed = false;
  readonly stats: GroundItemStats = {
    items: 0,
    drawn: 0,
    bouncing: 0,
    bounces: 0,
    rayCasts: 0,
    rayCastsTotal: 0,
    heightLookups: 0,
    heightLookupsTotal: 0,
    shines: 0,
    fallbacks: 0,
    loading: 0,
  };
  private frameCasts = 0;
  private frameHeights = 0;

  constructor(private readonly deps: GroundItemsDeps) {
    this.rnd = deps.rnd ?? Math.random;
  }

  /** `SpawnItem` outside the join batch: the item bounces when it lands near the ground. */
  drop(id: ObjId): void {
    this.announced.add(id);
    const it = this.items.get(id);
    if (it && !it.bounce && it.inst === null) it.dropped = true;
  }

  /** The position of the name label of an item: above the model (LH), `null` for an unknown item. */
  labelAnchor(id: ObjId): [number, number, number] | null {
    const it = this.items.get(id);
    if (!it) return null;
    const top = it.model ? Math.abs(it.model.bbox.max.y) * it.scale : 0.5;
    return [it.pos[0], it.y + top + 0.3, it.pos[2]];
  }

  has(id: ObjId): boolean {
    return this.items.has(id);
  }

  /** The visible state of an item (tests, probes). */
  stateOf(id: ObjId): {
    y: number;
    angle: number;
    bouncing: boolean;
    state: Item['state'];
    fallback: boolean;
    drawn: boolean;
  } | null {
    const it = this.items.get(id);
    return it
      ? {
          y: it.y,
          angle: it.angle,
          bouncing: it.bounce?.active === true,
          state: it.state,
          fallback: it.usedFallback,
          drawn: it.drawn,
        }
      : null;
  }

  // ---- the frame -------------------------------------------------------------------------------------------------

  /** Syncs the items with the entity table, answers the ground questions of new drops and places the visuals. */
  update(
    entities: EntityViewSource | null,
    cameraPos: readonly [number, number, number],
    frustum: Frustum | null,
  ): void {
    if (this.disposed) return;
    this.frameCasts = 0;
    this.frameHeights = 0;
    this.reconcile(entities);
    const opt = this.deps.objectDistant();
    let drawn = 0;
    let bouncing = 0;
    let loading = 0;
    let fallbacks = 0;
    for (const it of this.items.values()) {
      if (it.state === 'loading') loading++;
      if (it.usedFallback) fallbacks++;
      if (it.needsGround && it.model) this.startBounce(it);
      const inst = it.inst;
      if (!inst) {
        it.drawn = false;
        continue;
      }
      let dist = Math.hypot(
        it.pos[0] - cameraPos[0],
        it.y - cameraPos[1],
        it.pos[2] - cameraPos[2],
      );
      this.place(it);
      if (it.model) {
        // the sphere of the model box under the full world matrix (B7): the centre may be off the pivot in x/z and the item may be
        // rotated; an inverted header box used to give radius 0.3. The distance class measures to the sphere surface.
        worldSphere(it.model.bbox, it.matrix, WORLD);
        WORLD.radius += 0.3;
        // `cameraPos` is LH, the sphere is in three.js space (z mirrored)
        dist = Math.max(
          0,
          Math.hypot(
            WORLD.center.x - cameraPos[0],
            WORLD.center.y - cameraPos[1],
            WORLD.center.z + cameraPos[2],
          ) - WORLD.radius,
        );
      }
      let visible = dist <= cullDistance(opt, it.distant);
      if (visible && frustum && it.model) {
        SPHERE.set(WORLD.center, WORLD.radius);
        visible = frustum.intersectsSphere(SPHERE);
      }
      inst.root.visible = visible;
      it.drawn = visible;
      if (it.bounce?.active === true) bouncing++;
      if (!visible) continue;
      drawn++;
      inst.setTransform(it.matrix);
    }
    this.stats.items = this.items.size;
    this.stats.drawn = drawn;
    this.stats.bouncing = bouncing;
    this.stats.loading = loading;
    this.stats.fallbacks = fallbacks;
    this.stats.rayCasts = this.frameCasts;
    this.stats.heightLookups = this.frameHeights;
  }

  private place(it: Item): void {
    const p: Placement = {
      pos: [it.pos[0], it.y, it.pos[2]],
      angle: it.angle,
      angX: 0,
      angZ: 0,
      scale: [it.scale, it.scale, it.scale],
    };
    worldMatrixThree(p, it.matrix);
  }

  private reconcile(entities: EntityViewSource | null): void {
    if (!entities) return;
    const v = entities.version ?? 0;
    if (v === this.version && this.items.size > 0) return;
    this.version = v;
    const forEach = (
      entities as {
        forEach?: (kind: number, fn: (r: ItemRecord) => void) => void;
      }
    ).forEach;
    if (!forEach) return;
    const live = new Set<number>();
    forEach.call(entities, ITEM_KIND, (r) => {
      if (r.removed === true || (r.kind !== undefined && r.kind !== ITEM_KIND)) return;
      live.add(r.objId);
      if (!this.items.has(r.objId)) this.add(r);
    });
    for (const [id, it] of this.items) {
      if (live.has(id)) continue;
      this.remove(it);
      this.items.delete(id);
    }
    for (const id of this.announced) if (!live.has(id)) this.announced.delete(id);
  }

  private add(r: ItemRecord): void {
    const obj = r.spawn?.obj;
    const p = obj?.pos;
    if (!p) return;
    const itemId = r.spawn?.item?.itemId ?? obj.index ?? r.propId ?? 0;
    const scale100 = obj.scaleX100 ?? 100;
    const it: Item = {
      id: r.objId,
      itemId,
      pos: [p.x, p.y, p.z],
      scale: scale100 > 0 ? scale100 / 100 : 1,
      angle: quantizeAngle((obj.angleX10 ?? 0) / 10),
      inst: null,
      model: null,
      state: 'loading',
      dropped: this.announced.has(r.objId),
      needsGround: false,
      bounce: null,
      y: p.y,
      distant: 0,
      drawn: false,
      usedFallback: false,
      matrix: new Matrix4(),
    };
    this.announced.delete(r.objId);
    this.items.set(r.objId, it);
    this.load(it);
  }

  // ---- the model -------------------------------------------------------------------------------------------------

  private load(it: Item): void {
    const row = this.deps.modelOf(it.itemId);
    it.distant = row?.distant ?? 0;
    const fallback = (): void => {
      if (it.usedFallback) {
        this.fail(it, 'the fallback model is missing too');
        return;
      }
      it.usedFallback = true;
      const fb = this.deps.modelOf(II_ARM_M_VAG_HELMET02);
      if (fb?.file) this.request(it, fb.file);
      else this.fail(it, 'no fallback model');
    };
    if (row?.file) this.request(it, row.file, fallback);
    else fallback();
  }

  private request(it: Item, file: string, onMissing?: () => void): void {
    this.deps.models.model(file, Prio.Visible).then(
      (geometry) => {
        if (this.disposed || !this.items.has(it.id)) return;
        if (!geometry) {
          if (onMissing) onMissing();
          else this.fail(it, `${file} does not exist`);
          return;
        }
        this.ready(it, geometry);
      },
      (e: unknown) => {
        if (onMissing) onMissing();
        else this.fail(it, e instanceof Error ? e.message : String(e));
      },
    );
  }

  private fail(it: Item, why: string): void {
    it.state = 'failed';
    this.deps.log?.(`ground item ${String(it.itemId)}: ${why}`);
  }

  private ready(it: Item, model: ModelGeometry): void {
    it.model = model;
    it.state = 'ready';
    const reflect = this.deps.reflectOf(it.itemId) > 0;
    // `SetEffect(0, XE_REFLECT)` on the whole model
    const options: ModelInstanceOptions = reflect ? { elementEffect: XE.REFLECT } : {};
    it.inst = this.deps.host.createInstance(model, options);
    it.inst.root.visible = false;
    if (it.dropped) it.needsGround = true;
    this.place(it);
  }

  private remove(it: Item): void {
    if (it.inst) this.deps.host.removeInstance(it.inst);
    it.inst = null;
  }

  // ---- the drop -------------------------------------------------------------------------------------------------

  /** `GetItemHeight(pos + 1.5 y)`: one ray cast per drop, else (a miss, a cast the bridge could not run) the land height. */
  private groundAt(x: number, y: number, z: number): number {
    this.frameCasts++;
    this.stats.rayCastsTotal++;
    const hit = this.deps.ground.rayCast([x, y, z], [0, -1, 0], 3, GROUND_MASK);
    if (hit) return hit.pos[1];
    this.frameHeights++;
    this.stats.heightLookupsTotal++;
    return this.deps.ground.heightAt(x, z);
  }

  private startBounce(it: Item): void {
    it.needsGround = false;
    const model = it.model;
    if (!model) return;
    let g = this.groundAt(it.pos[0], it.pos[1] + BOUNCE_LIFT, it.pos[2]);
    // `GetMaxHeight` is the model's top, scaled by the object scale only through the matrix: the original adds it unscaled
    const h = Math.abs(model.bbox.max.y);
    g += h / 2 + BOUNCE_HALF_HEIGHT_PAD;
    // spawned in the air: no animation (`fabs(fGroundY - pos.y) < 1`)
    if (Math.abs(g - it.pos[1]) < 1) {
      it.bounce = new DropBounce(it.pos[1], g);
      it.y = it.bounce.y;
      this.stats.bounces++;
    }
  }

  // ---- the 60 Hz step ---------------------------------------------------------------------------------------------

  tick(steps: number): void {
    if (this.disposed) return;
    for (let k = 0; k < steps; k++) {
      for (const it of this.items.values()) {
        it.angle = spin(it.angle);
        const b = it.bounce;
        if (b) {
          b.tick();
          it.y = b.y;
        }
        if (it.drawn && Math.floor(this.rnd() * SHINE_ODDS) === 1) {
          this.stats.shines++;
          this.deps.sfx(XI_GEN_ITEM_SHINE01, { pos: [it.pos[0], it.y, it.pos[2]] });
        }
      }
    }
  }

  // ---- picking ---------------------------------------------------------------------------------------------------

  /** The pick source of the items drawn in the last frame (`PickSupport.register`). */
  pickSource(): PickSource {
    const matrixOf = (it: Item, out: Matrix4): Matrix4 => {
      // the LOGICAL placement: the protocol position
      return worldMatrixThree(
        {
          pos: [it.pos[0], it.pos[1], it.pos[2]],
          angle: it.angle,
          angX: 0,
          angZ: 0,
          scale: [it.scale, it.scale, it.scale],
        },
        out,
      );
    };
    const aabbOf = (it: Item): readonly [V3, V3] => {
      const b = it.model?.bbox;
      return b
        ? [
            [b.min.x, b.min.y, b.min.z],
            [b.max.x, b.max.y, b.max.z],
          ]
        : [
            [-0.3, 0, -0.3],
            [0.3, 0.6, 0.3],
          ];
    };
    return {
      collect: (out: PickProxy[]) => {
        for (const it of this.items.values()) {
          if (!it.drawn) continue;
          const aabb = aabbOf(it);
          out.push({
            id: it.id,
            kind: 'item',
            worldMatrix: matrixOf(it, new Matrix4()),
            localAabb: [aabb[0], aabb[1]],
            dead: false,
            isPlayer: false,
            rank: 0,
            ai: 0,
            ownPet: false,
            transparent: false,
            gcwarNotClick: false,
          });
        }
      },
      boundsOf: (id: number) => {
        const it = this.items.get(id);
        if (!it?.drawn) return undefined;
        const aabb = aabbOf(it);
        return {
          matrix: matrixOf(it, new Matrix4()),
          aabb: [[...aabb[0]], [...aabb[1]]] as readonly [readonly number[], readonly number[]],
        };
      },
      // items are picked by their box (doc 43 section 5); there is no precise test
      raycast: () => undefined,
    };
  }

  // ---- lifetime --------------------------------------------------------------------------------------------------

  /** A world change: the items of the old world are gone. */
  clear(): void {
    for (const it of this.items.values()) this.remove(it);
    this.items.clear();
    this.announced.clear();
    this.version = -1;
    this.stats.items = 0;
    this.stats.drawn = 0;
    this.stats.bouncing = 0;
  }

  dispose(): void {
    this.clear();
    this.disposed = true;
  }
}
