// CUSTOMMESH parts: the procedural cylinder / cone (Plan/41 section 16.4, ref/20 section 4.5; sfxbase.cpp:1303-1577).
//
// The key fields are re-purposed: `scale.x` = radius of the top ring, `scale.y` = height, `scale.z` = radius of the bottom ring;
// `pos_rotate.x` = U tiling, `.y` = top V, `.z` = bottom V. The ring has `N = points` (default 20), `seg = 2N` segments and
//
//   step = 360 / seg      INTEGER division (KEEP-QUIRK): N = 7 gives 25 degrees, 2N * 25 = 350, the ring does not close
//   theta_i = step * i    degrees, i = 0 .. seg
//   top_i = (sin theta * r_top, h, cos theta * r_top) * S3        bot_i = (sin theta * r_bot, 0, cos theta * r_bot) * S3
//   v_top = pos_rotate.y + 0.02 (Render2, the 3-axis path of directional projectiles: + 0.1)   v_bot = pos_rotate.z
//   u = pos_rotate.x == 0 ? (i even ? 0 : 1) : pos_rotate.x * i / seg
//
// The strip is `top_0, bot_0, top_1, bot_1, ...` (4N triangles) and the original draws it twice, with the culling set to CW and
// then to CCW (the inner faces first, then the outer ones), depth write off.

/** Floats per ring vertex: `x y z u v`. */
export const RING_STRIDE = 5;

/** The number of strip vertices of a ring of `points` (`(2N + 1) * 2`). */
export const ringVertexCount = (points: number): number =>
  (Math.max(1, Math.trunc(points)) * 2 + 1) * 2;

/** `360 / (N * 2)` with C integer division. */
export const ringStepDeg = (points: number): number =>
  Math.trunc(360 / (Math.max(1, Math.trunc(points)) * 2));

/** The inputs of one ring (a sampled key and the instance scale). */
export interface RingParams {
  /** `points` of the part. */
  readonly points: number;
  /** `key.scale`: top radius, height, bottom radius. */
  readonly scale: readonly [number, number, number];
  /** `key.pos_rotate`: U tiling, top V, bottom V. */
  readonly posRotate: readonly [number, number, number];
  /** The instance scale `S3`. */
  readonly s3: readonly [number, number, number];
  /** The V offset of the top ring: 0.02 (`Render`) or 0.1 (`Render2`). */
  readonly topVBias: number;
}

/**
 * Writes the strip vertices (`x y z u v` each, local space before the part orientation) into `out` and returns the vertex count.
 * `out` must hold `ringVertexCount(points) * RING_STRIDE` numbers.
 */
export function buildRing(out: Float32Array | Float64Array | number[], p: RingParams): number {
  const n = Math.max(1, Math.trunc(p.points));
  const seg = n * 2;
  const step = Math.trunc(360 / seg);
  const [rt, h, rb] = p.scale;
  const [sx, sy, sz] = p.s3;
  const tile = p.posRotate[0];
  const vTop = p.posRotate[1] + p.topVBias;
  const vBot = p.posRotate[2];
  let w = 0;
  for (let i = 0; i <= seg; i++) {
    const rad = (step * i * Math.PI) / 180;
    const s = Math.sin(rad);
    const c = Math.cos(rad);
    const u = tile === 0 ? (i % 2 === 0 ? 0 : 1) : (tile * i) / seg;
    out[w++] = s * rt * sx;
    out[w++] = h * sy;
    out[w++] = c * rt * sz;
    out[w++] = u;
    out[w++] = vTop;
    out[w++] = s * rb * sx;
    out[w++] = 0;
    out[w++] = c * rb * sz;
    out[w++] = u;
    out[w++] = vBot;
  }
  return seg * 2 + 2;
}
