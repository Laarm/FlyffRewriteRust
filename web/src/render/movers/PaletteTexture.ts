// The shared bone-palette texture (Plan/41 section 12.6, R5; WP-REND-04): ONE `RGBA32F` texture of
// `PALETTE_TEX_WIDTH = 1536` texels per row (512 matrices of 3 texels), height grown in powers of two (initial
// 32 rows = 16 384 matrices). Matrix `m` occupies texels `3m .. 3m + 2` of the row-major linearisation; each texel is
// one row of the 3x4 transposed affine matrix in three.js space (Plan/31 section 13.4), so the skinning shader
// computes `p' = vec3(dot(row0, vec4(p, 1)), dot(row1, ..), dot(row2, ..))`.
//
// The palette is rebuilt every frame by the pose evaluation (wasm, `SimWorld.render_view`); the renderer uploads
// the rows `[0, ceil(matrices / 512))` straight from the view (`texSubImage2D`, no copy), so growing the texture
// never has to copy old texels. The `SimWorld` palette area is padded to whole rows (31 section 17.7 size rule), a
// `texSubImage2D` of `rows` full rows therefore never reads past the buffer.
//
// The texture is a raw WebGL texture wrapped in a three.js `ExternalTexture` (like the texture cache does), because
// the upload must be a sub-image upload of exactly the rows in use, and three.js's `DataTexture` re-uploads the
// whole image.

import { ExternalTexture } from 'three';
import { GL } from '../gl/glConsts';
import type { TextureStateLike } from '../gl/dds';

/** Texels per palette row (a multiple of 3: a matrix never straddles a row). */
export const PALETTE_TEX_WIDTH = 1536;
/** Matrices per palette row. */
export const MATRICES_PER_ROW = PALETTE_TEX_WIDTH / 3;
/** Floats per matrix (3 RGBA32F texels). */
export const FLOATS_PER_MATRIX = 12;
/** Floats per palette row. */
export const FLOATS_PER_ROW = PALETTE_TEX_WIDTH * 4;
/** Rows allocated at the start. */
export const INITIAL_PALETTE_ROWS = 32;
/** WebGL2 guarantees `MAX_TEXTURE_SIZE >= 2048`. */
export const MAX_PALETTE_ROWS = 2048;
/** Bytes per palette row. */
export const BYTES_PER_ROW = FLOATS_PER_ROW * 4;

/** `RGBA32F`, `RGBA`, `FLOAT` (WebGL2 enums; no extension is needed for NEAREST sampling of a float texture). */
const RGBA32F = 0x8814;
const FLOAT = 0x1406;

/** Rows that hold `matrices` palette matrices: `ceil(matrices / 512)`. */
export const rowsForMatrices = (matrices: number): number =>
  Math.ceil(Math.max(0, matrices) / MATRICES_PER_ROW);

/** Rows that hold `floats` palette floats (the `palette_rows` rule of the render view header). */
export const rowsForFloats = (floats: number): number =>
  Math.ceil(Math.max(0, floats) / FLOATS_PER_ROW);

/** The part of the renderer the palette needs (a `RenderDevice.textureHost` or a test double). */
export interface PaletteHost {
  getContext(): WebGL2RenderingContext;
  readonly state: TextureStateLike;
}

/** The smallest power of two `>= n` (1 for `n <= 1`). */
function nextPow2(n: number): number {
  let p = 1;
  while (p < n) p *= 2;
  return p;
}

/** Counters of one frame's upload (the probe and the S2 budget: palette bytes per frame <= 0.5 MB at 300 movers). */
export interface PaletteUploadStats {
  /** Rows uploaded by the last `upload`. */
  readonly rows: number;
  /** Bytes uploaded by the last `upload`. */
  readonly bytes: number;
  /** Matrices (12 floats) the last upload carried (rows x 512; the palette area is padded). */
  readonly matrices: number;
  /** Wall time of the last `upload` call in ms (CPU side of `texSubImage2D`). */
  readonly ms: number;
}

/** The shared bone-palette texture. */
export class PaletteTexture {
  /** The texture the skinned shader samples (`uPalette`). `sourceTexture` is replaced when the texture grows. */
  readonly texture = new ExternalTexture(null);
  private gl: WebGL2RenderingContext | null = null;
  private tex: WebGLTexture | null = null;
  private allocatedRows = 0;
  private readonly maxRows: number;
  private lost = false;
  /** Statistics of the last upload. */
  last: PaletteUploadStats = { rows: 0, bytes: 0, matrices: 0, ms: 0 };
  /** Times the texture was re-allocated (growth, context restore). */
  allocations = 0;

  constructor(
    private readonly host: PaletteHost,
    options: { readonly initialRows?: number; readonly maxRows?: number } = {},
  ) {
    this.maxRows = Math.max(1, Math.min(MAX_PALETTE_ROWS, options.maxRows ?? MAX_PALETTE_ROWS));
    this.allocate(Math.max(1, options.initialRows ?? INITIAL_PALETTE_ROWS));
  }

  /** Rows currently allocated on the GPU (a power of two, at most 2048). */
  get rows(): number {
    return this.allocatedRows;
  }

  /** Matrices the allocation can hold. */
  get capacityMatrices(): number {
    return this.allocatedRows * MATRICES_PER_ROW;
  }

  /** GPU bytes of the allocation. */
  get gpuBytes(): number {
    return this.allocatedRows * BYTES_PER_ROW;
  }

  private allocate(rows: number): void {
    const gl = this.host.getContext();
    const height = Math.min(this.maxRows, nextPow2(rows));
    if (this.tex !== null && this.gl !== null) this.gl.deleteTexture(this.tex);
    const tex = gl.createTexture();
    this.host.state.bindTexture(GL.TEXTURE_2D, tex);
    try {
      gl.texStorage2D(GL.TEXTURE_2D, 1, RGBA32F, PALETTE_TEX_WIDTH, height);
      gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_MIN_FILTER, GL.NEAREST);
      gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_MAG_FILTER, GL.NEAREST);
      gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_WRAP_S, GL.CLAMP_TO_EDGE);
      gl.texParameteri(GL.TEXTURE_2D, GL.TEXTURE_WRAP_T, GL.CLAMP_TO_EDGE);
    } finally {
      this.host.state.unbindTexture();
    }
    this.gl = gl;
    this.tex = tex;
    this.allocatedRows = height;
    this.texture.sourceTexture = tex;
    this.lost = false;
    this.allocations++;
  }

  /**
   * Uploads the first `rows` rows of `src` (starting at float `offset`; `src` must hold `rows * 6144` floats from
   * there on). Grows the texture first when `rows` exceed the allocation (the old contents are not needed: the
   * palette is rebuilt in full every frame). Returns `false` and uploads nothing when `rows` is 0, the context is
   * lost or the source is too short / the palette needs more than the maximum rows.
   */
  upload(src: Float32Array, offset: number, rows: number): boolean {
    if (rows <= 0 || this.lost) return false;
    if (rows > this.maxRows) return false;
    const floats = rows * FLOATS_PER_ROW;
    if (offset < 0 || offset + floats > src.length) return false;
    const t0 = typeof performance === 'undefined' ? 0 : performance.now();
    if (rows > this.allocatedRows) this.allocate(rows);
    const gl = this.gl;
    if (gl === null || this.tex === null) return false;
    this.host.state.bindTexture(GL.TEXTURE_2D, this.tex);
    try {
      gl.texSubImage2D(
        GL.TEXTURE_2D,
        0,
        0,
        0,
        PALETTE_TEX_WIDTH,
        rows,
        GL.RGBA,
        FLOAT,
        src.subarray(offset, offset + floats),
      );
    } finally {
      this.host.state.unbindTexture();
    }
    this.last = {
      rows,
      bytes: rows * BYTES_PER_ROW,
      matrices: rows * MATRICES_PER_ROW,
      ms: (typeof performance === 'undefined' ? 0 : performance.now()) - t0,
    };
    return true;
  }

  /**
   * Uploads `matrixCount` tightly packed matrices (12 floats each, no row padding), padding the last row with
   * zeros. For sources that are not the render view (the bind-pose source, tests).
   */
  uploadMatrices(matrices: Float32Array, matrixCount: number): boolean {
    const rows = rowsForMatrices(matrixCount);
    if (rows === 0) return false;
    const need = rows * FLOATS_PER_ROW;
    if (matrices.length >= need) return this.upload(matrices, 0, rows);
    const padded = new Float32Array(need);
    padded.set(matrices.subarray(0, Math.min(matrices.length, matrixCount * FLOATS_PER_MATRIX)));
    return this.upload(padded, 0, rows);
  }

  /** The context was lost: every GL object is gone. */
  onContextLost(): void {
    this.lost = true;
    this.tex = null;
    this.gl = null;
    this.texture.sourceTexture = null;
  }

  /** The context is back: allocate again (the next `upload` fills it). */
  onContextRestored(): void {
    this.allocate(Math.max(1, this.allocatedRows));
  }

  dispose(): void {
    if (this.tex !== null && this.gl !== null) this.gl.deleteTexture(this.tex);
    this.tex = null;
    this.gl = null;
    this.texture.sourceTexture = null;
    this.texture.dispose();
  }
}
