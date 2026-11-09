// Geometry of the sky (Plan/41 section 14.2; ref/19 section 18; SkyBox.cpp `SetVertices` 1096-1271). Pure data in LEFT-HANDED
// world space (+Y up, +Z north): the renderer mirrors `z` once when it builds the three.js geometry.
//
//   cylinder  radius 110, 28 segments. Per segment two bands of two triangles: upper `y 150 -> 0` with `v 0 -> 0.9` and vertex
//             alpha `0 -> 255`, lower `y 0 -> -20` with `v 0.9 -> 1.0` and alpha `255 -> 0`; `u = i / 28`. 12 vertices per
//             segment, a plain triangle list (the original draws 116 triangles from a 348 vertex buffer of which only the first
//             336 are written; the 4 extra triangles read uninitialised memory and are not reproduced).
//   dome      a triangle fan: the centre `(0, 100, 0)` with alpha 255 and 29 rim vertices at `y = 40`, radius 110, alpha 0 (the
//             29th repeats the 1st); `uv = ((x + 110) / 110, (z + 110) / 110)`, the centre `(1, 1)`.
//
// Colour is the texture, alpha is `vertexAlpha * layerWeight` (the schedule of `schedule.ts`).

/** `radius` of `SetVertices`. */
export const SKY_RADIUS = 110;
/** Segments of the cylinder. */
export const SKY_SEGMENTS = 28;
/** Top and bottom of the cylinder and the `v` of the horizon (`y = 0`). */
export const SKY_TOP = 150;
export const SKY_BOTTOM = -20;
export const SKY_HORIZON_V = 0.9;
/** The dome: centre height and rim height. */
export const DOME_CENTRE_Y = 100;
export const DOME_RIM_Y = 40;
/** Rim vertices of the dome fan (the last one repeats the first). */
export const DOME_RIM_VERTICES = 29;

/** One sky mesh: positions (LH), uvs, vertex alpha 0..1 and an optional index list (the fan is indexed). */
export interface SkyMeshData {
  /** `x y z` per vertex, left-handed world space. */
  readonly positions: Float32Array;
  /** `u v` per vertex. */
  readonly uvs: Float32Array;
  /** Vertex alpha, `0..1` (the original's `D3DCOLOR_ARGB(a, 255, 255, 255)` alpha byte / 255). */
  readonly alphas: Float32Array;
  /** Triangle indices; `null` for a plain triangle list. */
  readonly indices: Uint16Array | null;
  readonly vertexCount: number;
  readonly triangleCount: number;
}

const TWO_PI = 2 * Math.PI;

/** The 336 vertices (112 triangles) of the sky cylinder. */
export function buildCylinder(): SkyMeshData {
  const n = SKY_SEGMENTS * 12;
  const positions = new Float32Array(n * 3);
  const uvs = new Float32Array(n * 2);
  const alphas = new Float32Array(n);
  let v = 0;
  const put = (x: number, y: number, z: number, u: number, tv: number, a: number): void => {
    positions[v * 3] = x;
    positions[v * 3 + 1] = y;
    positions[v * 3 + 2] = z;
    uvs[v * 2] = u;
    uvs[v * 2 + 1] = tv;
    alphas[v] = a;
    v++;
  };
  for (let i = 0; i < SKY_SEGMENTS; i++) {
    const x1 = Math.sin((TWO_PI / SKY_SEGMENTS) * i) * SKY_RADIUS;
    const z1 = Math.cos((TWO_PI / SKY_SEGMENTS) * i) * SKY_RADIUS;
    const x2 = Math.sin((TWO_PI / SKY_SEGMENTS) * (i + 1)) * SKY_RADIUS;
    const z2 = Math.cos((TWO_PI / SKY_SEGMENTS) * (i + 1)) * SKY_RADIUS;
    const u1 = (1 / SKY_SEGMENTS) * i;
    const u2 = (1 / SKY_SEGMENTS) * (i + 1);
    // upper band: (x1,150) (x1,0) (x2,150) | (x2,150) (x1,0) (x2,0)
    put(x1, SKY_TOP, z1, u1, 0, 0);
    put(x1, 0, z1, u1, SKY_HORIZON_V, 1);
    put(x2, SKY_TOP, z2, u2, 0, 0);
    put(x2, SKY_TOP, z2, u2, 0, 0);
    put(x1, 0, z1, u1, SKY_HORIZON_V, 1);
    put(x2, 0, z2, u2, SKY_HORIZON_V, 1);
    // lower band: (x1,0) (x1,-20) (x2,0) | (x2,0) (x1,-20) (x2,-20)
    put(x1, 0, z1, u1, SKY_HORIZON_V, 1);
    put(x1, SKY_BOTTOM, z1, u1, 1, 0);
    put(x2, 0, z2, u2, SKY_HORIZON_V, 1);
    put(x2, 0, z2, u2, SKY_HORIZON_V, 1);
    put(x1, SKY_BOTTOM, z1, u1, 1, 0);
    put(x2, SKY_BOTTOM, z2, u2, 1, 0);
  }
  return { positions, uvs, alphas, indices: null, vertexCount: n, triangleCount: n / 3 };
}

/** The 30 vertices and 28 fan triangles of the cloud dome (the scroll offset is applied by the material). */
export function buildDome(): SkyMeshData {
  const n = 1 + DOME_RIM_VERTICES;
  const positions = new Float32Array(n * 3);
  const uvs = new Float32Array(n * 2);
  const alphas = new Float32Array(n);
  positions.set([0, DOME_CENTRE_Y, 0], 0);
  uvs.set([1, 1], 0);
  alphas[0] = 1;
  for (let i = 1; i <= DOME_RIM_VERTICES; i++) {
    const x = Math.sin((TWO_PI / SKY_SEGMENTS) * i) * SKY_RADIUS;
    const z = Math.cos((TWO_PI / SKY_SEGMENTS) * i) * SKY_RADIUS;
    positions.set([x, DOME_RIM_Y, z], i * 3);
    uvs.set([(x + SKY_RADIUS) / SKY_RADIUS, (z + SKY_RADIUS) / SKY_RADIUS], i * 2);
    alphas[i] = 0;
  }
  const triangles = DOME_RIM_VERTICES - 1;
  const indices = new Uint16Array(triangles * 3);
  for (let t = 0; t < triangles; t++) indices.set([0, t + 1, t + 2], t * 3);
  return { positions, uvs, alphas, indices, vertexCount: n, triangleCount: triangles };
}

/** Quad texture coordinates in the order top-left, top-right, bottom-right, bottom-left (the `D3DSUNVERTEX` quads). */
export const QUAD_UVS: Float32Array = new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]);
/** The two triangles of a quad drawn as a fan `(0, 1, 2)`, `(0, 2, 3)`. */
export const QUAD_INDICES: Uint16Array = new Uint16Array([0, 1, 2, 0, 2, 3]);
