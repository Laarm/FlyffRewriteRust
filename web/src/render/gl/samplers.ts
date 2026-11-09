// Sampler presets of the renderer (Plan/41 section 6.1 table "Sampler presets", R8). A preset decides the filters,
// the wrapping and how many mip levels are uploaded (`TEXTURE_MAX_LEVEL`).

import { GL } from './glConsts';

export type SamplerPresetName =
  'world' | 'sky' | 'lightmap' | 'sfx' | 'ui2d' | 'ui2dNearest' | 'palette';

export interface SamplerSpec {
  readonly name: SamplerPresetName;
  readonly min: number;
  readonly mag: number;
  readonly wrap: number;
  /** Highest mip level uploaded (`TEXTURE_MAX_LEVEL`), `'full'` = the whole chain of the file. */
  readonly maxLevel: number | 'full';
}

/**
 * - `world` (models, terrain layers, water): `LINEAR_MIPMAP_NEAREST` (POINT mip filter, World3D.cpp:156) with
 *   `TEXTURE_MAX_LEVEL = 3`: KEEP-QUIRK, D3DX created 4 levels (ref/18 section 8.1, TerrainMng.cpp:63-66);
 * - `sky`: no mips (MIPFILTER NONE, World3D.cpp:198);
 * - `lightmap`: terrain light/alpha maps, 1 level, clamped;
 * - `sfx`: the full chain with a triangle filter (ref/20 section 4.7);
 * - `ui2d`: overlay atlases, balloons use NEAREST;
 * - `palette`: the bone palette, NEAREST.
 */
export const SAMPLER_PRESETS: Readonly<Record<SamplerPresetName, SamplerSpec>> = {
  world: {
    name: 'world',
    min: GL.LINEAR_MIPMAP_NEAREST,
    mag: GL.LINEAR,
    wrap: GL.REPEAT,
    maxLevel: 3,
  },
  sky: { name: 'sky', min: GL.LINEAR, mag: GL.LINEAR, wrap: GL.REPEAT, maxLevel: 0 },
  lightmap: {
    name: 'lightmap',
    min: GL.LINEAR,
    mag: GL.LINEAR,
    wrap: GL.CLAMP_TO_EDGE,
    maxLevel: 0,
  },
  sfx: {
    name: 'sfx',
    min: GL.LINEAR_MIPMAP_LINEAR,
    mag: GL.LINEAR,
    wrap: GL.REPEAT,
    maxLevel: 'full',
  },
  ui2d: { name: 'ui2d', min: GL.LINEAR, mag: GL.LINEAR, wrap: GL.CLAMP_TO_EDGE, maxLevel: 0 },
  ui2dNearest: {
    name: 'ui2dNearest',
    min: GL.NEAREST,
    mag: GL.NEAREST,
    wrap: GL.CLAMP_TO_EDGE,
    maxLevel: 0,
  },
  palette: {
    name: 'palette',
    min: GL.NEAREST,
    mag: GL.NEAREST,
    wrap: GL.CLAMP_TO_EDGE,
    maxLevel: 0,
  },
};

/** `true` for the mipmapped minification filters. */
export function usesMips(minFilter: number): boolean {
  return (
    minFilter === GL.NEAREST_MIPMAP_NEAREST ||
    minFilter === GL.LINEAR_MIPMAP_NEAREST ||
    minFilter === GL.NEAREST_MIPMAP_LINEAR ||
    minFilter === GL.LINEAR_MIPMAP_LINEAR
  );
}

/** The non-mipmapped filter closest to `minFilter` (a texture with a single level must not use a mip filter). */
export function withoutMips(minFilter: number): number {
  return minFilter === GL.NEAREST_MIPMAP_NEAREST || minFilter === GL.NEAREST_MIPMAP_LINEAR
    ? GL.NEAREST
    : usesMips(minFilter)
      ? GL.LINEAR
      : minFilter;
}

/** Number of levels of a chain down to 1x1 for a texture of `w` x `h`. */
export function fullChainLength(w: number, h: number): number {
  return Math.floor(Math.log2(Math.max(1, w, h))) + 1;
}
