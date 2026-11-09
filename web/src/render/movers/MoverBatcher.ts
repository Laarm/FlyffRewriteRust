// The world's movers (Plan/41 section 12.6-12.10, WP-REND-12): one `MoverVisual` per mover that is drawn, driven every
// frame from the entity table (WHAT a mover looks like) and the sim render view (WHERE it is and its pose):
//
//   * `SET_VISIBLE` management: the renderer decides who needs a pose and tells the sim ONLY on change (`want_attach` =
//     something is mounted on the mover). A mover (<= 384 m, not hidden by a rule) is posed when its draw sphere (`moverCullSphere`,
//     a bound of the model box) is in the frustum, when it stands inside the pose window (96 m square around the active mover:
//     the server's view window, in view or not) or when the frustum reaches within 8 m of its sphere; leaving all three keeps the
//     pose for a second (#018 B4, B5),
//   * visual lifecycle: created when a mover is first posed (so the models and textures of everybody inside the window are
//     resident before the camera turns to them), appearance changes re-run the part assembly (the visual diffs the elements),
//     removed with the entity (or 60 s after a mover beyond the window was last wanted),
//   * per-mover rules: far cut-off, TRANSPARENT_MODE, render-skip modes, hidden pets, the telescope, LOD, hit shake and the
//     special-case models (`moverRules.ts`),
//   * the loading gate: `ownModelReady` holds once the own player's visual is complete and showed a pose,
//   * the list of the movers drawn in the last frame (blob shadows, picking, nameplates read it) and the stats.
//
// The batcher never evaluates a pose or a gameplay formula: matrices, palettes and attachment records come from the
// `PoseSource` (the `SimWorld` render view, rule B1). Draw-call batching of many movers is `MoverInstancer` (see there).

import { Box3, Sphere, type Frustum, type Object3D } from 'three';
import { RANK_NORMAL } from '../../gen/defs/attribute';
import { AII_EGG, AII_NONE, AII_PET } from '../../gen/defs/neuz';
import type { MoverVisualState } from '../../game/entities';
import { Prio } from '../../workers/priority';
import type { RenderOptions } from '../quality';
import type { EntityViewRecord, FrameInput } from '../types';
import type { MoverInstancer } from './MoverInstancer';
import { lodFor, MoverVisual, type ModelProvider, type VisualHost } from './MoverVisual';
import type { VisualRegistryLike } from './MoverVisual';
import {
  decideDraw,
  hasDecorations,
  HIT_SHAKE_TICKS,
  ignoresServerScale,
  inPoseWindow,
  isSyliaca,
  MOVER_KEEP_ALIVE_MS,
  moverCullSphere,
  POSE_LOOKAHEAD_M,
  shakeOffset,
  shakes,
  syliacaLift,
  VISIBLE_GRACE_MS,
  wantsAttach,
  type DrawVerdict,
} from './moverRules';
import { MOVER_FLAGS, newPoseFrame, type MoverPoseFrame, type PoseSource } from './poseSource';
import { DecorView } from '../objects/decor';
import type { RideAssets } from './rides';

/** Why a mover is not drawn: a rule (`moverRules`), outside the view, no pose yet, or its models and textures are still loading. */
export type EntryVerdict = DrawVerdict | 'culled' | 'noPose' | 'loading';

/** What the batcher tells the simulation (`SimBridge.cmd.setVisible`, Plan/31 section 17.4 op 0x0F). */
export interface MoverSimSink {
  setVisible(id: number, visible: boolean, wantAttach: boolean): void;
}

export interface MoverBatcherDeps {
  readonly host: VisualHost;
  /** Where the visuals' root groups go (the renderer's shared scene). */
  readonly parent: Object3D;
  readonly pose: PoseSource;
  readonly models: ModelProvider;
  readonly registry: VisualRegistryLike;
  readonly sim: MoverSimSink;
  /** Draw-call batching of the drawn movers (WP-REND-12); without it every mesh draws on its own. */
  readonly instancer?: MoverInstancer;
  readonly options: () => RenderOptions;
  /**
   * Makes the pose source follow the render view of this frame (`SimPoseSource.useView`, which also uploads the palette).
   * Returns `false` when the frame has no view.
   */
  readonly bindView: (view: unknown) => boolean;
  /** World lighting is on (hair ambient); previews switch it off. */
  readonly lighting?: boolean;
  /** Random source of the eye blink and the hit shake in `[0, 1)`. */
  readonly rand?: () => number;
  /**
   * Milliseconds a mover beyond the pose window that is not wanted keeps its visual (default {@link MOVER_KEEP_ALIVE_MS}, 60 s).
   * Inside the window the visual is kept for as long as the mover is there.
   */
  readonly keepAliveMs?: number;
  /** After this long a complete own visual without a pose counts as ready anyway (default 2 s). */
  readonly poseGraceMs?: number;
  /**
   * The appearance of a mover with a visual changed (equipment, hair, disguise ...): the owner re-sends `SET_PARAMS`, whose
   * bounds, weapon stance and ride follow the look (Plan/40 7.4).
   */
  readonly onAppearanceChanged?: (id: number) => void;
  /** The sim side of rides and wings (decor entries, wing clips, WP-REND-25). */
  readonly rides?: RideAssets;
}

/** One mover of the batcher (the record of the last frame it was visited). */
export interface MoverEntry {
  readonly id: number;
  rec: EntityViewRecord;
  visual: MoverVisual | null;
  /** The visual state hash the visual was built for. */
  hash: number;
  /** What the sim was told (`null` = nothing yet, the sim default is "not visible"). */
  simVisible: boolean;
  simAttach: boolean;
  /** `true` when the mover was drawn in the last `update`. */
  drawn: boolean;
  verdict: EntryVerdict;
  /** Logical position (LH), yaw (degrees), radius, ground height and camera distance of the last frame. */
  readonly pos: [number, number, number];
  /** The smoothed position of the draw matrix (LH): where a pet stands (`GetScrPos`). */
  readonly drawPos: [number, number, number];
  angle: number;
  radius: number;
  currentHeight: number;
  dist: number;
  flags: number;
  /** `AII_PET` / `AII_EGG`. */
  pet: boolean;
  /** Not GM-invisible: gets a shadow, a name and the highlight. */
  decorated: boolean;
  /** `MoverProp.dwClass` / `dwAI` of the registry. */
  rank: number;
  ai: number;
  /** A pose was shown at least once. */
  poseSeen: boolean;
  /** `performance.now()` at which the visual became complete (`0` while loading). */
  completeAt: number;
  /** Hit shake: ticks left and the offset of the current tick (LH x, z). */
  shake: number;
  shakeX: number;
  shakeZ: number;
  /** The mover stands inside the pose window of the active mover (not the active mover itself, not hidden by a rule). */
  inWindow: boolean;
  /**
   * Last time (ms) the mover was wanted posed (in view, inside the pose window or within the look-ahead, not hidden by a rule):
   * `SET_VISIBLE` stays on for a second after it, and the visual of a mover beyond the window goes after the keep-alive.
   */
  lastWanted: number;
  seen: number;
}

/** Counters of the last `update` (probe `render.movers`). */
export interface MoverBatchStats {
  /** Movers of the entity table. */
  entities: number;
  /** Movers with a live visual. */
  visuals: number;
  /** Movers drawn this frame. */
  drawn: number;
  /** Drawn per verdict (`far`, `transparent`, ...). */
  hidden: Partial<Record<EntryVerdict, number>>;
  /** `SET_VISIBLE` records sent so far. */
  visibleSent: number;
  /** Visuals created and disposed so far. */
  created: number;
  disposed: number;
  /** Draw calls of the drawn movers (blocks of the visible instances + merged meshes). */
  drawCalls: number;
  /** CPU time of the last `update` (ms). */
  updateMs: number;
}

const FNV = 0x01000193;
const _sphere = new Sphere();
const _box = new Box3();

/** A 32-bit hash of the parts of a visual state that decide the models of a mover (not names, titles or guilds). */
export function visualHash(v: MoverVisualState): number {
  let h = 0x811c9dc5;
  const mix = (n: number): void => {
    h = Math.imul(h ^ (n | 0), FNV);
    h = Math.imul(h ^ ((n / 4294967296) | 0), FNV);
  };
  mix(v.kind === 'player' ? 1 : v.kind === 'npc' ? 2 : v.kind === 'monster' ? 3 : 4);
  mix(v.moverIndex);
  mix(v.sex);
  mix(v.hairMesh);
  mix(v.hairColor);
  mix(v.headMesh);
  mix(v.skinSet);
  mix(v.mode);
  mix(v.disguiseMover ?? -1);
  mix(v.rideItem ?? -1);
  for (const c of v.hideCoat) mix(c ? 1 : 0);
  for (const [part, e] of v.equip) {
    mix(part);
    mix(e.itemId);
    mix(e.flag);
  }
  return h >>> 0;
}

export class MoverBatcher {
  private readonly entries = new Map<number, MoverEntry>();
  private readonly frame: MoverPoseFrame = newPoseFrame();
  /** The decor area of the frame's render view: the bone palette of the wings and the node matrices of animated rides. */
  private readonly rideView = new DecorView();
  private frameNo = 0;
  private selfId = 0;
  private highlight = 0;
  private readonly rand: () => number;
  private readonly keepAliveMs: number;
  private readonly poseGraceMs: number;
  private disposed = false;
  readonly stats: MoverBatchStats = {
    entities: 0,
    visuals: 0,
    drawn: 0,
    hidden: {},
    visibleSent: 0,
    created: 0,
    disposed: 0,
    drawCalls: 0,
    updateMs: 0,
  };
  /** The movers drawn in the last `update`, in visit order (reused: read it inside the frame). */
  private readonly drawnList: MoverEntry[] = [];

  constructor(private readonly d: MoverBatcherDeps) {
    this.rand = d.rand ?? Math.random;
    this.keepAliveMs = d.keepAliveMs ?? MOVER_KEEP_ALIVE_MS;
    this.poseGraceMs = d.poseGraceMs ?? 2000;
  }

  // ---- per frame ------------------------------------------------------------------------------------------

  /**
   * One drawn frame: reads the entity table and the pose source, decides visibility, tells the sim, updates the visuals.
   * `frustum` is the camera frustum of the frame (three.js space).
   */
  update(frame: FrameInput, frustum: Frustum): void {
    if (this.disposed) return;
    const t0 = performance.now();
    this.frameNo++;
    const frameNo = this.frameNo;
    this.drawnList.length = 0;
    const st = this.stats;
    st.drawn = 0;
    st.drawCalls = 0;
    st.hidden = {};
    st.entities = 0;
    const entities = frame.entities;
    const bound = frame.view !== null && this.d.bindView(frame.view);
    if (bound) this.rideView.bind(frame.view);
    this.d.instancer?.beginFrame();
    if (!bound || !entities?.forEach) {
      this.hideAll();
      this.d.instancer?.endFrame();
      st.visuals = this.countVisuals();
      st.updateMs = performance.now() - t0;
      return;
    }
    const opts = this.d.options();
    const self = entities.self?.();
    this.selfId = self?.objId ?? 0;
    const viewerAuthority = self?.visual?.authorization ?? 0;
    const cam = frame.camera;
    const camPos = cam.pos;
    const telescope = cam.telescope;
    const nowMs = frame.nowMs;
    // the centre of the pose window: the active mover (the same rule as `WorldScene.update`)
    const active = frame.activePos ?? cam.lookAt;
    entities.forEach(0, (rec) => {
      const vis = rec.visual;
      if (!vis) return;
      st.entities++;
      this.visit(rec, vis, frameNo, {
        frustum,
        camPos,
        active,
        telescope,
        viewerAuthority,
        opts,
        nowMs,
      });
    });
    // entities that are gone; a visual inside the pose window stays (the server removes what leaves it)
    for (const e of [...this.entries.values()]) {
      if (e.seen !== frameNo) this.drop(e);
      else if (!e.drawn && e.visual && !e.inWindow && nowMs - e.lastWanted > this.keepAliveMs) {
        this.release(e);
      }
    }
    this.d.instancer?.endFrame();
    st.drawCalls += this.d.instancer?.drawCalls ?? 0;
    st.visuals = this.countVisuals();
    st.updateMs = performance.now() - t0;
  }

  private visit(
    rec: EntityViewRecord,
    vis: MoverVisualState,
    frameNo: number,
    f: {
      frustum: Frustum;
      camPos: readonly [number, number, number];
      /** The active mover's position (LH): the centre of the pose window. */
      active: readonly [number, number, number];
      telescope: number;
      viewerAuthority: number;
      opts: RenderOptions;
      nowMs: number;
    },
  ): void {
    const id = rec.objId;
    let e = this.entries.get(id);
    if (e && e.rec !== rec) {
      // a NEW record of a known id (the old entity was removed and spawned again): a new sim mover, a new visual
      this.drop(e);
      e = undefined;
    }
    if (!e) {
      e = this.newEntry(id, rec);
      this.entries.set(id, e);
    }
    e.seen = frameNo;
    e.drawn = false;
    const st = this.stats;
    const fr = this.frame;
    if (!this.d.pose.read(id, fr)) {
      // the sim does not know the mover yet (ADD_MOVER is applied at the next frame)
      e.verdict = 'noPose';
      this.count(st, 'noPose');
      return;
    }
    e.flags = fr.flags;
    e.pos[0] = fr.pos[0];
    e.pos[1] = fr.pos[1];
    e.pos[2] = fr.pos[2];
    const w = fr.world.elements;
    e.drawPos[0] = w[12];
    e.drawPos[1] = w[13];
    e.drawPos[2] = -w[14];
    e.angle = fr.angle;
    e.radius = fr.radius;
    // a preview mover has no ground collision: it stands where it is placed
    e.currentHeight = (fr.flags & MOVER_FLAGS.PREVIEW) !== 0 ? fr.pos[1] : fr.currentHeight;
    const dx = fr.pos[0] - f.camPos[0];
    const dy = fr.pos[1] - f.camPos[1];
    const dz = fr.pos[2] - f.camPos[2];
    e.dist = Math.sqrt(dx * dx + dy * dy + dz * dz);
    const prop = this.d.registry.mover(vis.moverIndex);
    e.rank = prop?.rank ?? RANK_NORMAL;
    e.ai = prop?.ai ?? AII_NONE;
    e.pet = e.ai === AII_PET || e.ai === AII_EGG;
    const isActive = rec.isSelf;
    const verdict = decideDraw({
      isActive,
      mode: vis.mode,
      viewerAuthority: f.viewerAuthority,
      ai: e.ai,
      hidePets: f.opts.hidePets,
      telescope: f.telescope,
      dist: e.dist,
    });
    e.decorated = hasDecorations(vis.mode);
    let visual = e.visual;
    // who is posed (SET_VISIBLE, the visual) and who is drawn (#018 B4, B5)
    let wantPose = false;
    let wantDraw = false;
    e.inWindow = false;
    if (verdict.draw) {
      if (isActive) {
        // the active mover is never culled
        e.lastWanted = f.nowMs;
        wantPose = true;
        wantDraw = true;
      } else {
        // the draw sphere: a bound of the model box (the bounds of the visual once its models are resident), not `GetRadius`
        const box = visual && !visual.bounds.isEmpty() ? visual.bounds : null;
        const s = moverCullSphere(box, w, fr.radius, _sphere);
        // the Syliaca models are lifted by half their height at the draw (`adjustWorld`), which the margin does not always hold
        if (box && isSyliaca(vis.moverIndex)) s.center.y += syliacaLift(box.min.y, box.max.y);
        const inView = f.frustum.intersectsSphere(s);
        const near = inPoseWindow(fr.pos, f.active);
        e.inWindow = near;
        let ahead = false;
        if (!inView && !near) {
          // beyond the window the pose starts before the mover is seen, with a look-ahead around the draw sphere
          const r = s.radius;
          s.radius = r + POSE_LOOKAHEAD_M;
          ahead = f.frustum.intersectsSphere(s);
          s.radius = r;
        }
        if (inView || near || ahead) e.lastWanted = f.nowMs;
        // ... and ends a second after the mover left all three, so panning across an edge does not toggle SET_VISIBLE
        wantPose =
          inView || near || ahead || (e.simVisible && f.nowMs - e.lastWanted <= VISIBLE_GRACE_MS);
        wantDraw = inView;
        if (!inView) e.verdict = 'culled';
      }
    } else {
      e.verdict = verdict.verdict;
    }
    // the visual (created on first need) and its appearance
    if (wantPose && !visual) visual = this.createVisual(e, vis, isActive);
    if (visual) {
      const hash = visualHash(vis);
      if (hash !== e.hash) {
        e.hash = hash;
        visual.setState(vis);
        this.d.onAppearanceChanged?.(id);
      }
    }
    const attach = visual?.currentPlan
      ? wantsAttach(visual.currentPlan.elements, vis.rideItem, vis.moverIndex)
      : false;
    this.syncSim(e, wantPose, wantPose && attach);
    if (!wantDraw || !visual) {
      if (visual) visual.root.visible = false;
      this.count(st, e.verdict);
      return;
    }
    // draw
    this.adjustWorld(e, vis, visual, fr);
    if (visual.complete && e.completeAt === 0) e.completeAt = f.nowMs;
    // the highlighted mover is drawn on its own: its additive twins repeat exactly the draw of its meshes (equal depth, LEQUAL)
    const instancer = id !== this.highlight ? this.d.instancer : undefined;
    const batched = instancer !== undefined;
    const shown = visual.update({
      nowMs: f.nowMs,
      objectDetail: f.opts.objectDetail,
      distCamera: e.dist,
      pose: fr,
      decor: this.rideView,
      // the part nodes' matrices are only needed by the meshes that do not go to a batch
      deferWorld: batched,
    });
    if (!shown) {
      e.verdict = 'noPose';
      this.count(st, 'noPose');
      return;
    }
    e.poseSeen = true;
    // a mover appears when everything it needs is resident (no half-dressed pop-in); the own mover is the loading gate's
    if (!rec.isSelf && e.completeAt === 0) {
      visual.root.visible = false;
      e.verdict = 'loading';
      this.count(st, 'loading');
      return;
    }
    // meshes the instancer takes over are not draws of their own (the batches are counted once, after the loop)
    let delegated = 0;
    if (instancer) {
      delegated = instancer.submit(visual, fr.world, fr.paletteBase);
      // weapons, blended blocks and the like still draw through the scene graph: they need their matrices
      if (delegated < instancer.lastTotal) visual.refreshWorld();
    }
    st.drawCalls += visual.drawCalls() - delegated;
    e.drawn = true;
    e.verdict = 'draw';
    st.drawn++;
    this.drawnList.push(e);
  }

  private count(st: MoverBatchStats, why: EntryVerdict): void {
    st.hidden[why] = (st.hidden[why] ?? 0) + 1;
  }

  private newEntry(id: number, rec: EntityViewRecord): MoverEntry {
    return {
      id,
      rec,
      visual: null,
      hash: 0,
      simVisible: false,
      simAttach: false,
      drawn: false,
      verdict: 'noPose',
      pos: [0, 0, 0],
      drawPos: [0, 0, 0],
      angle: 0,
      radius: 0,
      currentHeight: 0,
      dist: 0,
      flags: 0,
      pet: false,
      decorated: true,
      rank: RANK_NORMAL,
      ai: AII_NONE,
      poseSeen: false,
      completeAt: 0,
      shake: 0,
      shakeX: 0,
      shakeZ: 0,
      inWindow: false,
      lastWanted: 0,
      seen: 0,
    };
  }

  private createVisual(e: MoverEntry, vis: MoverVisualState, active: boolean): MoverVisual {
    const d = this.d;
    const visual = new MoverVisual(e.id, vis, {
      pose: d.pose,
      parent: d.parent,
      host: d.host,
      models: d.models,
      registry: d.registry,
      ...(d.rides ? { rides: d.rides } : {}),
      ...(d.lighting !== undefined ? { lighting: d.lighting } : {}),
      ...(active ? { activeMover: true } : {}),
      texturePrio: active ? Prio.Critical : Prio.Visible,
      rand: this.rand,
    });
    e.visual = visual;
    e.hash = visualHash(vis);
    e.poseSeen = false;
    e.completeAt = 0;
    this.stats.created++;
    return visual;
  }

  /** `SET_VISIBLE` only on change (Plan/41 12.8). */
  private syncSim(e: MoverEntry, visible: boolean, attach: boolean): void {
    if (e.simVisible === visible && (!visible || e.simAttach === attach)) return;
    e.simVisible = visible;
    e.simAttach = visible ? attach : false;
    this.stats.visibleSent++;
    this.d.sim.setVisible(e.id, visible, e.simAttach);
  }

  /** Hit shake and the special-case models: adjusts the world matrix the visual is about to use (three.js space). */
  private adjustWorld(
    e: MoverEntry,
    vis: MoverVisualState,
    visual: MoverVisual,
    fr: MoverPoseFrame,
  ): void {
    const el = fr.world.elements;
    if (ignoresServerScale(vis.moverIndex) && vis.scale > 0 && vis.scale !== 1) {
      // MI_MASA_TINA: the model default scale (1) is reset every frame
      const k = 1 / vis.scale;
      for (let i = 0; i < 12; i++) if (i % 4 !== 3) el[i] = (el[i] ?? 0) * k;
    }
    if (isSyliaca(vis.moverIndex) && !visual.bounds.isEmpty()) {
      _box.copy(visual.bounds);
      el[13] += syliacaLift(_box.min.y, _box.max.y);
    }
    if (e.shake > 0 && shakes(e.shake, vis.kind === 'player', e.rank)) {
      el[12] += e.shakeX;
      el[14] -= e.shakeZ; // LH z -> three.js -z
    }
  }

  /** The mover is not drawn this frame: the visual keeps its resources but hides. */
  private hideAll(): void {
    for (const e of this.entries.values()) {
      e.drawn = false;
      if (e.visual) e.visual.root.visible = false;
    }
  }

  private release(e: MoverEntry): void {
    if (!e.visual) return;
    e.visual.dispose();
    e.visual = null;
    e.hash = 0;
    e.poseSeen = false;
    e.completeAt = 0;
    this.stats.disposed++;
  }

  /** The entity is gone: its visual goes (the sim removes the mover with `REMOVE_MOVER`). */
  private drop(e: MoverEntry): void {
    this.release(e);
    this.entries.delete(e.id);
  }

  private countVisuals(): number {
    let n = 0;
    for (const e of this.entries.values()) if (e.visual) n++;
    return n;
  }

  // ---- cosmetic ticks ---------------------------------------------------------------------------------------

  /** `steps` 60 Hz cosmetic steps: the hit shake counts down and re-rolls its offset per tick (FIX of the render-rate roll). */
  tick(steps: number): void {
    if (steps <= 0) return;
    for (const e of this.entries.values()) {
      if (e.shake <= 0) continue;
      e.shake = Math.max(0, e.shake - steps);
      if (e.shake > 0) {
        const [x, z] = shakeOffset(this.rand);
        e.shakeX = x;
        e.shakeZ = z;
      } else {
        e.shakeX = 0;
        e.shakeZ = 0;
      }
    }
  }

  /** `fx.hitShake(id)`: `dmgCnt = 10` (Plan/41 12.8). */
  hitShake(id: number): void {
    const e = this.entries.get(id);
    if (!e) return;
    e.shake = HIT_SHAKE_TICKS;
    const [x, z] = shakeOffset(this.rand);
    e.shakeX = x;
    e.shakeZ = z;
  }

  // ---- queries ----------------------------------------------------------------------------------------------

  /** The mover under the cursor (`setHighlight`, Plan/41 12.9); `0` clears. */
  setHighlight(id: number): void {
    this.highlight = id;
  }

  get highlighted(): number {
    return this.highlight;
  }

  /** Counts the drawn frames (a cache key for the per-frame lists of the pick support). */
  get frameNumber(): number {
    return this.frameNo;
  }

  /** The own character's object id of the last frame (`0` = none). */
  get activeId(): number {
    return this.selfId;
  }

  entry(id: number): MoverEntry | undefined {
    return this.entries.get(id);
  }

  visualOf(id: number): MoverVisual | null {
    return this.entries.get(id)?.visual ?? null;
  }

  /** Calls `fn` for the movers drawn in the last `update`. */
  forEachDrawn(fn: (e: MoverEntry, visual: MoverVisual) => void): void {
    for (const e of this.drawnList) if (e.visual) fn(e, e.visual);
  }

  get drawn(): readonly MoverEntry[] {
    return this.drawnList;
  }

  /**
   * The loading gate (Plan/40 9.8): the own mover's visual is complete (skeleton, every part and texture resident) and a pose
   * was shown; a complete visual that never gets a pose (its skeleton or clip does not exist, KQ-02/30) counts after a short
   * grace so that a missing file never holds the gate.
   */
  ownModelReady(id: number, nowMs: number = performance.now()): boolean {
    const e = this.entries.get(id);
    if (!e?.visual?.complete) return false;
    if (e.poseSeen) return true;
    return e.completeAt !== 0 && nowMs - e.completeAt >= this.poseGraceMs;
  }

  /** Distance-based LOD group a mover would use now (probe). */
  lodOf(id: number, objectDetail: 0 | 1 | 2): number {
    const e = this.entries.get(id);
    return e ? lodFor(e.dist, objectDetail, e.rec.isSelf) : 0;
  }

  /** The probe section `render.movers`. */
  probe(): Record<string, unknown> {
    const rows = [...this.entries.values()].map((e) => ({
      id: e.id,
      model: e.rec.visual?.moverIndex ?? 0,
      scale: e.rec.visual?.scale ?? 1,
      name: e.rec.visual?.name ?? '',
      drawn: e.drawn,
      verdict: e.verdict,
      simVisible: e.simVisible,
      lod: e.visual?.lodGroup ?? -1,
      complete: e.visual?.complete ?? false,
      // the part slots of the visual that are resident (merged ones included): server specs assert the world mover's parts
      parts: e.visual?.loadedSlots() ?? [],
      // the ride or the wing of a flying mover (WP-REND-25): its kind and whether it is on the screen
      ride: e.visual?.rides.instance
        ? { kind: e.visual.rides.rideKind, shown: e.visual.rides.instance.root.visible }
        : null,
      dist: Math.round(e.dist * 10) / 10,
      ...(e.visual
        ? this.motionOf(e.id)
        : { motion: 0, resolved: 0, stateFlags: 0, frame: 0, radius: 0 }),
    }));
    return { ...this.stats, rows };
  }

  private motionOf(id: number): {
    motion: number;
    resolved: number;
    stateFlags: number;
    frame: number;
    radius: number;
  } {
    const fr = newPoseFrame();
    return this.d.pose.read(id, fr)
      ? {
          motion: fr.motion,
          resolved: fr.motionResolved,
          stateFlags: fr.stateFlags,
          frame: Math.round(fr.frameCur * 100) / 100,
          radius: Math.round(fr.radius * 100) / 100,
        }
      : { motion: 0, resolved: 0, stateFlags: 0, frame: 0, radius: 0 };
  }

  /** Forgets every mover (a world change or leaving the world): the sim state of the old world is gone too. */
  reset(): void {
    for (const e of [...this.entries.values()]) this.drop(e);
    this.drawnList.length = 0;
    this.highlight = 0;
    this.stats.drawn = 0;
    this.stats.visuals = 0;
  }

  dispose(): void {
    if (this.disposed) return;
    this.reset();
    this.disposed = true;
  }
}
