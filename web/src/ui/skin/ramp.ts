// The screen ramp on the DOM UI (Plan/42 §7.8, U17; resolves 41 Open question 2).
//
// The original sets the display's device gamma ramp, which brightened EVERYTHING on screen (3-D view, windows, text,
// cursor). 41 reproduces it for the canvas; this module ramps the DOM UI with the SAME table (`buildRampLut` of
// `render/rampLut.ts`, the only implementation):
//
//  - one hidden `<svg>` with the filter `flyff-ui-ramp` (`feComponentTransfer`, `type="table"`, 256 values
//    `out[i] / 255`, `color-interpolation-filters="sRGB"`, no alpha function) appended to the body, created once;
//  - the UI root element gets `filter: url(#flyff-ui-ramp)`; an identity table (and `ramp=off`) sets `filter: none`;
//  - a change of `gamma` / `bright` / `contrast` rewrites the three `tableValues` attributes and the root's `filter`
//    DIRECTLY in the DOM, so React never re-renders;
//  - CSS cursors are drawn by the browser outside the filtered root: every cursor PNG goes through the same LUT on the
//    CPU (decode -> RGBA -> LUT on RGB, alpha unchanged -> PNG data URL), cached per (stem, LUT), and `CursorManager`
//    re-applies the cursors when a ramped image is ready (`setUrlTransform`). A cursor that cannot be processed keeps
//    its original URL.

import { buildRampLut, RAMP_SIZE } from '../../render/rampLut';
import type { CursorManager } from '../cursor/CursorManager';
import type { UiOptions, UiOptionsStore } from '../options';
import { createBrowserDecoder } from './images';

/** The id of the SVG filter (`filter: url(#flyff-ui-ramp)`). */
export const RAMP_FILTER_ID = 'flyff-ui-ramp';
const SVG_NS = 'http://www.w3.org/2000/svg';

/** The `filter` value of a ramped root. */
export const RAMP_FILTER_VALUE = `url(#${RAMP_FILTER_ID})`;

/** The `tableValues` attribute: the 256 entries as `out[i] / 255`, 6 decimals, space separated. */
export function rampTableValues(lut: ArrayLike<number>): string {
  return Array.from(lut, (v) => (v / 255).toFixed(6)).join(' ');
}

/** The inverse of {@link rampTableValues} (tests): `round(v * 255)` per entry. */
export function parseTableValues(values: string): Uint8Array {
  const parts = values.trim().split(/\s+/);
  return Uint8Array.from(parts, (p) => Math.round(Number(p) * 255));
}

/** `out[i] == i` for every entry: the filter is not needed. */
export function isIdentityLut(lut: ArrayLike<number>): boolean {
  if (lut.length !== RAMP_SIZE) return false;
  for (let i = 0; i < RAMP_SIZE; i += 1) if (lut[i] !== i) return false;
  return true;
}

/** RGBA pixels as the canvas hands them out. */
export interface RgbaImage {
  width: number;
  height: number;
  data: Uint8ClampedArray<ArrayBuffer>;
}

/** Maps R, G and B of every pixel through the LUT; alpha is unchanged. Returns a new array. */
export function rampPixels(
  data: ArrayLike<number>,
  lut: ArrayLike<number>,
): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(data.length);
  for (let i = 0; i + 3 < data.length; i += 4) {
    out[i] = lut[data[i] ?? 0] ?? 0;
    out[i + 1] = lut[data[i + 1] ?? 0] ?? 0;
    out[i + 2] = lut[data[i + 2] ?? 0] ?? 0;
    out[i + 3] = data[i + 3] ?? 0;
  }
  return out;
}

/** How a cursor image is decoded to pixels and encoded back (the browser canvas by default; injected in tests). */
export interface CursorImageIo {
  decode(url: string): Promise<RgbaImage>;
  encode(image: RgbaImage): Promise<string>;
}

/**
 * The browser implementation: the UI's own PNG decoder (`createBrowserDecoder`: straight alpha, no colour-space
 * conversion, so the table is applied to the stored values) and `putImageData` + `toDataURL('image/png')`.
 */
export function canvasImageIo(doc: Document): CursorImageIo {
  const decode = createBrowserDecoder();
  return {
    async decode(url) {
      const img = await decode(url);
      if (!img) throw new Error(`cannot decode ${url}`);
      return { width: img.width, height: img.height, data: new Uint8ClampedArray(img.data) };
    },
    encode(image) {
      const canvas = doc.createElement('canvas');
      canvas.width = image.width;
      canvas.height = image.height;
      const ctx = canvas.getContext('2d');
      if (!ctx) return Promise.reject(new Error('no 2-D canvas'));
      ctx.putImageData(new ImageData(image.data, image.width, image.height), 0, 0);
      return Promise.resolve(canvas.toDataURL('image/png'));
    },
  };
}

export interface UiRampOptions {
  /** `gamma`, `bright`, `contrast` come from here and are followed. Without it the defaults apply (`set` overrides). */
  options?: UiOptionsStore;
  /** The document that owns the SVG (default: the global one, if any). */
  doc?: Document | null;
  /** The LUT generator (default `buildRampLut`; tests inject an identity). */
  buildLut?: (gamma: number, bright: number, contrast: number) => Uint8Array;
  /** Cursor image decode/encode (default: the canvas, when there is a document). */
  io?: CursorImageIo | null;
  /** `false`: `ramp=off` (gallery screenshots): `filter: none` and unramped cursors. */
  enabled?: boolean;
  /** Reports a cursor image that could not be ramped. */
  onError?: (stem: string, error: unknown) => void;
}

type RampValues = Pick<UiOptions, 'gamma' | 'bright' | 'contrast'>;

export class UiRamp {
  readonly #options: UiOptionsStore | null;
  readonly #doc: Document | null;
  readonly #buildLut: (gamma: number, bright: number, contrast: number) => Uint8Array;
  readonly #io: CursorImageIo | null;
  readonly #onError: ((stem: string, error: unknown) => void) | undefined;
  readonly #roots = new Set<HTMLElement>();
  readonly #cursorCache = new Map<string, Map<string, string>>();
  readonly #cursorPending = new Set<string>();
  #cursor: CursorManager | null = null;
  #svg: SVGSVGElement | null = null;
  #funcs: SVGElement[] = [];
  #override: RampValues | null = null;
  #enabled: boolean;
  #lut: Uint8Array;
  #table = '';
  #identity = true;
  #unsubscribe: (() => void) | null = null;
  #rebuilds = 0;

  constructor(o: UiRampOptions = {}) {
    this.#options = o.options ?? null;
    this.#doc = o.doc === undefined ? (typeof document === 'undefined' ? null : document) : o.doc;
    this.#buildLut = o.buildLut ?? buildRampLut;
    this.#io = o.io === undefined ? (this.#doc ? canvasImageIo(this.#doc) : null) : o.io;
    this.#onError = o.onError;
    this.#enabled = o.enabled ?? true;
    this.#lut = new Uint8Array(RAMP_SIZE);
    this.#rebuild();
    this.#unsubscribe =
      this.#options?.subscribe(() => {
        this.#rebuild();
      }) ?? null;
  }

  /** The current table (256 entries). */
  get lut(): Uint8Array {
    return this.#lut;
  }
  /** The `tableValues` string of the current table. */
  get tableValues(): string {
    return this.#table;
  }
  /** `true` when the table is the identity (the root's `filter` is `none`). */
  get identity(): boolean {
    return this.#identity;
  }
  get enabled(): boolean {
    return this.#enabled;
  }
  /** The `filter` value the roots carry now. */
  get filterValue(): string {
    return this.#enabled && !this.#identity ? RAMP_FILTER_VALUE : 'none';
  }
  /** How many times the table was rebuilt (tests: an option change rewrites the attributes once). */
  get rebuilds(): number {
    return this.#rebuilds;
  }
  /** The hidden SVG element (`null` before a root was attached or without a document). */
  get svg(): SVGSVGElement | null {
    return this.#svg;
  }

  /** `ramp=off` / on. */
  setEnabled(on: boolean): void {
    if (this.#enabled === on) return;
    this.#enabled = on;
    this.#applyRoots();
    this.#refreshCursor();
  }

  /** Overrides the option values (the `/dev/ui/ramp` page); `null` follows the options again. */
  set(values: RampValues | null): void {
    this.#override = values;
    this.#rebuild();
  }

  /**
   * Puts the filter on the UI root element (and creates the SVG on first use). Returns the detach function, which
   * clears the root's `filter` again.
   */
  attachRoot(el: HTMLElement): () => void {
    this.#mount(el.ownerDocument);
    this.#roots.add(el);
    el.style.filter = this.filterValue;
    return () => {
      this.#roots.delete(el);
      el.style.filter = '';
    };
  }

  /** Hands the ramped cursor images to `cursor` (`setUrlTransform`); returns the detach function. */
  bindCursor(cursor: CursorManager): () => void {
    this.#cursor = cursor;
    cursor.setUrlTransform((stem, url) => this.#cursorUrl(stem, url));
    return () => {
      if (this.#cursor === cursor) this.#cursor = null;
      cursor.setUrlTransform(null);
    };
  }

  /** Removes the SVG and the subscriptions. */
  dispose(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    for (const el of this.#roots) el.style.filter = '';
    this.#roots.clear();
    this.#svg?.remove();
    this.#svg = null;
    this.#funcs = [];
    if (this.#cursor) {
      this.#cursor.setUrlTransform(null);
      this.#cursor = null;
    }
  }

  #values(): RampValues {
    return this.#override ?? this.#options?.get() ?? { gamma: 1, bright: 10, contrast: 1 };
  }

  #rebuild(): void {
    const v = this.#values();
    const lut = this.#buildLut(v.gamma, v.bright, v.contrast);
    const table = rampTableValues(lut);
    if (table === this.#table) return;
    this.#lut = lut;
    this.#table = table;
    this.#identity = isIdentityLut(lut);
    this.#rebuilds += 1;
    for (const f of this.#funcs) f.setAttribute('tableValues', table);
    this.#applyRoots();
    this.#refreshCursor();
  }

  #applyRoots(): void {
    const value = this.filterValue;
    for (const el of this.#roots) if (el.style.filter !== value) el.style.filter = value;
  }

  #mount(doc: Document): void {
    if (this.#svg?.ownerDocument === doc && this.#svg.isConnected) return;
    this.#svg?.remove();
    // one filter per document: a stale one (HMR, a second UI in a test) is replaced
    for (const old of Array.from(doc.querySelectorAll('svg[data-flyff-ui-ramp]'))) old.remove();
    const svg = doc.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('width', '0');
    svg.setAttribute('height', '0');
    svg.setAttribute('aria-hidden', 'true');
    svg.setAttribute('data-flyff-ui-ramp', '');
    svg.setAttribute('style', 'position:absolute');
    const filter = doc.createElementNS(SVG_NS, 'filter');
    filter.setAttribute('id', RAMP_FILTER_ID);
    filter.setAttribute('color-interpolation-filters', 'sRGB');
    filter.setAttribute('filterUnits', 'objectBoundingBox');
    filter.setAttribute('x', '0');
    filter.setAttribute('y', '0');
    filter.setAttribute('width', '1');
    filter.setAttribute('height', '1');
    const transfer = doc.createElementNS(SVG_NS, 'feComponentTransfer');
    this.#funcs = ['feFuncR', 'feFuncG', 'feFuncB'].map((name) => {
      const f = doc.createElementNS(SVG_NS, name);
      f.setAttribute('type', 'table');
      f.setAttribute('tableValues', this.#table);
      transfer.append(f);
      return f;
    });
    filter.append(transfer);
    svg.append(filter);
    doc.body.append(svg);
    this.#svg = svg;
  }

  // ---- cursors -----------------------------------------------------------------------------------------------

  #cursorUrl(stem: string, url: string): string | null {
    if (!this.#enabled || this.#identity || !this.#io) return null;
    const hit = this.#cursorCache.get(this.#table)?.get(stem);
    if (hit !== undefined) return hit === '' ? null : hit;
    this.#schedule(stem, url);
    return null;
  }

  #schedule(stem: string, url: string): void {
    const io = this.#io;
    if (!io) return;
    const table = this.#table;
    const lut = this.#lut;
    const pending = `${table}|${stem}`;
    if (this.#cursorPending.has(pending)) return;
    this.#cursorPending.add(pending);
    const store = (value: string): void => {
      this.#cursorPending.delete(pending);
      let byStem = this.#cursorCache.get(table);
      if (!byStem) {
        byStem = new Map();
        this.#cursorCache.set(table, byStem);
        // keep the last few tables only (the options window drags a slider through many)
        while (this.#cursorCache.size > 4) {
          const oldest = this.#cursorCache.keys().next().value;
          if (oldest === undefined) break;
          this.#cursorCache.delete(oldest);
        }
      }
      byStem.set(stem, value);
      if (table === this.#table) this.#cursor?.refresh();
    };
    void io
      .decode(url)
      .then(async (img) =>
        io.encode({ width: img.width, height: img.height, data: rampPixels(img.data, lut) }),
      )
      .then(store)
      .catch((e: unknown) => {
        this.#onError?.(stem, e);
        store(''); // keep the original image
      });
  }

  #refreshCursor(): void {
    this.#cursor?.refresh();
  }
}
