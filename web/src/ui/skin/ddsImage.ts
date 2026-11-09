// DDS files as UI images (WP-REND-25 leftover: `SFX/CountFight.dds`, the sprites of the PvP countdown). The server converts the theme, icon,
// item and char textures to PNG; the `sfx/` directory is served as raw DDS (the effect renderer decodes it for the GPU), so an atlas that
// lives there (`sfx/countfight.inc`) has NO png form and `createImageBitmap` cannot read it. The browser decoder therefore recognises the DDS
// magic in what it fetched and hands the bytes to a DDS decoder (the wasm `decode_dds_bc` job of the decode pool: every pixel format comes
// back as RGBA8, level 0 of face 0 first) whose result this module turns into a `PixelImage`.
//
// Pure functions (no DOM, no worker): the browser wiring is `createBrowserDecoder({ dds })` of `images.ts` and `features/session/uiMount.ts`.

import type { DdsMeta } from '../../gen/formats';
import type { PixelImage } from './pixels';

/** The four magic bytes of a DDS file: `"DDS "`. */
export function isDdsMagic(head: Uint8Array): boolean {
  return (
    head.length >= 4 && head[0] === 0x44 && head[1] === 0x44 && head[2] === 0x53 && head[3] === 0x20
  );
}

/** What a DDS decoder answers (the shape of a `dds_bc` job: the meta and the blob it points into). */
export interface DdsPack {
  readonly meta: DdsMeta;
  readonly blob: Uint8Array;
}

/**
 * Level 0 of face 0 of an RGBA8 pack as a straight-alpha image (row 0 at the top, the DDS order). `null` for a pack that is not
 * RGBA8 (the `dds_bc` job always answers RGBA8), has no level, or whose level does not lie inside the blob.
 */
export function ddsToPixelImage(pack: DdsPack): PixelImage | null {
  const { meta, blob } = pack;
  if (meta.pixel !== 'rgba8') return null;
  const level = meta.levels.find((l) => l.face === 0);
  if (!level) return null;
  const { w, h } = level;
  const bytes = w * h * 4;
  if (w <= 0 || h <= 0 || level.view.len < bytes) return null;
  const end = level.view.off + bytes;
  if (level.view.off < 0 || end > blob.length) return null;
  const data = new Uint8ClampedArray(bytes);
  data.set(blob.subarray(level.view.off, end));
  return { width: w, height: h, data, hasAlpha: true };
}
