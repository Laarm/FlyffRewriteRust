// The geometry of the SFX parts (Plan/41 section 16.4, ref/20 sections 4.3-4.6; sfxbase.cpp:196-418, 1072-1577, 2507-2697).
//
// Every part is evaluated in the left-handed D3D convention with row vectors; the finished world matrix goes to a `PartSink`,
// which maps it into three.js space once (`z` negated). A quad is a 1 x 1 unit rectangle in its local XY plane, so only the
// first two rows of the 3x3 part of the matrix and the translation matter (the third row multiplies z = 0).
//
//   BILL part      O = Scale(S3 * k.scale) * { Rz(k.rot.z) * InvViewRot | Rz(k.rot.z - a) * Rx(90) | YPR(k.rot + (0, a, 0)) }
//                  o = k.pos * YPR(k.posRotate + (0, a, 0)), then o *= S3;              world = O * Translate(P + o)
//   PARTICLE part  the emitter keys give O (no scale), e = k.pos * YPR(k.posRotate + (0, a, 0)) (NOT scaled by S3); per particle
//                  q = (p.pos * YPR(k.rot + k.posRotate + (0, a, 0))) * S3;   world = YPR(spin) * Scale(p.scale * S3) * O * Translate(e + q + P)
//                  spin = lerp(0, p.rotation, t) with t the fraction of the EMITTER key segment (0 when the segment is a point)
//   CUSTOMMESH     the ring of `customMesh.ts` oriented by { Rz(k.rot.z - a) * InvViewRot | Rz(k.rot.z) * Rx(90) | Rz(k.rot.z) * InvViewRot | YPR(k.rot + (0, a, 0)) }
//   MESH           Scale(S3 * k.scale) * YPR(k.rot + (0, a, 0)) * Translate(P + (k.pos * YPR(k.posRotate + (0, a, 0))) * S3)
//
// `a` is the object yaw (`m_vRotate.y`, the yaw-only path). The 3-axis path (`Render2`, directional projectiles) takes a full
// rotation vector `A`: BILL BOTTOM uses `Rz(k.rot.z - A.z)`, NORMAL `Scale * YPR(k.rot) * YPR(A)`, the offset `k.pos * (YPR(A) * YPR(k.posRotate))`;
// particles NORMAL `YPR(k.rot) * YPR(A)`, `e = k.pos * (YPR(A) * YPR(k.posRotate))`, `q = p.pos * (YPR(k.rot + k.posRotate) * YPR(A))`.

import { RING_STRIDE, buildRing, ringVertexCount } from './customMesh';
import { newSampledKey, particleAlpha, sampleKey, textureFrameIndex } from './keys';
import {
  RAD,
  type Aff,
  mul,
  newAff,
  setIdentity,
  setRotX,
  setRotZ,
  setScale,
  setYpr,
  setYprDeg,
  transform,
} from './mat';
import { BILL_TYPE, type PartProto } from './proto';
import type { SfxModel } from './SfxModel';

/** What the geometry code needs to know about the frame and the textures. */
export interface EmitEnv {
  /** `g_matInvView`: the inverse view rotation (translation zeroed) in LH row-vector form. */
  readonly invViewRot: Aff;
  /**
   * The texture id of a canonical path, or `-1` when the part must not be drawn now (the texture is still loading, failed to load
   * or does not exist: KQ-30, nothing is drawn).
   */
  textureId(path: string): number;
}

/** The consumer of the parts of an effect, in draw order (the original's draw order: parts in file order, particles in list order). */
export interface PartSink {
  /**
   * One textured unit quad. The arguments are rows 0 and 1 and the translation row of the LH world matrix (`v * M`), and the
   * wrapped alpha byte 0..255.
   */
  quad(
    tex: number,
    additive: boolean,
    r0x: number,
    r0y: number,
    r0z: number,
    r1x: number,
    r1y: number,
    r1z: number,
    tx: number,
    ty: number,
    tz: number,
    alpha: number,
  ): void;
  /** A custom-mesh strip: `count` vertices of `x y z u v` (LH world space), drawn twice (inner faces first, then the outer ones). */
  strip(tex: number, additive: boolean, vertices: Float64Array, count: number, alpha: number): void;
  /** A MESH part: the LH world matrix of the model, the key alpha byte. */
  mesh(model: SfxModel, part: PartProto, world: Aff, alpha: number): void;
}

const HALF_PI = (90 * Math.PI) / 180;

// scratch (the SFX update is single threaded and not re-entrant)
const K = newSampledKey();
const A0 = newAff();
const A1 = newAff();
const A2 = newAff();
const A3 = newAff();
const O = newAff();
const W = newAff();
const YP = newAff();
const YQ = newAff();
const SC = newAff();
const MESHW = newAff();
const VEC: [number, number, number] = [0, 0, 0];
const RING = new Float64Array(ringVertexCount(64) * RING_STRIDE);
const STRIP = new Float64Array(ringVertexCount(64) * RING_STRIDE);

/**
 * The orientation `O` of a facing part, WITHOUT the scale: BILL/POLE `Rz(rz) * InvViewRot`, BOTTOM `Rz(rz) * Rx(90)`. Returns
 * `false` for NORMAL (the caller builds the YPR orientation).
 */
function facing(o: Aff, billType: number, rzDeg: number, env: EmitEnv): boolean {
  if (billType === BILL_TYPE.normal) return false;
  setRotZ(A0, rzDeg * RAD);
  if (billType === BILL_TYPE.bottom) {
    setRotX(A1, HALF_PI);
    mul(o, A0, A1);
  } else {
    mul(o, A0, env.invViewRot);
  }
  return true;
}

/** The NORMAL orientation: `YPR(rot + (0, a, 0))` (yaw-only) or `YPR(rot) * YPR(A)` (3-axis). */
function normalOrientation(
  o: Aff,
  rot: readonly number[],
  a: number,
  rot3: readonly number[] | null,
): void {
  if (rot3 === null) {
    setYprDeg(o, rot[0] ?? 0, (rot[1] ?? 0) + a, rot[2] ?? 0);
  } else {
    setYprDeg(A2, rot[0] ?? 0, rot[1] ?? 0, rot[2] ?? 0);
    setYprDeg(A3, rot3[0] ?? 0, rot3[1] ?? 0, rot3[2] ?? 0);
    mul(o, A2, A3);
  }
}

/** `VEC = k.pos * R` with `R = YPR(posRotate + (0, a, 0))` (yaw-only) or `YPR(A) * YPR(posRotate)` (3-axis). */
function keyOffset(
  pos: readonly number[],
  posRotate: readonly number[],
  a: number,
  rot3: readonly number[] | null,
): void {
  if (rot3 === null) {
    setYprDeg(YP, posRotate[0] ?? 0, (posRotate[1] ?? 0) + a, posRotate[2] ?? 0);
  } else {
    setYprDeg(YQ, rot3[0] ?? 0, rot3[1] ?? 0, rot3[2] ?? 0);
    setYprDeg(YP, posRotate[0] ?? 0, posRotate[1] ?? 0, posRotate[2] ?? 0);
    mul(SC, YQ, YP);
    YP.set(SC);
  }
  transform(VEC, pos[0] ?? 0, pos[1] ?? 0, pos[2] ?? 0, YP);
}

/** The texture of a plain or texture-animated part at animation frame index `n` (`-1` = not drawable now). */
function partTexture(part: PartProto, n: number, env: EmitEnv): number {
  const path = part.texturePaths[part.texFrame > 1 ? n : 0];
  return path === undefined ? -1 : env.textureId(path);
}

/** The animation frame of a texture-animated part (`-1`: the part is not drawn yet, `f < first`). */
function animFrame(part: PartProto, f: number): number {
  if (part.texFrame <= 1) return 0;
  if (f < part.first) return -1;
  return textureFrameIndex(part.texFrame, f - part.first, part.texLoop);
}

/** One BILL part at the model's current frame. */
export function emitBill(
  model: SfxModel,
  part: PartProto,
  env: EmitEnv,
  sink: PartSink,
  rot3: readonly number[] | null,
): void {
  const f = model.cur;
  const animIndex = animFrame(part, f);
  if (animIndex < 0) return;
  if (!sampleKey(part.keys, f, K)) return;
  const tex = partTexture(part, animIndex, env);
  if (tex < 0) return;
  const s3 = model.scale;
  const a = model.rotate[1];
  const sx = s3[0] * K.scale[0];
  const sy = s3[1] * K.scale[1];
  const rz =
    part.billType === BILL_TYPE.bottom ? K.rotate[2] - (rot3 ? (rot3[2] ?? 0) : a) : K.rotate[2];
  if (!facing(O, part.billType, rz, env)) normalOrientation(O, K.rotate, a, rot3);
  keyOffset(K.pos, K.posRotate, a, rot3);
  const tx = model.pos[0] + VEC[0] * s3[0];
  const ty = model.pos[1] + VEC[1] * s3[1];
  const tz = model.pos[2] + VEC[2] * s3[2];
  sink.quad(
    tex,
    part.additive,
    sx * (O[0] ?? 0),
    sx * (O[1] ?? 0),
    sx * (O[2] ?? 0),
    sy * (O[3] ?? 0),
    sy * (O[4] ?? 0),
    sy * (O[5] ?? 0),
    tx,
    ty,
    tz,
    K.alpha & 0xff,
  );
}

/** One PARTICLE part: every live particle of the part as a quad. */
export function emitParticles(
  model: SfxModel,
  part: PartProto,
  env: EmitEnv,
  sink: PartSink,
  rot3: readonly number[] | null,
): void {
  const list = model.lists[part.index];
  const p = part.particle;
  if (!list || !p || list.n === 0) return;
  const f = model.cur;
  if (!sampleKey(part.keys, f, K, p.repeat_scale === 0)) return;
  const a = model.rotate[1];
  const s3 = model.scale;
  // the emitter orientation (no scale); BOTTOM subtracts the yaw only on the yaw-only path
  const rz = part.billType === BILL_TYPE.bottom && rot3 === null ? K.rotate[2] - a : K.rotate[2];
  if (!facing(O, part.billType, rz, env)) normalOrientation(O, K.rotate, a, rot3);
  keyOffset(K.pos, K.posRotate, a, rot3);
  const px = model.pos[0] + VEC[0];
  const py = model.pos[1] + VEC[1];
  const pz = model.pos[2] + VEC[2];
  // the particle offset rotation: q = p.pos * YPR(k.rot + k.posRotate + (0, a, 0)) or * (YPR(k.rot + k.posRotate) * YPR(A))
  if (rot3 === null) {
    setYprDeg(
      YQ,
      K.rotate[0] + K.posRotate[0],
      K.rotate[1] + K.posRotate[1] + a,
      K.rotate[2] + K.posRotate[2],
    );
  } else {
    setYprDeg(
      A2,
      K.rotate[0] + K.posRotate[0],
      K.rotate[1] + K.posRotate[1],
      K.rotate[2] + K.posRotate[2],
    );
    setYprDeg(A3, rot3[0] ?? 0, rot3[1] ?? 0, rot3[2] ?? 0);
    mul(YQ, A2, A3);
  }
  const spinT = K.t;
  const keyAlpha = K.alpha;
  const o0 = O[0] ?? 0;
  const o1 = O[1] ?? 0;
  const o2 = O[2] ?? 0;
  const o3 = O[3] ?? 0;
  const o4 = O[4] ?? 0;
  const o5 = O[5] ?? 0;
  const additive = part.additive;
  for (let i = 0; i < list.n; i++) {
    const i3 = i * 3;
    const age = list.age[i] ?? 0;
    const tex = partTexture(part, textureFrameIndex(part.texFrame, age, part.texLoop), env);
    if (tex < 0) continue;
    // q = p.pos * R, then * S3
    transform(VEC, list.pos[i3] ?? 0, list.pos[i3 + 1] ?? 0, list.pos[i3 + 2] ?? 0, YQ);
    const tx = px + VEC[0] * s3[0];
    const ty = py + VEC[1] * s3[1];
    const tz = pz + VEC[2] * s3[2];
    const sx = (list.scale[i3] ?? 0) * s3[0];
    const sy = (list.scale[i3 + 1] ?? 0) * s3[1];
    const alpha = particleAlpha(keyAlpha, age, p.appear, p.keep, p.disappear) & 0xff;
    const rx = (list.rotation[i3] ?? 0) * spinT;
    const ry = (list.rotation[i3 + 1] ?? 0) * spinT;
    const rzSpin = (list.rotation[i3 + 2] ?? 0) * spinT;
    if (rx === 0 && ry === 0 && rzSpin === 0) {
      // world = Scale(sx, sy, .) * O * T: rows 0 and 1 of O scaled
      sink.quad(
        tex,
        additive,
        sx * o0,
        sx * o1,
        sx * o2,
        sy * o3,
        sy * o4,
        sy * o5,
        tx,
        ty,
        tz,
        alpha,
      );
    } else {
      // world = YPR(spin) * Scale(sx, sy, sz) * O * T
      const sz = (list.scale[i3 + 2] ?? 0) * s3[2];
      setScale(SC, sx, sy, sz);
      mul(A0, SC, O);
      setYprDeg(A1, rx, ry, rzSpin);
      mul(W, A1, A0);
      sink.quad(
        tex,
        additive,
        W[0] ?? 0,
        W[1] ?? 0,
        W[2] ?? 0,
        W[3] ?? 0,
        W[4] ?? 0,
        W[5] ?? 0,
        tx,
        ty,
        tz,
        alpha,
      );
    }
  }
}

/** One CUSTOMMESH part: the ring strip in LH world space (the two draws are the sink's business). */
export function emitCustomMesh(
  model: SfxModel,
  part: PartProto,
  env: EmitEnv,
  sink: PartSink,
  rot3: readonly number[] | null,
): void {
  const f = model.cur;
  const animIndex = animFrame(part, f);
  if (animIndex < 0) return;
  if (!sampleKey(part.keys, f, K)) return;
  const tex = partTexture(part, animIndex, env);
  if (tex < 0) return;
  const s3 = model.scale;
  const a = model.rotate[1];
  const points = Math.min(64, Math.max(1, part.points));
  // orientation (no scale: the ring is scaled by the key and S3 in its vertices)
  switch (part.billType) {
    case BILL_TYPE.bill:
      facing(O, BILL_TYPE.bill, K.rotate[2] - (rot3 ? (rot3[2] ?? 0) : a), env);
      break;
    case BILL_TYPE.bottom:
    case BILL_TYPE.pole:
      facing(O, part.billType, K.rotate[2], env);
      break;
    case BILL_TYPE.normal:
      normalOrientation(O, K.rotate, a, rot3);
      break;
  }
  // the offset: key.pos rotated by RotY(a) (yaw-only) or YPR(A), NOT by pos_rotate, then * S3
  if (rot3 === null) setYpr(YP, a * RAD, 0, 0);
  else setYprDeg(YP, rot3[0] ?? 0, rot3[1] ?? 0, rot3[2] ?? 0);
  transform(VEC, K.pos[0], K.pos[1], K.pos[2], YP);
  const tx = model.pos[0] + VEC[0] * s3[0];
  const ty = model.pos[1] + VEC[1] * s3[1];
  const tz = model.pos[2] + VEC[2] * s3[2];
  const count = buildRing(RING, {
    points,
    scale: K.scale,
    posRotate: K.posRotate,
    s3,
    topVBias: rot3 === null ? 0.02 : 0.1,
  });
  for (let i = 0; i < count; i++) {
    const at = i * RING_STRIDE;
    transform(VEC, RING[at] ?? 0, RING[at + 1] ?? 0, RING[at + 2] ?? 0, O);
    STRIP[at] = VEC[0] + tx;
    STRIP[at + 1] = VEC[1] + ty;
    STRIP[at + 2] = VEC[2] + tz;
    STRIP[at + 3] = RING[at + 3] ?? 0;
    STRIP[at + 4] = RING[at + 4] ?? 0;
  }
  sink.strip(tex, part.additive, STRIP, count, K.alpha & 0xff);
}

/** One MESH part: the LH world matrix `Scale(S3 * k.scale) * YPR(k.rot + (0, a, 0)) * Translate(...)` and the key alpha byte. */
export function emitMesh(model: SfxModel, part: PartProto, sink: PartSink): void {
  const f = model.cur;
  if (!sampleKey(part.keys, f, K)) return;
  const s3 = model.scale;
  const a = model.rotate[1];
  setScale(SC, s3[0] * K.scale[0], s3[1] * K.scale[1], s3[2] * K.scale[2]);
  setYprDeg(A0, K.rotate[0], K.rotate[1] + a, K.rotate[2]);
  mul(MESHW, SC, A0);
  keyOffset(K.pos, K.posRotate, a, null);
  MESHW[9] = model.pos[0] + VEC[0] * s3[0];
  MESHW[10] = model.pos[1] + VEC[1] * s3[1];
  MESHW[11] = model.pos[2] + VEC[2] * s3[2];
  sink.mesh(model, part, MESHW, K.alpha & 0xff);
}

/**
 * All parts of a model in file order into `sink`. `rot3` is the 3-axis rotation vector of a directional effect (`Render2`),
 * `null` for the yaw-only path.
 */
export function emitModel(
  model: SfxModel,
  env: EmitEnv,
  sink: PartSink,
  rot3: readonly number[] | null = null,
): void {
  for (const part of model.proto.parts) {
    if (!part.inUse) continue;
    switch (part.kind) {
      case 'bill':
        emitBill(model, part, env, sink, rot3);
        break;
      case 'particle':
        emitParticles(model, part, env, sink, rot3);
        break;
      case 'custom_mesh':
        emitCustomMesh(model, part, env, sink, rot3);
        break;
      case 'mesh':
        emitMesh(model, part, sink);
        break;
    }
  }
}

/** The identity `InvViewRot` (tests, dev pages without a camera). */
export function identityInvView(): Aff {
  return setIdentity(newAff());
}
