// Fetching and uploading the tiles of the base layer (Plan/25 section 3.4): level 0 is the original minimap `<prefix>XX-ZZ.dds` (BC1 tiles are
// uploaded as compressed S3TC textures when the GPU has the extension; A1R5G5B5, ARGB and every tile without the extension are decoded to
// RGBA8 by the WASM DDS decoder in the decode worker), levels 1.. are the PNG overview tiles of the `admin-map` kind. Every texture is created
// with `flipY = false`: image row 0 (north) is texture row 0, the tile shader flips the plane's v instead.
import {
  CompressedTexture,
  DataTexture,
  LinearFilter,
  NoColorSpace,
  RGBAFormat,
  RGB_S3TC_DXT1_Format,
  Texture,
  UnsignedByteType,
  ClampToEdgeWrapping,
} from 'three';
import { level0Url, overviewUrl } from '../components/mapGeo';
import type { DdsMeta } from '../../gen/formats';
import { Prio } from '../../workers/priority';
import { decodePool } from './decodePool';
import type { TilesMeta } from './metaTypes';
import type { TileId } from './tiles';

/** A tile on the GPU. */
export interface GpuTile {
  readonly texture: Texture;
  /** The right edge of the image in texture coordinates (`256 / 257` for the one odd tile of `WdHeaven06`). */
  readonly uMax: number;
  /** Texture memory in bytes (the budget of the cache). */
  readonly bytes: number;
}

/** What the loader needs of the outside: the network and the DDS decoder (tests pass fakes). */
export interface TileIo {
  /** The bytes of `url`; `null` for a 404; throws for every other failure. */
  fetchBytes(url: string, signal: AbortSignal): Promise<ArrayBuffer | null>;
  /** A DDS file decoded by the pool. */
  decodeDds(bytes: ArrayBuffer, signal: AbortSignal, forceRgba: boolean): Promise<DdsResult>;
  /** A PNG decoded to an image bitmap without premultiplying or colour conversion. */
  decodeBitmap(bytes: ArrayBuffer): Promise<ImageBitmap>;
}

export interface DdsResult {
  readonly meta: Pick<DdsMeta, 'pixel' | 'width' | 'height' | 'levels'>;
  readonly blob: ArrayBuffer | null;
}

/** The browser's implementation of {@link TileIo}. */
export function browserIo(): TileIo {
  return {
    async fetchBytes(url, signal) {
      const r = await fetch(url, { signal, credentials: 'same-origin' });
      if (r.status === 404) return null;
      if (!r.ok) throw new Error(`HTTP ${String(r.status)} for ${url}`);
      return r.arrayBuffer();
    },
    async decodeDds(bytes, signal, forceRgba) {
      const pool = await decodePool();
      const handle = pool.submit({
        kind: forceRgba ? 'dds_bc' : 'dds',
        bytes,
        prio: Prio.Visible,
      });
      const onAbort = (): void => {
        handle.cancel('tile left the view');
      };
      signal.addEventListener('abort', onAbort, { once: true });
      try {
        const r = await handle.promise;
        return { meta: r.meta, blob: r.blob };
      } finally {
        signal.removeEventListener('abort', onAbort);
      }
    },
    decodeBitmap(bytes) {
      return createImageBitmap(new Blob([bytes], { type: 'image/png' }), {
        premultiplyAlpha: 'none',
        colorSpaceConversion: 'none',
      });
    },
  };
}

function baseSettings(t: Texture): void {
  t.minFilter = LinearFilter;
  t.magFilter = LinearFilter;
  t.generateMipmaps = false;
  t.flipY = false;
  t.premultiplyAlpha = false;
  t.colorSpace = NoColorSpace;
  t.wrapS = ClampToEdgeWrapping;
  t.wrapT = ClampToEdgeWrapping;
}

/** Builds the texture of a decoded DDS (`null` when the pack is of a form this uploader does not take). */
export function textureOfDds(r: DdsResult, s3tc: boolean): GpuTile | null {
  const { meta, blob } = r;
  if (!blob) return null;
  const level = meta.levels.find((l) => l.face === 0);
  if (!level) return null;
  if (level.view.off < 0 || level.view.off + level.view.len > blob.byteLength) return null;
  const w = level.w;
  const h = level.h;
  if (w <= 0 || h <= 0) return null;
  const uMax = w === 257 ? 256 / 257 : 1;
  if (meta.pixel === 'bc1' && s3tc) {
    const data = new Uint8Array(blob, level.view.off, level.view.len);
    const tex = new CompressedTexture([{ data, width: w, height: h }], w, h, RGB_S3TC_DXT1_Format);
    baseSettings(tex);
    tex.needsUpdate = true;
    // the CPU copy is not needed once the GPU has it
    tex.onUpdate = () => {
      tex.mipmaps = [];
    };
    return { texture: tex, uMax, bytes: Math.ceil(w / 4) * Math.ceil(h / 4) * 8 };
  }
  if (meta.pixel === 'rgba8' && level.view.len >= w * h * 4) {
    const data = new Uint8Array(blob, level.view.off, w * h * 4);
    const tex = new DataTexture(data, w, h, RGBAFormat, UnsignedByteType);
    baseSettings(tex);
    tex.unpackAlignment = 1;
    tex.needsUpdate = true;
    return { texture: tex, uMax, bytes: w * h * 4 };
  }
  return null;
}

/** The loader of one world's tiles. `load` resolves `null` for a tile that does not exist (a 404 or a form it cannot upload). */
export function createTileLoader(
  tiles: TilesMeta,
  worldId: number,
  io: TileIo,
  s3tc: () => boolean,
): (id: TileId, signal: AbortSignal) => Promise<GpuTile | null> {
  return async (id, signal) => {
    if (id.level === 0) {
      if (tiles.res === null) return null;
      const bytes = await io.fetchBytes(level0Url(tiles.res, tiles.prefix, id.tx, id.tz), signal);
      if (bytes === null) return null;
      const bc = s3tc();
      // the pool is asked for `dds`: it keeps the BC1 blocks when the GPU takes them; anything else is decoded again as RGBA8
      let r = await io.decodeDds(bytes.slice(0), signal, !bc);
      let tile = textureOfDds(r, bc);
      if (tile === null && bc && r.meta.pixel !== 'rgba8') {
        r = await io.decodeDds(bytes, signal, true);
        tile = textureOfDds(r, false);
      }
      return tile;
    }
    if (tiles.gen === null) return null;
    const bytes = await io.fetchBytes(
      overviewUrl(tiles.gen, worldId, id.level, id.tx, id.tz),
      signal,
    );
    if (bytes === null) return null;
    const bitmap = await io.decodeBitmap(bytes);
    const tex = new Texture(bitmap);
    baseSettings(tex);
    tex.needsUpdate = true;
    tex.onUpdate = () => {
      bitmap.close();
    };
    return { texture: tex, uMax: 1, bytes: bitmap.width * bitmap.height * 4 };
  };
}

/** Frees a tile's GPU memory. */
export function disposeTile(t: GpuTile): void {
  t.texture.dispose();
}
