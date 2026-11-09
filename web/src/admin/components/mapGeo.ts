// The tile placement of the admin map (Plan/25 section 3.3, 3.4), pure and shared by the live map (`world/`) and the static previews
// (`StaticMapPreview`): this file lives in `components/` so that a preview never pulls a module of the map engine into its chunk.
//
//   x = world x, y = world z, north up. A landscape (lx, lz) covers x in [lx*L, (lx+1)*L] and z in [lz*L, (lz+1)*L], L = 128 * mpu.
//   Its minimap tile has image row 0 on the NORTH edge: pixel (u, v) is x = lx*L + u*L/256, z = (lz+1)*L - v*L/256.
//   An overview tile of level k covers 2^k x 2^k landscapes in one 256 px image (tx, tz count like the landscapes, the image is north-up).

/** Pixels per tile side at every level. */
export const TILE_PX = 256;

/** The side of a landscape in metres (`MAP_SIZE * MPU`). */
export function landMeters(mpu: number): number {
  return 128 * mpu;
}

/** World units per texel of a level-`k` tile (2 at MPU 4 and level 0). */
export function texelSize(landM: number, level: number): number {
  return (landM * 2 ** level) / TILE_PX;
}

/** The landscape coordinate containing `v` (a position along one axis). */
export function landOf(v: number, landM: number): number {
  return Math.floor(v / landM);
}

export interface TilePixel {
  readonly lx: number;
  readonly lz: number;
  /** Pixel column, fractional, 0..256 from the west edge. */
  readonly u: number;
  /** Pixel row, fractional, 0..256 from the NORTH edge. */
  readonly v: number;
}

/** The landscape and the pixel of its level-0 tile that show the world point (x, z). */
export function tilePixel(x: number, z: number, landM: number): TilePixel {
  const lx = landOf(x, landM);
  const lz = landOf(z, landM);
  return {
    lx,
    lz,
    u: ((x - lx * landM) / landM) * TILE_PX,
    v: (((lz + 1) * landM - z) / landM) * TILE_PX,
  };
}

/** The world point at pixel (u, v) of the level-0 tile of landscape (lx, lz). */
export function pixelWorld(
  lx: number,
  lz: number,
  u: number,
  v: number,
  landM: number,
): { x: number; z: number } {
  return { x: lx * landM + (u * landM) / TILE_PX, z: (lz + 1) * landM - (v * landM) / TILE_PX };
}

/** The overview tile of level `k` (>= 1) that shows (x, z), and the pixel in it (row 0 north). */
export function overviewPixel(
  x: number,
  z: number,
  landM: number,
  k: number,
): { tx: number; tz: number; u: number; v: number } {
  const span = landM * 2 ** k;
  const tx = Math.floor(x / span);
  const tz = Math.floor(z / span);
  return {
    tx,
    tz,
    u: ((x - tx * span) / span) * TILE_PX,
    v: (((tz + 1) * span - z) / span) * TILE_PX,
  };
}

const pad2 = (n: number): string => (n < 10 ? `0${String(n)}` : String(n));

/** `<prefix>XX-ZZ.dds`: the level-0 file of landscape (lx, lz). */
export function level0File(prefix: string, lx: number, lz: number): string {
  return `${prefix}${pad2(lx)}-${pad2(lz)}.dds`;
}

/** The URL of the level-0 tile, `res` being `TilesMeta.res`. */
export function level0Url(res: string, prefix: string, lx: number, lz: number): string {
  return `${res}${level0File(prefix, lx, lz)}`;
}

/** The URL of the overview tile (level >= 1), `gen` being `TilesMeta.gen`. */
export function overviewUrl(
  gen: string,
  world: number,
  level: number,
  tx: number,
  tz: number,
): string {
  return `${gen}${String(world)}/${String(level)}/${String(tx)}_${String(tz)}.png`;
}

/** `true` when `covered[lz][lx]` is `1` (`TilesMeta.covered`: one string per landscape row, row 0 = z 0). */
export function isCovered(covered: readonly string[], lx: number, lz: number): boolean {
  return covered[lz]?.charCodeAt(lx) === 49;
}
