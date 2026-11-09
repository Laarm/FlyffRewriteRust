// The pictures of the minimap (`Theme/navigator.inc` -> `ImgNavObj.bmp`, `NavPosition.inc` -> `ImgNavPos.tga`, `ImgNavArrow.bmp`,
// `NavDunFog.tga`; ref/99 §4.4, Plan/45 §7.6; WP-CFEAT-28): decoded once through the UI image cache and atlases, kept as canvases so that
// the painter can `drawImage` a frame of a sheet without any further work.

import type { UiAtlases } from '../../ui/skin/atlas';
import { MISSING, type UiImageCache } from '../../ui/skin/images';
import type { PixelImage } from '../../ui/skin/pixels';

/** A canvas-backed sheet and the rectangle of one frame in it. */
export interface Frame {
  readonly sheet: CanvasImageSource;
  readonly sx: number;
  readonly sy: number;
  readonly w: number;
  readonly h: number;
}

export const NAV_OBJECTS_ATLAS = 'theme/navigator.inc';
export const NAV_POSITION_ATLAS = 'theme/navposition.inc';
export const NAV_ARROW = 'theme/imgnavarrow.bmp';
export const NAV_FOG = 'theme/navdunfog.tga';
/** `m_pDestinationPositionTexture`: the destination of the quest helper (16 x 16). */
export const NAV_DESTINATION = 'theme/buttdestination.bmp';
/** `navigator.inc` has 24 frames, `NavPosition.inc` 4. */
export const OBJECT_FRAMES = 24;
export const MARK_FRAME_COUNT = 4;

/** An RGBA image as a canvas (`null` without a DOM). */
export function pixelsToCanvas(img: PixelImage): HTMLCanvasElement | null {
  if (typeof document === 'undefined') return null;
  const c = document.createElement('canvas');
  c.width = img.width;
  c.height = img.height;
  const ctx = c.getContext('2d');
  if (!ctx) return null;
  ctx.putImageData(
    new ImageData(img.data as Uint8ClampedArray<ArrayBuffer>, img.width, img.height),
    0,
    0,
  );
  return c;
}

export interface SpriteSource {
  readonly atlases: Pick<UiAtlases, 'sprite'>;
  readonly images: Pick<UiImageCache, 'get'>;
}

export class NavSprites {
  readonly #src: SpriteSource;
  readonly #sheets = new WeakMap<PixelImage, CanvasImageSource | null>();
  #objects: (Frame | null)[] = [];
  #marks: (Frame | null)[] = [];
  #arrow: Frame | null = null;
  #fog: Frame | null = null;
  #destination: Frame | null = null;
  #loading: Promise<void> | null = null;
  #done = false;

  constructor(source: SpriteSource) {
    this.#src = source;
  }

  /** All pictures are decoded. */
  get ready(): boolean {
    return this.#done;
  }

  /** Starts the load (once); resolves when every picture is decoded or known to be missing. */
  load(): Promise<void> {
    this.#loading ??= this.#run().then(() => {
      this.#done = true;
    });
    return this.#loading;
  }

  async #run(): Promise<void> {
    const objects: (Frame | null)[] = [];
    for (let i = 0; i < OBJECT_FRAMES; i += 1) {
      objects.push(await this.#atlasFrame(NAV_OBJECTS_ATLAS, i));
    }
    this.#objects = objects;
    const marks: (Frame | null)[] = [];
    for (let i = 0; i < MARK_FRAME_COUNT; i += 1) {
      marks.push(await this.#atlasFrame(NAV_POSITION_ATLAS, i));
    }
    this.#marks = marks;
    this.#arrow = await this.#whole(NAV_ARROW);
    this.#fog = await this.#whole(NAV_FOG);
    this.#destination = await this.#whole(NAV_DESTINATION);
  }

  async #atlasFrame(inc: string, index: number): Promise<Frame | null> {
    const s = await this.#src.atlases.sprite(inc, index);
    if (!s) return null;
    const sheet = this.#sheetOf(s.img);
    return sheet ? { sheet, sx: s.sx, sy: s.sy, w: s.w, h: s.h } : null;
  }

  async #whole(path: string): Promise<Frame | null> {
    const img = await this.#src.images.get(path, 'exact');
    if (img === MISSING) return null;
    const sheet = this.#sheetOf(img);
    return sheet ? { sheet, sx: 0, sy: 0, w: img.width, h: img.height } : null;
  }

  #sheetOf(img: PixelImage): CanvasImageSource | null {
    if (this.#sheets.has(img)) return this.#sheets.get(img) ?? null;
    const c = pixelsToCanvas(img);
    this.#sheets.set(img, c);
    return c;
  }

  /** Frame `i` of `navigator.inc`. */
  object(i: number): Frame | null {
    return this.#objects[i] ?? null;
  }
  /** Frame `i` of `NavPosition.inc`. */
  mark(i: number): Frame | null {
    return this.#marks[i] ?? null;
  }
  get arrow(): Frame | null {
    return this.#arrow;
  }
  get fog(): Frame | null {
    return this.#fog;
  }
  get destination(): Frame | null {
    return this.#destination;
  }
}
