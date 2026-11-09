// The visual of ONE mover (Plan/41 section 12, WP-REND-05): which models it shows (`resolveVisualPlan`: skeleton, whole
// model or assembled parts, weapons), their GPU instances with palette skinning, the per-instance texture rules (texture
// set, skin set, eye blink), the hair ambient, the attachments of weapons/shield/wings, the LOD group, the bounding box
// and the per-frame update from a pose source. Appearance changes (`setState`) diff the elements: unchanged parts keep
// their instances (Plan/41 section 12.10).
//
// Nothing here evaluates a pose: the world matrix, the bone palette base and the attachment record come from the
// `PoseSource` (SimWorld render view, or the M1 bind pose). Every mover has its own `SkinBinding` (uniform objects shared
// by the materials of its parts), so a mover costs one uniform write per frame, not one material per frame.

import { Box3, Group, Matrix4, Vector3, type Mesh, type Object3D } from 'three';
import { MI_FEMALE, MI_MALE } from '../../gen/defs/obj';
import { PARTS_HEAD } from '../../gen/defs/neuz';
import type { MoverVisualState } from '../../game/entities';
import { XE } from '../gl/materials/key';
import type { HairBinding, MaterialFactory, SkinBinding } from '../gl/materials/MaterialFactory';
import type { TextureCache } from '../gl/textures';
import { withSkinSetLiteral, type TextureNameRules } from '../gl/textureNames';
import type { ModelGeometry } from '../objects/ModelCache';
import type {
  AlternateTexture,
  ModelInstance,
  ModelInstanceOptions,
} from '../objects/ModelInstance';
import { rows3ToMatrix4 } from '../space';
import type { RGB } from '../types';
import type { Prio } from '../../workers/priority';
import type { DecorView } from '../objects/decor';
import { ATTACH_AT } from './attachments';
import { mergeablePrefix, slotTextureNames, type MergedGeometryCache } from './merge';
import { MergedBody } from './MergedBody';
import { EyeBlink } from './eyeBlink';
import { hairColorFromDword } from './hair';
import {
  assembleParts,
  MODE_TRANSPARENT,
  type AssemblyInput,
  type EquipEntry,
  type PartElement,
  type PartRegistry,
} from './partRules';
import { FLOATS_PER_MATRIX } from './PaletteTexture';
import { MOVER_FLAGS, newPoseFrame, type MoverPoseFrame, type PoseSource } from './poseSource';
import type { MoverVisualProp } from './registry';
import { RideVisual, type RideAssets } from './rides';
import { boneFromPalette, type SkeletonRender } from './skeleton';

// ---- what the visual needs from the outside ----------------------------------------------------------------

/** Models and skeletons by canonical VFS path (the asset manager in the game, `DevAssets` on the dev pages). */
export interface ModelProvider {
  /**
   * The geometry of an `.o3d` (decoded in three.js space); `null` when the file does not exist or failed. `prio` is the
   * load priority of the requesting mover (the own player's parts are `Critical`, others `Visible`).
   */
  model(path: string, prio?: Prio): Promise<ModelGeometry | null>;
  /** The render skeleton of a `.chr`; `null` when it does not exist or failed. */
  skeleton(path: string, prio?: Prio): Promise<SkeletonRender | null>;
}

/** What the visual needs of the renderer. `WorldRenderer` implements it. */
export interface VisualHost {
  /** What part merging needs (`WorldRenderer` provides it); without it every part is its own instance. */
  readonly merge?: MergeHost;
  createInstance(
    model: ModelGeometry,
    options?: ModelInstanceOptions,
    parent?: Object3D,
  ): ModelInstance;
  removeInstance(inst: ModelInstance): void;
}

/** The renderer pieces the merged bodies of movers use. */
export interface MergeHost {
  readonly materials: MaterialFactory;
  readonly textures: TextureCache;
  readonly cache: MergedGeometryCache;
}

/** The registry questions of the visual (a `VisualRegistry` implements all of them). */
export interface VisualRegistryLike extends PartRegistry {
  mover(id: number): MoverVisualProp | undefined;
  moverModel(
    index: number,
  ): { file: string | null; bone: string | null; texture_ex: number } | undefined;
  /** The registry entry of an item model (the model viewer); optional for fakes. */
  itemModel?(
    id: number,
  ): { file: string | null; distant?: number; bone?: string | null } | undefined;
  /** The registry entry of a ctrl model (`models.types["2"]`); optional for fakes (no ctrls are drawn without it). */
  ctrlModel?(index: number): { file: string | null; distant?: number } | undefined;
}

// ---- the plan: which models a state shows -------------------------------------------------------------------

/** The slot of the whole-model element of monsters, NPCs and disguises (not a `PARTS_*` index). */
export const WHOLE_SLOT = -1;

export interface VisualPlan {
  /** The skeleton `.chr` (`null` for a rigid, bone-less model). */
  readonly skeleton: string | null;
  /** `texture_ex` of the mover's registry entry: every texture of the model gets the `-etNN` variant. */
  readonly textureSet: number;
  /** The mover is assembled from parts (players, humanoid NPCs). */
  readonly ifParts: boolean;
  /** The elements to load, in load order (hidden ones included, flagged). */
  readonly elements: readonly PartElement[];
  /** `true` for a player that is not disguised: its head blinks. */
  readonly blinks: boolean;
}

const whole = (file: string | null): PartElement => ({
  slot: WHOLE_SLOT,
  role: 'body',
  file,
  itemId: null,
  mount: null,
  copyOfSlot: null,
  skinTexture: false,
  reflect: false,
  hidden: false,
});

/** The `EquipEntry` map of a visual state. */
function equipOf(state: MoverVisualState): ReadonlyMap<number, EquipEntry> {
  return state.equip;
}

/**
 * Resolves the models of a visual state (Plan/41 section 12.2): the player skeleton of its sex and the assembled
 * parts, the whole model and bone of a registry entry (monsters, pets, non-humanoid NPCs), or the disguise mover's
 * model (no weapons).
 */
export function resolveVisualPlan(
  state: MoverVisualState,
  reg: VisualRegistryLike,
  opts: { readonly activeMover: boolean },
): VisualPlan {
  const isHuman =
    state.disguiseMover === null &&
    (state.kind === 'player' ||
      state.moverIndex === MI_MALE ||
      state.moverIndex === MI_FEMALE ||
      reg.mover(state.moverIndex)?.ifParts === true);
  if (state.disguiseMover !== null) {
    const row = reg.moverModel(state.disguiseMover);
    return {
      skeleton: row?.bone ?? null,
      textureSet: row?.texture_ex ?? 0,
      ifParts: false,
      elements: [whole(row?.file ?? null)],
      blinks: false,
    };
  }
  const sex: 0 | 1 = state.sex;
  const humanIndex = isHuman ? (sex === 0 ? MI_MALE : MI_FEMALE) : state.moverIndex;
  const row = reg.moverModel(isHuman ? humanIndex : state.moverIndex);
  const input: AssemblyInput = {
    sex,
    ifParts: isHuman,
    hairMesh: state.hairMesh,
    headMesh: state.headMesh,
    equip: equipOf(state),
    hideCoat: state.hideCoat,
    mode: state.mode,
    disguised: false,
    activeMover: opts.activeMover,
  };
  const assembly = assembleParts(input, reg);
  const elements = isHuman
    ? [...assembly.elements]
    : [whole(row?.file ?? null), ...assembly.elements];
  return {
    skeleton: row?.bone ?? null,
    textureSet: row?.texture_ex ?? 0,
    ifParts: isHuman,
    elements,
    blinks: isHuman && state.kind === 'player',
  };
}

/** A visible element that can join the merged body: a skinned body, armour or whole-model part (no hair, no weapon). */
function canMerge(el: PartElement): boolean {
  return (
    !el.hidden &&
    el.file !== null &&
    el.role !== 'hair' &&
    el.role !== 'weapon' &&
    el.mount === null &&
    !el.reflect
  );
}

/** Recomputes the world matrices of the visible descendants of `o` (whose own `matrixWorld` is current). */
function updateVisibleWorld(o: Object3D): void {
  const world = o.matrixWorld;
  for (const c of o.children) {
    if (!c.visible) continue;
    if (c.matrixAutoUpdate) c.updateMatrix();
    c.matrixWorld.multiplyMatrices(world, c.matrix);
    c.matrixWorldNeedsUpdate = false;
    updateVisibleWorld(c);
  }
}

/** `true` when the skin set rule (`withSkinSetLiteral`) renames a texture of `model` at some LOD. */
export function skinRuleFires(model: ModelGeometry, skinSet: number): boolean {
  for (let lod = 0; lod < model.lodCount; lod++) {
    for (const n of slotTextureNames([model], lod)) {
      const r = withSkinSetLiteral(n, skinSet);
      if (r !== null && r !== n) return true;
    }
  }
  return false;
}

/** Signature of an element's GPU instance: when it changes the instance is rebuilt. */
function elementSig(el: PartElement, textureSet: number, state: MoverVisualState): string {
  return [
    el.slot,
    el.file ?? '-',
    textureSet,
    el.skinTexture ? state.skinSet : '-',
    el.reflect ? 'r' : '',
    el.role === 'hair' ? 'h' : '',
  ].join('|');
}

// ---- the visual -------------------------------------------------------------------------------------------

export interface MoverVisualOptions {
  /** The pose source this mover follows (SimWorld render view, or the M1 bind pose). */
  readonly pose: PoseSource;
  /** Where the visual's root group is added (the renderer scene, a preview scene). */
  readonly parent: Object3D;
  readonly host: VisualHost;
  readonly models: ModelProvider;
  readonly registry: VisualRegistryLike;
  /** World lighting is on (the hair ambient uses `fblack`); previews switch it off. */
  readonly lighting?: boolean;
  /** The own player: LOD 0 always, and the inventory branch of the cap/hat rules. */
  readonly activeMover?: boolean;
  /** Per-mover random source of the eye blink in `[0, 1)`. */
  readonly rand?: () => number;
  readonly texturePrio?: Prio;
  /** The sim side of rides and wings (decor entries, wing clips); without it a ride is a static model (WP-REND-25). */
  readonly rides?: RideAssets;
  /**
   * Merge the skinned, non-hair, non-weapon parts of this mover into one draw per material state (default `true` when
   * the host supports it, Plan/41 section 12.7 "part merging").
   */
  readonly merge?: boolean;
}

/** Per-frame inputs of `update`. */
export interface MoverFrameInput {
  readonly nowMs: number;
  /** `RenderOptions.objectDetail`: 0, 1 or 2 -> LOD distance 20 / 10 / 5. */
  readonly objectDetail: 0 | 1 | 2;
  /** The camera position (LH): the distance to the mover's logical position drives the LOD and the eye blink. */
  readonly camera?: readonly [number, number, number];
  /** An explicit distance camera -> mover in metres (overrides `camera`; tests, previews). */
  readonly distCamera?: number;
  /**
   * The pose frame the caller already read from the pose source (the world batcher reads every mover once for its own
   * decisions and may adjust `world` for hit shake and special-case models); `update` then does not read again.
   */
  readonly pose?: MoverPoseFrame;
  /**
   * Do not compute the world matrices of the part nodes: the caller draws the meshes itself (the instancer) and calls
   * `refreshWorld()` when some mesh still draws through the scene graph.
   */
  readonly deferWorld?: boolean;
  /** The decor area of this frame's render view (the bone palette of a wing, the node matrices of an animated ride). */
  readonly decor?: DecorView;
}

/** `d` of `MoverRender.cpp:383-406`: the distance of one LOD step for `objectDetail` 0 / 1 / 2. */
export const LOD_STEP: readonly [number, number, number] = [20, 10, 5];

/** The LOD group of a mover: the active one always 0, the others `min(2, floor(dist / d))`. */
export function lodFor(distCamera: number, objectDetail: 0 | 1 | 2, active: boolean): number {
  if (active) return 0;
  const step = LOD_STEP[objectDetail];
  const level = Math.floor(distCamera / step);
  return Math.min(2, Math.max(0, level));
}

interface PartInstance {
  readonly element: PartElement;
  readonly sig: string;
  readonly inst: ModelInstance;
  readonly model: ModelGeometry;
  readonly alt: AlternateTexture | null;
  /** Rigid objects parented to a bone of the mover skeleton, per LOD group. */
  readonly boneNodes: Map<number, { objectIndex: number; bone: number }[]>;
}

const _m = new Matrix4();
const _bone = new Matrix4();
const _local = new Matrix4();
const _pal = new Matrix4();

export class MoverVisual {
  /** The group of all parts (its matrix is the mover world matrix). */
  readonly root = new Group();
  /** The union of the loaded elements' model boxes, starting from an EMPTY box (24 section 10.6), three.js space. */
  readonly bounds = new Box3();
  readonly skin: SkinBinding;
  readonly hair: HairBinding;
  private plan: VisualPlan | null = null;
  /** The ride (a broom, a board or a wing while flying, Plan/41 12.4): its model, matrix and decor entry (`rides.ts`). */
  private readonly rideVisual: RideVisual;
  private skeletonPath: string | null = null;
  private skeletonRender: SkeletonRender | null = null;
  private readonly parts = new Map<number, PartInstance>();
  private readonly pending = new Map<number, string>();
  private generation = 0;
  private state: MoverVisualState;
  private lod = 0;
  private fade = 255;
  private transparent = false;
  private readonly blink: EyeBlink | null;
  private disposed = false;
  private readonly lighting: boolean;
  private readonly frame: MoverPoseFrame = newPoseFrame();
  private readonly loading: Promise<unknown>[] = [];
  /** Loads (models, skeleton, textures) that have been requested and not settled yet. */
  private outstanding = 0;
  /** The merged body of the parts that could be merged, the signature it was built for and the slots it covers. */
  private merged: {
    body: MergedBody;
    sig: string;
    slots: number[];
    models: ModelGeometry[];
  } | null = null;
  private mergedPending: string | null = null;
  /**
   * The candidate signature the last COMPLETED `buildMerged` ran for (also when it made no merged body: nothing worth
   * merging); `null` while none is valid. The body is rebuilt only when the candidate set differs from it.
   */
  private builtMergeSig: string | null = null;
  private readonly mergeEnabled: boolean;

  constructor(
    readonly id: number,
    state: MoverVisualState,
    private readonly o: MoverVisualOptions,
  ) {
    this.state = state;
    this.mergeEnabled = o.merge !== false && o.host.merge !== undefined;
    this.lighting = o.lighting !== false;
    this.skin = { palette: o.pose.palette, base: { value: 0 } };
    this.hair = { color: { value: new Vector3(1, 1, 1) }, lit: { value: this.lighting ? 1 : 0 } };
    this.blink = state.kind === 'player' ? new EyeBlink(o.rand ?? Math.random) : null;
    this.rideVisual = new RideVisual({
      moverId: id,
      parent: this.root,
      host: o.host,
      models: o.models,
      registry: o.registry,
      pose: o.pose,
      ...(o.texturePrio !== undefined ? { texturePrio: o.texturePrio } : {}),
      ...(o.rides ? { assets: o.rides } : {}),
      track: (p) => {
        this.track(p);
      },
    });
    this.bounds.makeEmpty();
    this.root.matrixAutoUpdate = false;
    this.root.name = `mover#${String(id)}`;
    this.root.visible = false; // until the first pose
    o.parent.add(this.root);
    this.setState(state);
  }

  // ---- state -------------------------------------------------------------------------------------------

  get current(): MoverVisualState {
    return this.state;
  }

  /** The element plan of the current state (tests, probes). */
  get currentPlan(): VisualPlan | null {
    return this.plan;
  }

  /** The skeleton once it has loaded. */
  get skeleton(): SkeletonRender | null {
    return this.skeletonRender;
  }

  /** The skeleton path the state needs (the host registers it with the pose source / the sim). */
  get skeletonKey(): string | null {
    return this.skeletonPath;
  }

  /** The hair colour of the state (white replaced by the default table, Plan/41 section 12.5). */
  hairColor(): RGB {
    return hairColorFromDword(this.state.hairColor, this.state.sex, this.state.hairMesh);
  }

  /** Settles when every model and texture requested so far finished loading. */
  get ready(): Promise<void> {
    return Promise.all(this.loading).then(() => undefined);
  }

  /** Registers a load: `complete` is `false` until every registered load has settled. */
  private track(p: Promise<unknown>): void {
    this.loading.push(p);
    this.outstanding++;
    const done = (): void => {
      this.outstanding--;
    };
    p.then(done, done);
  }

  /**
   * `true` when everything the current state needs is resident: the skeleton, every model and every texture requested so
   * far settled (a missing file counts as settled, it shows nothing, KQ-30). The loading gate asks it for the own player
   * (`ownModelReady`, Plan/40 9.8).
   */
  get complete(): boolean {
    return (
      !this.disposed &&
      this.plan !== null &&
      this.outstanding === 0 &&
      this.pending.size === 0 &&
      this.mergedPending === null
    );
  }

  /**
   * Applies a new visual state: re-runs the assembly and diffs the elements (Plan/41 section 12.10). The merge candidates are
   * part of the diff too: a candidate that `buildMerged` left as a separate instance (a rigid cap, everything sorted after it)
   * is kept while its signature is unchanged, and the merged body is rebuilt only when the candidate set changes.
   */
  setState(state: MoverVisualState): void {
    this.state = state;
    const plan = resolveVisualPlan(state, this.o.registry, {
      activeMover: this.o.activeMover === true,
    });
    this.plan = plan;
    const [r, g, b] = this.hairColor();
    this.hair.color.value.set(r, g, b);
    this.transparent = (state.mode & MODE_TRANSPARENT) !== 0;
    this.generation++;

    if (plan.skeleton !== this.skeletonPath) {
      this.skeletonPath = plan.skeleton;
      this.skeletonRender = null;
      if (plan.skeleton !== null) {
        const path = plan.skeleton;
        const p = this.o.models.skeleton(path, this.o.texturePrio).then((s) => {
          if (this.disposed || path !== this.skeletonPath) return;
          this.skeletonRender = s;
        });
        this.track(p);
      }
    }

    const wanted = new Map<number, { el: PartElement; sig: string }>();
    for (const el of plan.elements) {
      if (el.file === null) continue;
      wanted.set(el.slot, { el, sig: elementSig(el, plan.textureSet, state) });
    }
    // part merging: the skinned body parts of the mover become one merged body (they leave the per-part path)
    const candidates: { slot: number; el: PartElement; sig: string }[] = [];
    if (this.mergeEnabled) {
      for (const [slot, w] of [...wanted]) {
        if (canMerge(w.el)) {
          candidates.push({ slot, el: w.el, sig: w.sig });
          wanted.delete(slot);
        }
      }
      candidates.sort((a, b) => a.slot - b.slot);
    }
    const mergeSig = candidates.map((c) => c.sig).join(';');
    if (this.builtMergeSig !== mergeSig) {
      // another candidate set: the body goes, and a build of a superseded set (A, B, A before B loaded) must not show up
      this.dropMerged();
      this.builtMergeSig = null;
    }
    if (candidates.length === 0) {
      this.mergedPending = null;
      this.builtMergeSig = mergeSig;
    } else if (this.builtMergeSig === null && this.mergedPending !== mergeSig) {
      this.mergedPending = mergeSig;
      this.track(this.buildMerged(candidates, mergeSig, plan));
    }
    // remove what is gone or changed; a candidate that is a separate instance (rest of the merge) stays while its sig holds
    const candidateSig = new Map(candidates.map((c) => [c.slot, c.sig]));
    for (const [slot, part] of [...this.parts]) {
      const sig = wanted.get(slot)?.sig ?? candidateSig.get(slot);
      if (sig !== part.sig) this.removePart(slot, part);
    }
    for (const [slot] of [...this.pending]) {
      const w = wanted.get(slot);
      if (w?.sig !== this.pending.get(slot)) this.pending.delete(slot);
    }
    // add what is new; refresh flags of what stays
    for (const [slot, w] of wanted) {
      const have = this.parts.get(slot);
      if (have) {
        this.parts.set(slot, { ...have, element: w.el });
        have.inst.root.visible = !w.el.hidden;
        continue;
      }
      if (this.pending.get(slot) === w.sig) continue;
      this.pending.set(slot, w.sig);
      this.track(this.addPart(slot, w.el, w.sig, plan));
    }
    // kept separate instances of merge candidates: new element flags, and shown again (a candidate is never hidden; it may
    // have been hidden by the hide-coat / fashion rules while it was not a candidate)
    for (const c of candidates) {
      const have = this.parts.get(c.slot);
      if (have?.sig === c.sig) {
        this.parts.set(c.slot, { ...have, element: c.el });
        have.inst.root.visible = true;
      }
    }
    this.rideVisual.setState(state, plan.ifParts);
    this.applyBlend();
    this.rebuildBounds();
  }

  /** The ride instance (tests, probes): `null` without a ride or before its model loaded. */
  get rideInstance(): ModelInstance | null {
    return this.rideVisual.instance;
  }

  /** The ride of this mover (tests, probes). */
  get rides(): RideVisual {
    return this.rideVisual;
  }

  private removePart(slot: number, part: PartInstance): void {
    this.parts.delete(slot);
    this.o.host.removeInstance(part.inst);
  }

  private dropMerged(): void {
    if (!this.merged) return;
    this.merged.body.dispose();
    this.merged = null;
    this.rebuildBounds();
  }

  /**
   * Loads the models of the merge candidates and builds the merged body; what cannot be merged stays separate (instances that
   * already exist are kept, later `setState` calls keep them as long as the candidate set is the same). A separate instance of a
   * slot that joins the body is removed.
   */
  private async buildMerged(
    candidates: readonly { slot: number; el: PartElement; sig: string }[],
    sig: string,
    plan: VisualPlan,
  ): Promise<void> {
    const loaded = await Promise.all(
      candidates.map(async (c) => ({
        c,
        model: c.el.file ? await this.o.models.model(c.el.file, this.o.texturePrio) : null,
      })),
    );
    if (this.disposed || this.mergedPending !== sig) return;
    this.mergedPending = null;
    this.builtMergeSig = sig; // also when no body is made (nothing worth merging): the same set is not built again
    const present: { c: (typeof candidates)[number]; model: ModelGeometry }[] = [];
    for (const l of loaded) if (l.model) present.push({ c: l.c, model: l.model }); // a missing model shows nothing
    // a body part whose texture the skin set rule would rename stays a plain instance (the merged slots hold the raw names)
    const eligible = present.filter(
      (p) => !(p.c.el.skinTexture && skinRuleFires(p.model, this.state.skinSet)),
    );
    const n = mergeablePrefix(eligible.map((p) => p.model));
    const set = eligible.slice(0, n);
    const blocks = set.reduce(
      (k, p) => k + (p.model.lods[0] ?? []).reduce((m, o) => m + o.blocks.length, 0),
      0,
    );
    // one part of one block gains nothing from merging: keep it a plain instance
    const worth = n >= 2 || blocks >= 2;
    const mergeSet = worth ? set : [];
    const rest = present.filter((p) => !mergeSet.includes(p));
    const host = this.o.host.merge;
    if (mergeSet.length > 0 && host) {
      const rules: TextureNameRules = {
        ...(plan.textureSet > 0 ? { textureSet: plan.textureSet } : {}),
      };
      const body = new MergedBody(
        { materials: host.materials, textures: host.textures, cache: host.cache, skin: this.skin },
        mergeSet.map((p) => ({
          file: p.c.el.file ?? '',
          model: p.model,
          ...(plan.blinks && p.c.slot === PARTS_HEAD ? { head: true } : {}),
        })),
        {
          rules,
          modelBlend: this.blend,
          lod: this.lod,
          ...(this.o.texturePrio !== undefined ? { texturePrio: this.o.texturePrio } : {}),
        },
        this.root,
      );
      this.merged = {
        body,
        sig,
        slots: mergeSet.map((p) => p.c.slot),
        models: mergeSet.map((p) => p.model),
      };
      // a part that moves from the separate instances into the body (a cloak after the rigid cap was taken off) is not drawn twice
      for (const p of mergeSet) {
        const separate = this.parts.get(p.c.slot);
        if (separate) this.removePart(p.c.slot, separate);
      }
      this.rebuildBounds();
      this.track(body.ready);
    }
    await Promise.all([
      ...rest.map((r) => this.instantiatePart(r.c.slot, r.c.el, r.c.sig, plan, r.model)),
      this.merged?.body.ready,
    ]);
  }

  private async addPart(
    slot: number,
    el: PartElement,
    sig: string,
    plan: VisualPlan,
  ): Promise<void> {
    if (el.file === null) return;
    const model = await this.o.models.model(el.file, this.o.texturePrio);
    if (this.disposed || this.pending.get(slot) !== sig) return;
    this.pending.delete(slot);
    if (model === null) return; // a missing model shows nothing (KQ-30)
    await this.instantiatePart(slot, el, sig, plan, model);
  }

  private async instantiatePart(
    slot: number,
    el: PartElement,
    sig: string,
    plan: VisualPlan,
    model: ModelGeometry,
  ): Promise<void> {
    if (this.parts.has(slot)) return;
    const rules: TextureNameRules = {
      ...(plan.textureSet > 0 ? { textureSet: plan.textureSet } : {}),
      ...(el.skinTexture ? { skinSet: this.state.skinSet, skinSetLiteral: true } : {}),
    };
    const options: ModelInstanceOptions = {
      lod: this.lod,
      rules,
      modelBlend: this.blend,
      skin: this.skin,
      ...(el.role === 'hair' ? { hair: this.hair } : {}),
      ...(el.reflect ? { elementEffect: XE.REFLECT } : {}),
      ...(this.o.texturePrio !== undefined ? { texturePrio: this.o.texturePrio } : {}),
    };
    const inst = this.o.host.createInstance(model, options, this.root);
    inst.root.visible = !el.hidden;
    let alt: AlternateTexture | null = null;
    if (plan.blinks && el.slot === PARTS_HEAD) {
      alt = inst.addAlternate(0, 0, { eyeFlash: true });
    }
    const boneNodes = new Map<number, { objectIndex: number; bone: number }[]>();
    model.lods.forEach((objs, lodIndex) => {
      const list: { objectIndex: number; bone: number }[] = [];
      objs.forEach((obj, objectIndex) => {
        if (obj.kind === 'rigid' && obj.parent.kind === 'bone')
          list.push({ objectIndex, bone: obj.parent.index });
      });
      if (list.length > 0) boneNodes.set(lodIndex, list);
    });
    this.parts.set(slot, { element: el, sig, inst, model, alt, boneNodes });
    this.rebuildBounds();
    await inst.ready; // textures (and the alternate eye texture) settled: `ready` covers them
  }

  private rebuildBounds(): void {
    this.bounds.makeEmpty();
    for (const p of this.parts.values()) this.bounds.union(p.model.bbox);
    for (const m of this.merged?.models ?? []) this.bounds.union(m.bbox);
  }

  // ---- per frame ---------------------------------------------------------------------------------------

  /** The current LOD group. */
  get lodGroup(): number {
    return this.lod;
  }

  /** The model blend factor in effect: 80 for a TRANSPARENT_MODE mover, else the fade factor (0..255). */
  get blend(): number {
    return this.transparent ? 80 : this.fade;
  }

  /** The fade-in / fade-out factor 0..255 (Plan/41 section 22.4: 250 ms ramp); 255 = opaque. */
  setFade(fade: number): void {
    this.fade = Math.min(255, Math.max(0, Math.trunc(fade)));
    this.applyBlend();
  }

  private applyBlend(): void {
    const b = this.blend;
    for (const p of this.parts.values()) p.inst.setModelBlend(b);
    this.merged?.body.setModelBlend(b);
  }

  /**
   * One frame: reads the pose, positions the root and the attachments, selects the LOD, drives the eye blink.
   * Returns `false` (and hides the mover) when the source has no pose for it.
   */
  update(f: MoverFrameInput): boolean {
    const fr = f.pose ?? this.frame;
    const have = f.pose !== undefined || this.o.pose.read(this.id, fr);
    const visible =
      have && (fr.flags & MOVER_FLAGS.VISIBLE) !== 0 && fr.paletteBase !== 0xffff_ffff;
    this.root.visible = visible;
    if (!visible) return false;
    this.skin.base.value = fr.paletteBase;
    this.root.matrix.copy(fr.world);
    this.root.matrixWorldNeedsUpdate = true;

    const dist =
      f.distCamera ??
      (f.camera
        ? Math.hypot(fr.pos[0] - f.camera[0], fr.pos[1] - f.camera[1], fr.pos[2] - f.camera[2])
        : 0);
    const lod = lodFor(dist, f.objectDetail, this.o.activeMover === true);
    if (lod !== this.lod) {
      this.lod = lod;
      for (const p of this.parts.values()) p.inst.setLod(lod);
      this.merged?.body.setLod(lod);
    }

    for (const p of this.parts.values()) this.updatePart(p, fr);
    this.rideVisual.update(fr, f.decor ?? null);
    if (this.blink) {
      const closed = this.blink.update(f.nowMs, dist, (fr.flags & MOVER_FLAGS.DEAD) !== 0);
      for (const p of this.parts.values()) p.alt?.set(closed);
      this.merged?.body.setEyesClosed(closed);
    }
    if (f.deferWorld !== true) this.updateWorld();
    return true;
  }

  /** Computes the world matrices of the drawn part nodes now (after `update` with `deferWorld`). */
  refreshWorld(): void {
    this.updateWorld();
  }

  /**
   * `root.updateMatrixWorld(true)` without the hidden subtrees (the other LOD groups, meshes whose textures are not
   * resident): a crowd pays one matrix product per drawn node only. A subtree that is shown later (LOD change, texture
   * settled) is refreshed by the next `update`, which runs before every draw.
   */
  private updateWorld(): void {
    const root = this.root;
    const parent = root.parent;
    if (root.matrixAutoUpdate) root.updateMatrix();
    if (parent) root.matrixWorld.multiplyMatrices(parent.matrixWorld, root.matrix);
    else root.matrixWorld.copy(root.matrix);
    root.matrixWorldNeedsUpdate = false;
    updateVisibleWorld(root);
  }

  private updatePart(p: PartInstance, fr: MoverPoseFrame): void {
    const el = p.element;
    if (el.mount !== null) {
      // a weapon / shield hangs on its attachment point; without a record it cannot be placed: hidden
      if (fr.attach === null) {
        p.inst.root.visible = false;
      } else {
        p.inst.root.visible = !el.hidden;
        rows3ToMatrix4(fr.attach, fr.attachOffset + ATTACH_AT[el.mount], p.inst.root.matrix);
        p.inst.root.matrixWorldNeedsUpdate = true;
      }
    }
    const nodes = p.boneNodes.get(p.inst.lod);
    if (nodes && this.skeletonRender && fr.palette) {
      for (const n of nodes) {
        const obj = p.model.lods[p.inst.lod]?.[n.objectIndex];
        if (!obj) continue;
        rows3ToMatrix4(fr.palette, fr.paletteOffset + n.bone * FLOATS_PER_MATRIX, _pal);
        boneFromPalette(this.skeletonRender, n.bone, _pal, _bone);
        // bone[b] x localTM (the node group matrix is relative to the model root)
        _local.copy(obj.localTM);
        _m.multiplyMatrices(_bone, _local);
        p.inst.setNodeMatrix(n.objectIndex, _m);
      }
    }
  }

  // ---- probes ------------------------------------------------------------------------------------------

  /** Draw calls this mover costs now: blocks of the visible instances in the current LOD that are drawn. */
  drawCalls(): number {
    if (!this.root.visible) return 0;
    let n = 0;
    for (const p of this.parts.values()) {
      if (!p.inst.root.visible) continue;
      for (const b of p.inst.blocks) if (b.mesh.visible) n++;
    }
    return n + (this.merged?.body.drawCalls() ?? 0);
  }

  /**
   * Calls `fn` for every mesh drawn now: the blocks of the visible part instances and the meshes of the merged body (current
   * LOD group, textures settled). Highlight twins, picking and the draw statistics read it.
   */
  forEachDrawnMesh(fn: (mesh: Mesh) => void): void {
    if (!this.root.visible) return;
    for (const p of this.parts.values()) {
      if (!p.inst.root.visible) continue;
      for (const b of p.inst.blocks) if (b.mesh.visible) fn(b.mesh);
    }
    this.merged?.body.forEachDrawnMesh(fn);
  }

  /**
   * The models of the visible, unmounted elements (separate parts and the parts of the merged body): the geometry the precise
   * ray test of the pick support walks (LOD 0, Plan/41 20.3). Weapons and shields are not part of the body.
   */
  pickModels(): ModelGeometry[] {
    const out: ModelGeometry[] = [];
    for (const p of this.parts.values()) {
      if (!p.element.hidden && p.element.mount === null) out.push(p.model);
    }
    for (const m of this.merged?.models ?? []) out.push(m);
    return out;
  }

  /** The merged body (probes, tests): `null` while parts are separate. */
  get mergedBody(): MergedBody | null {
    return this.merged?.body ?? null;
  }

  /** The slots of the loaded (resident) parts, merged ones included. */
  loadedSlots(): number[] {
    return [...this.parts.keys(), ...(this.merged?.slots ?? [])].sort((a, b) => a - b);
  }

  /** The world position (three.js space) of the instance of a part slot after the last `update`; `null` when absent. */
  partPosition(slot: number): [number, number, number] | null {
    const p = this.parts.get(slot);
    if (!p) {
      if (!this.merged?.slots.includes(slot)) return null;
      const m = this.root.matrixWorld.elements; // merged parts share the mover's world matrix
      return [m[12], m[13], m[14]];
    }
    const e = p.inst.root.matrixWorld.elements;
    return [e[12], e[13], e[14]];
  }

  /** The instance of a part slot (tests, previews). */
  partOf(slot: number): ModelInstance | null {
    return this.parts.get(slot)?.inst ?? null;
  }

  /** `true` while the closed-eyes texture is shown. */
  get eyesClosed(): boolean {
    return this.blink?.closed ?? false;
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
    for (const [slot, p] of [...this.parts]) this.removePart(slot, p);
    this.dropMerged();
    this.builtMergeSig = null;
    this.rideVisual.dispose();
    this.pending.clear();
    this.root.removeFromParent();
  }
}
