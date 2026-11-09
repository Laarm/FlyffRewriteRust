// Rides, wings and brooms of a flying mover (Plan/41 section 12.4, WP-REND-25; `CMover::Render` MoverRender.cpp:712-858,
// `CMover::WingMotionSetting` / `GetRideFrameSpeed` Mover.cpp:10977-11030; Plan/31 section 13.7 for the decor entries).
//
// A ride is the item model of the `PARTS_RIDE` slot, drawn while the mover flies (`FLY`) relative to the mover's world matrix:
//
//   board (`IK3_BOARD`) and every other ride   at the mover's world matrix
//   broom (`IK3_STICK`)                       lifted by the root bone's Y, `bone[0]._42` = the broom lift of the attachment record
//   wing  (`IK3_WING`)                        at the WING attachment `RotX(180) . RotZ(-90) . bone[4]` with the translation of
//                                             `bone[5]` (the sim writes that matrix into the record); the feather wings
//                                             (`II_RID_RID_WIN_FEATHER01/02`) are nudged by `(0, -0.02, +0.1)` (LH)
//
// The ride also lives in the simulation as an ATTACHED DECOR ENTRY (`DECOR_ADD` kind 1, Plan/31 13.7), for two reasons: the sim runs the
// ride's clip and raises its `AttrSound` events (the T02 sounds, non-positional on purpose: the entry carries `SOUNDS` and not
// `SOUND_POSITIONAL`), and a wing is a skinned model with its own `.chr` whose bone palette only the sim evaluates (rule B1): `STAND`
// clip while floating, `WALK` clip while accelerating (`WingMotionSetting`). A ride with a node animation of its own (`maxFrame > 0` in
// the `.o3d`) gets its node matrices from the entry like an animated static object.
//
// The wing clock runs at speed 1 in the sim; the original sped it up with the thrust (`GetRideFrameSpeed`, 1..4): not reproduced.
//
// Nothing here evaluates a pose or a gameplay formula: matrices and palettes come from the render view.

import { Matrix4, type Object3D } from 'three';
import { OT_ITEM } from '../../gen/defs/define';
import { II_RID_RID_WIN_FEATHER01, II_RID_RID_WIN_FEATHER02 } from '../../gen/defs/item';
import { IK3_BOARD, IK3_STICK, IK3_WING } from '../../gen/defs/itemkind';
import { MTI_STAND, MTI_WALK } from '../../gen/defs/neuz';
import { DECOR as DECOR_BITS } from '../../gen/sim/wire';
import type { MoverVisualState } from '../../game/entities';
import type { Prio } from '../../workers/priority';
import {
  nodeLocals,
  type DecorAdd,
  type DecorSink,
  type DecorView,
  type NodeParent,
} from '../objects/decor';
import type { ModelGeometry } from '../objects/ModelCache';
import type { ModelInstance } from '../objects/ModelInstance';
import type { SkinBinding } from '../gl/materials/MaterialFactory';
import { rows3ToMatrix4 } from '../space';
import { ATTACH_AT, FEATHER_OFFSET_LH } from './attachments';
import type { ModelProvider, VisualHost, VisualRegistryLike } from './MoverVisual';
import { MOVER_FLAGS, NO_PALETTE, type MoverPoseFrame, type PoseSource } from './poseSource';

// ---- kinds ------------------------------------------------------------------------------------------------------------

export type RideKind = 'broom' | 'board' | 'wing' | 'other';

/** The kind of a ride item by its `IK3_*`. */
export function rideKindOf(kind3: number | undefined): RideKind {
  if (kind3 === IK3_STICK) return 'broom';
  if (kind3 === IK3_BOARD) return 'board';
  if (kind3 === IK3_WING) return 'wing';
  return 'other';
}

/** The two feather wings carry the temporary position fix of the original (`MoverRender.cpp:741`). */
export function isFeatherWing(itemId: number): boolean {
  return itemId === II_RID_RID_WIN_FEATHER01 || itemId === II_RID_RID_WIN_FEATHER02;
}

// ---- the ride matrix ----------------------------------------------------------------------------------------------------

/**
 * The broom lift: the y translation of the root bone (`bone[0]._42`) the attachment record carries in its last slot. A
 * record that is absent (a mover the sim has no attachments for yet) lifts nothing.
 */
export function broomLiftOf(attach: ArrayLike<number> | null, base: number): number {
  return attach === null ? 0 : (attach[base + ATTACH_AT.broomLift] ?? 0);
}

/**
 * The matrix of the ride relative to the mover's world matrix (`mRide = local * mWorld`, in three.js column terms
 * `mWorld x local`). A broom is lifted, a wing sits at the WING attachment (the feather wings nudged), anything else is the identity.
 * Returns `false` for a wing without an attachment record (it cannot be placed yet).
 */
export function rideLocalMatrix(
  kind: RideKind,
  itemId: number,
  attach: ArrayLike<number> | null,
  base: number,
  out: Matrix4,
): boolean {
  switch (kind) {
    case 'broom':
      out.makeTranslation(0, broomLiftOf(attach, base), 0);
      return true;
    case 'wing': {
      if (attach === null) return false;
      rows3ToMatrix4(attach, base + ATTACH_AT.wing, out);
      if (isFeatherWing(itemId)) {
        // LH `(0, -0.02, +0.1)` added to the translation; the z axis is mirrored in three.js space
        const e = out.elements;
        e[13] += FEATHER_OFFSET_LH[1];
        e[14] -= FEATHER_OFFSET_LH[2];
      }
      return true;
    }
    case 'board':
    case 'other':
      out.identity();
      return true;
  }
}

// ---- the wing motion (`WingMotionSetting`) --------------------------------------------------------------------------------

/** `m_eWingStateFlag`: the wing clip that plays. `turning` keeps the clip of the state before. */
export type WingState = 'floating' | 'flying' | 'turning';

export interface WingInput {
  /** `OBJSTA_*` state word of the mover. */
  readonly state: number;
  /** `OBJSTAF_*` state flags. */
  readonly stateFlags: number;
  /** The mover did not move since the last frame (`|m_vDelta|^2 <= 0`). */
  readonly stopped: boolean;
  /** `pModel->m_fFrameCurrent == 0` of the mover's own clip. */
  readonly frameZero: boolean;
}

const OBJSTA_STAND = 0x01;
const OBJSTA_TURN_ALL = 0x300;
const OBJSTAF_ACC = 0x10;

/**
 * `CMover::WingMotionSetting`: standing (STAND state, no velocity, the mover's clip at its first frame) -> the `stand` clip
 * (`floating`); accelerating -> the `walk` clip (`flying`); turning changes nothing. `prev` is the state before.
 */
export function nextWingState(prev: WingState, i: WingInput): WingState {
  if ((i.state & OBJSTA_STAND) !== 0 && i.stopped && i.frameZero) {
    return prev !== 'floating' ? 'floating' : prev;
  }
  if ((i.stateFlags & OBJSTAF_ACC) !== 0) return prev !== 'flying' ? 'flying' : prev;
  if ((i.state & OBJSTA_TURN_ALL) !== 0) return prev !== 'turning' ? 'turning' : prev; // LTURN | RTURN
  return prev;
}

/** The slot of the clip a wing state plays (`stand` / `walk`); `turning` has no clip of its own. */
export function wingMotion(state: WingState): number | null {
  if (state === 'floating') return MTI_STAND;
  if (state === 'flying') return MTI_WALK;
  return null;
}

// ---- the decor entry -------------------------------------------------------------------------------------------------------

/** The first decor id of the ride entries; the id of a mover's ride is `RIDE_DECOR_BASE + (objId & 0x0fffffff)` (the statics count from 1). */
export const RIDE_DECOR_BASE = 0xa000_0000;

/** The decor id of the ride of mover `objId`. */
export function rideDecorId(objId: number): number {
  return (RIDE_DECOR_BASE + (objId & 0x0fff_ffff)) >>> 0;
}

/** The `Mat43` identity (LH, 12 floats) of a decor entry without an offset. */
const IDENTITY_43: readonly number[] = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0];

/** `DECOR_ADD.attach` codes (`wire.ts`/Plan 31 17.4): 0 root, 5 wing. */
export const ATTACH_ROOT = 0;
export const ATTACH_WING = 5;

export interface RideDecorSpec {
  readonly moverId: number;
  readonly itemId: number;
  readonly kind: RideKind;
  /** `skel_id` of the wing's `.chr` (0: rigid node animation). */
  readonly skelId: number;
  /** The clip: a `ClipId`, or `DECOR.CLIP_BUILTIN` for the model's own node animation. */
  readonly clipId: number;
  /** The entry is drawn (the mover flies). */
  readonly visible: boolean;
}

/**
 * The `DECOR_ADD` record of a ride. `SOUNDS` is set and `SOUND_POSITIONAL` is NOT: the sounds of the ride's clip are played
 * non-positionally (KEEP-QUIRK, Mover.cpp:3637-3819 passes `NULL` as the position, Plan/44 T02).
 */
export function rideDecorAdd(s: RideDecorSpec): DecorAdd {
  return {
    id: rideDecorId(s.moverId),
    kind: DECOR_BITS.KIND_ATTACHED,
    flags: DECOR_BITS.LOOP | DECOR_BITS.SOUNDS | (s.visible ? DECOR_BITS.VISIBLE : 0),
    attach: s.kind === 'wing' ? ATTACH_WING : ATTACH_ROOT,
    modelType: OT_ITEM,
    modelIndex: s.itemId,
    clipId: s.clipId,
    skelId: s.skelId,
    parentMover: s.moverId,
    startFrame: 0,
    local: IDENTITY_43,
  };
}

/** What a ride needs from the game besides the decor sink (the pose feeder and the motion table). */
export interface RideAssets {
  readonly decor: DecorSink;
  /** The `skel_id` of a `.chr` path (requests the file once); absent: wings have no sim pose. */
  skeletonId?(path: string): number;
  /** Makes sure the sim gets clip `clip` (the `.ani` is fetched and forwarded once). */
  requestClip?(clip: number): void;
  /** The clip of motion `mti` of model `(ot, index)` (`MotionTable::resolve`), `null` when there is none. */
  resolveClip?(ot: number, index: number, mti: number): number | null;
}

/**
 * The sim side of one mover's ride: registers the attached decor entry, toggles its visibility with the flight, switches a wing's
 * clip with `WingMotionSetting` and removes the entry with the ride.
 */
export class RideDecor {
  private spec: Omit<RideDecorSpec, 'visible'> | null = null;
  private visible = false;
  private wing: WingState = 'floating';
  /** `DECOR_ADD` records written (tests, probe). */
  adds = 0;

  constructor(
    private readonly moverId: number,
    private readonly assets: RideAssets,
  ) {}

  /** The wing state in effect. */
  get wingState(): WingState {
    return this.wing;
  }

  /** The decor id of the entry (`0` while none is registered). */
  get id(): number {
    return this.spec === null ? 0 : rideDecorId(this.moverId);
  }

  /**
   * (Re)registers the entry for `itemId` of `kind`; `chr` is the wing's skeleton file. A `null` item removes the entry. Called when the ride
   * item changes (the entry is replaced, its clock restarts).
   */
  set(itemId: number | null, kind: RideKind, chr: string | null, builtinClip: number | null): void {
    if (itemId === null) {
      this.clear();
      return;
    }
    this.wing = 'floating';
    let skelId = 0;
    let clipId: number = DECOR_BITS.CLIP_BUILTIN;
    if (kind === 'wing') {
      const clip = this.assets.resolveClip?.(OT_ITEM, itemId, MTI_STAND) ?? null;
      if (chr === null || this.assets.skeletonId === undefined || clip === null) {
        // a wing without a skeleton or a clip stays a static model: no entry (the sounds of a clipless wing do not exist)
        this.clear();
        return;
      }
      skelId = this.assets.skeletonId(chr);
      clipId = clip;
      this.assets.requestClip?.(clip);
    } else if (builtinClip === null) {
      // no node animation: the entry would only be an empty clock; the ride is still registered so that a clip-less model costs nothing
      clipId = DECOR_BITS.CLIP_BUILTIN;
    }
    this.spec = { moverId: this.moverId, itemId, kind, skelId, clipId };
    this.send();
  }

  private send(): void {
    if (this.spec === null) return;
    this.assets.decor.add(rideDecorAdd({ ...this.spec, visible: this.visible }));
    this.adds += 1;
  }

  /** The ride shows only while the mover flies. */
  setVisible(visible: boolean): void {
    if (this.visible === visible) return;
    this.visible = visible;
    if (this.spec !== null) this.assets.decor.setVisible(rideDecorId(this.moverId), visible);
  }

  /** A wing frame: `WingMotionSetting` decides the clip; a change of clip re-sends the entry (its clock restarts at frame 0). */
  setWingState(next: WingState): void {
    if (next === this.wing) return;
    this.wing = next;
    const spec = this.spec;
    if (spec?.kind !== 'wing') return;
    const mti = wingMotion(next);
    if (mti === null) return; // turning keeps the playing clip
    const clip = this.assets.resolveClip?.(OT_ITEM, spec.itemId, mti) ?? null;
    if (clip === null) return;
    this.assets.requestClip?.(clip);
    this.spec = { ...spec, clipId: clip };
    this.send();
  }

  clear(): void {
    if (this.spec !== null) this.assets.decor.remove(rideDecorId(this.moverId));
    this.spec = null;
    this.visible = false;
  }
}

// ---- the visual -------------------------------------------------------------------------------------------------------------

export interface RideVisualOptions {
  readonly moverId: number;
  /** The mover's root group: the ride instance hangs below it (so the mover's world matrix applies). */
  readonly parent: Object3D;
  readonly host: VisualHost;
  readonly models: ModelProvider;
  readonly registry: VisualRegistryLike;
  readonly pose: PoseSource;
  readonly texturePrio?: Prio;
  /** The sim side (decor entry, wing clips); without it the ride is a static model (a wing in its bind pose). */
  readonly assets?: RideAssets;
  /** Registers a load with the owner (`complete` waits for it). */
  readonly track: (p: Promise<unknown>) => void;
}

const _m = new Matrix4();

/**
 * The ride of ONE mover: the item model of its `PARTS_RIDE` slot (a broom, a board, a wing, a mount), drawn while the mover flies, and
 * its attached decor entry in the sim. A wing with a sim pose is drawn skinned with the bone palette of its decor entry; it appears
 * when that palette exists.
 */
export class RideVisual {
  private item: number | null = null;
  private file: string | null = null;
  private kind: RideKind = 'other';
  private inst: ModelInstance | null = null;
  private nodes: NodeParent[] = [];
  private nodeEntry = false;
  private generation = 0;
  private disposed = false;
  private readonly decor: RideDecor | null;
  /** The bone palette of the wing's decor entry (the wing is skinned with it). */
  private skin: SkinBinding | null = null;
  private lastPos: [number, number, number] | null = null;
  private readonly scratch: Matrix4[] = [];
  private readonly tmp = new Matrix4();

  constructor(private readonly o: RideVisualOptions) {
    this.decor = o.assets ? new RideDecor(o.moverId, o.assets) : null;
  }

  /** The ride instance (tests, probes): `null` without a ride or before its model loaded. */
  get instance(): ModelInstance | null {
    return this.inst;
  }

  /** The decor bookkeeping (tests, probes). */
  get decorState(): RideDecor | null {
    return this.decor;
  }

  /** The kind of the ride in effect. */
  get rideKind(): RideKind {
    return this.kind;
  }

  /** The palette base the wing is skinned with (`NO_PALETTE` while it has none; tests, probes). */
  get skinBase(): number {
    return this.skin?.base.value ?? NO_PALETTE;
  }

  /** The state changed: loads (or drops) the ride model and (re)registers its decor entry. */
  setState(state: MoverVisualState, ifParts: boolean): void {
    const item = state.rideItem !== null && ifParts ? state.rideItem : null;
    const reg = this.o.registry;
    const file = item !== null ? reg.itemFile(item, state.sex) : null;
    if (file === this.file && item === this.item) return;
    this.file = file;
    this.item = item;
    this.kind = item !== null ? rideKindOf(reg.item(item)?.kind3) : 'other';
    this.drop();
    if (file === null || item === null) {
      this.decor?.set(null, 'other', null, null);
      return;
    }
    const gen = ++this.generation;
    const kind = this.kind;
    this.o.track(
      this.o.models.model(file, this.o.texturePrio).then(async (model) => {
        if (this.disposed || gen !== this.generation || model === null) return;
        this.registerDecor(item, kind, model);
        const skinned = kind === 'wing' && this.decor !== null && this.decor.id !== 0;
        this.skin = skinned ? { palette: this.o.pose.palette, base: { value: NO_PALETTE } } : null;
        const inst = this.o.host.createInstance(
          model,
          {
            lod: 0,
            ...(this.skin ? { skin: this.skin } : {}),
            ...(this.o.texturePrio !== undefined ? { texturePrio: this.o.texturePrio } : {}),
          },
          this.o.parent,
        );
        inst.root.visible = false; // `update` shows it while the mover flies
        this.inst = inst;
        await inst.ready;
      }),
    );
  }

  /** Registers the attached decor entry of the ride (the clip of a node-animated ride is handed over once). */
  private registerDecor(item: number, kind: RideKind, model: ModelGeometry): void {
    const d = this.decor;
    if (d === null) return;
    const assets = this.o.assets;
    let builtin: number | null = null;
    if (kind !== 'wing' && model.nodeClip !== null && model.maxFrame > 0 && assets) {
      builtin = assets.decor.builtinClip(OT_ITEM, item);
      if (builtin !== null) assets.decor.loadClip(builtin, model.nodeClip);
    }
    this.nodeEntry = builtin !== null;
    this.nodes = (model.lods[0] ?? []).map((ob) => ({
      parent: ob.parent.kind === 'object' ? ob.parent.index : -1,
    }));
    const chr = kind === 'wing' ? (this.o.registry.itemModel?.(item)?.bone ?? null) : null;
    d.set(item, kind, chr, builtin);
  }

  private drop(): void {
    if (this.inst) this.o.host.removeInstance(this.inst);
    this.inst = null;
    this.skin = null;
    this.nodeEntry = false;
    this.nodes = [];
    this.lastPos = null;
  }

  /**
   * One frame: the ride shows while the mover flies, at the matrix of its kind; the sim entry follows the flight, a wing's clip follows
   * `WingMotionSetting` and its bone palette comes from the entry.
   */
  update(fr: MoverPoseFrame, decor: DecorView | null): void {
    const flying = (fr.flags & MOVER_FLAGS.FLY) !== 0;
    this.decor?.setVisible(flying);
    const inst = this.inst;
    if (inst === null) return;
    if (!flying) {
      inst.root.visible = false;
      this.lastPos = null;
      return;
    }
    const placed = rideLocalMatrix(this.kind, this.item ?? 0, fr.attach, fr.attachOffset, _m);
    inst.root.matrix.copy(_m);
    inst.root.matrixWorldNeedsUpdate = true;
    let shown = placed;
    if (this.kind === 'wing' && this.decor !== null) {
      const last = this.lastPos;
      const stopped =
        last !== null && last[0] === fr.pos[0] && last[1] === fr.pos[1] && last[2] === fr.pos[2];
      this.lastPos = [fr.pos[0], fr.pos[1], fr.pos[2]];
      this.decor.setWingState(
        nextWingState(this.decor.wingState, {
          state: fr.state ?? 0,
          stateFlags: fr.stateFlags,
          stopped,
          frameZero: fr.frameCur === 0,
        }),
      );
    }
    if (this.skin !== null) {
      // the wing is skinned with the palette of its decor entry; before the sim evaluated it nothing is drawn
      const i = decor === null ? -1 : decor.find(rideDecorId(this.o.moverId));
      if (decor !== null && i >= 0 && decor.hasPose(i)) this.skin.base.value = decor.paletteBase(i);
      else shown = false;
    } else if (this.nodeEntry && decor !== null) {
      const i = decor.find(rideDecorId(this.o.moverId));
      if (i >= 0 && decor.hasPose(i)) {
        nodeLocals(
          this.nodes,
          (k, out) => decor.matrix(i, k, out),
          (n, local) => {
            this.tmp.copy(local);
            inst.setNodeMatrix(n, this.tmp);
          },
          this.scratch,
        );
      }
    }
    inst.root.visible = shown;
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
    this.drop();
    this.decor?.clear();
  }
}
