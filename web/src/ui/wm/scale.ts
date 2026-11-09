// UI scaling and the virtual viewport (Plan/42 §16, PL-125).
//
// The UI lays itself out in VIRTUAL pixels. The root is a `V.w x V.h` box with `transform: scale(s)`; the pointer is
// divided by `s` (`UiRoot`), integer scales above 1 use `image-rendering: pixelated`. `s` is the `uiScale` option, or
// (option 0 = auto) the §16 rule over the CSS size of the browser viewport:
//
//   vh < 720 or vw < 1024:   s = max(0.6, floorTo(min(vh / 720, vw / 1024), 0.05))
//   vh >= 1440 and vw >= 2560: s = floorTo(min(vh / 1080, vw / 1920), 0.25)
//   otherwise                s = 1
//
// `UiScaler` owns the state (css viewport, option, mode) and pushes `V` and the taskbar inset into the window manager,
// which realigns the windows (`alignAll`, §5.10). Everything else is a pure function so the goldens run anywhere.

import { Observable } from '../wnd/observable';
import { UI_SCALE_MAX, UI_SCALE_MIN, type UiOptionsStore } from '../options';
import type { WindowManager } from './WindowManager';

export { UI_SCALE_MAX, UI_SCALE_MIN };

/** The taskbar height of the game states (`rectLayout = (0, 0, V.w, V.h - 48)`, §16). */
export const TASKBAR_HEIGHT = 48;

/** The step of the automatic downscale and of the automatic upscale. */
const DOWN_STEP = 0.05;
const UP_STEP = 0.25;

/** `floor(v / step) * step` without the binary-fraction noise (`0.9 / 0.05` is not an integer in IEEE-754). */
export function floorTo(v: number, step: number): number {
  const k = Math.floor(v / step + 1e-9);
  return Math.round(k * step * 1e6) / 1e6;
}

/** The automatic scale for a CSS viewport of `vw x vh` px (§16). */
export function autoUiScale(vw: number, vh: number): number {
  if (!(vw > 0) || !(vh > 0)) return 1;
  if (vh < 720 || vw < 1024) {
    return Math.max(UI_SCALE_MIN, floorTo(Math.min(vh / 720, vw / 1024), DOWN_STEP));
  }
  if (vh >= 1440 && vw >= 2560) return floorTo(Math.min(vh / 1080, vw / 1920), UP_STEP);
  return 1;
}

/** An explicit scale clamped into `[0.6, 2.0]`. */
export function clampUiScale(scale: number): number {
  return Math.min(UI_SCALE_MAX, Math.max(UI_SCALE_MIN, scale));
}

/** The scale of option value `option` (0 = auto, else clamped into `[0.6, 2.0]`) for a CSS viewport. */
export function resolveUiScale(option: number, vw: number, vh: number): number {
  if (!Number.isFinite(option) || option <= 0) return autoUiScale(vw, vh);
  return clampUiScale(option);
}

/** `V = (floor(vw / s), floor(vh / s))`. */
export function virtualViewport(
  vw: number,
  vh: number,
  scale: number,
): { width: number; height: number } {
  return {
    width: Math.max(1, Math.floor(vw / scale + 1e-9)),
    height: Math.max(1, Math.floor(vh / scale + 1e-9)),
  };
}

/** Integer scales above 1 keep the skins and icons crisp (`image-rendering: pixelated`). */
export function isPixelatedScale(scale: number): boolean {
  return scale > 1 && Number.isInteger(scale);
}

/** Title screens (login, character select/create) have no taskbar; the game states reserve 48 px. */
export type UiMode = 'title' | 'game';

/** The taskbar inset of a mode. */
export const modeTaskbarHeight = (mode: UiMode): number => (mode === 'game' ? TASKBAR_HEIGHT : 0);

export interface UiScaleState {
  /** The browser viewport in CSS px. */
  readonly css: { readonly width: number; readonly height: number };
  /** The resolved scale `s`. */
  readonly scale: number;
  /** The virtual viewport `V`. */
  readonly virtual: { readonly width: number; readonly height: number };
  /** The taskbar inset subtracted from the layout rect. */
  readonly taskbarHeight: number;
  readonly pixelated: boolean;
}

/** The part of `window` the scaler reads. */
export interface BrowserViewport {
  readonly innerWidth: number;
  readonly innerHeight: number;
  addEventListener(type: 'resize', fn: () => void): void;
  removeEventListener(type: 'resize', fn: () => void): void;
}

export interface UiScalerOptions {
  wm: WindowManager;
  /** Where `uiScale` comes from; without a store the scale is automatic. */
  options?: UiOptionsStore;
  /** The browser viewport in CSS px (default: the manager's current viewport at scale 1). */
  css?: { width: number; height: number };
  /** Default: the inset the manager has now (no taskbar unless it was created with one). */
  mode?: UiMode;
}

function sameState(a: UiScaleState, b: UiScaleState): boolean {
  return (
    a.css.width === b.css.width &&
    a.css.height === b.css.height &&
    a.scale === b.scale &&
    a.virtual.width === b.virtual.width &&
    a.virtual.height === b.virtual.height &&
    a.taskbarHeight === b.taskbarHeight
  );
}

/**
 * Connects the browser viewport, the `uiScale` option and the window manager: whenever one of them changes it
 * recomputes `s` and `V` and calls `wm.setViewport(V.w, V.h, taskbarHeight)`.
 *
 * A scaler that was never told the browser viewport (no `css` option, no `setCssViewport`/`observeBrowser`) is
 * UNMANAGED: the manager's virtual viewport stays as it was created and only an explicit scale (option or override)
 * applies; the automatic rule needs the browser size. Tests and the gallery create it that way.
 */
export class UiScaler {
  readonly #wm: WindowManager;
  readonly #options: UiOptionsStore | null;
  readonly #state: Observable<UiScaleState>;
  readonly #fixed: { width: number; height: number };
  #optionOverride: number | null = null;
  #unsubscribe: (() => void) | null = null;
  #taskbar: number;
  /** The browser viewport; `null` until the first `setCssViewport` (unmanaged). */
  #css: { width: number; height: number } | null;

  constructor(o: UiScalerOptions) {
    this.#wm = o.wm;
    this.#options = o.options ?? null;
    const vp = o.wm.viewport;
    this.#fixed = { width: vp.r - vp.l, height: vp.b - vp.t };
    this.#taskbar = o.mode ? modeTaskbarHeight(o.mode) : vp.b - o.wm.layoutRect.b;
    this.#css = o.css ? { ...o.css } : null;
    this.#state = new Observable<UiScaleState>(this.#compute(), sameState);
    this.#unsubscribe =
      this.#options?.subscribe(() => {
        this.#recompute();
      }) ?? null;
    this.#push(this.#state.get());
  }

  /** The current state (`useSyncExternalStore(scaler.subscribe, scaler.get)`). */
  readonly get = (): UiScaleState => this.#state.get();
  readonly subscribe = (fn: () => void): (() => void) => this.#state.subscribe(fn);

  get scale(): number {
    return this.#state.get().scale;
  }

  /** Whether the browser viewport is known (the automatic rule is in force). */
  get managed(): boolean {
    return this.#css !== null;
  }

  /** The browser viewport changed (`window.innerWidth/innerHeight`, `visualViewport`). */
  setCssViewport(width: number, height: number): void {
    if (this.#css?.width === width && this.#css.height === height) return;
    this.#css = { width, height };
    this.#recompute();
  }

  /** Overrides the `uiScale` option (the gallery's `scale=` parameter); `null` follows the option again. */
  setOption(uiScale: number | null): void {
    this.#optionOverride = uiScale;
    this.#recompute();
  }

  /** Title screens: no taskbar, `rectLayout = rectWindow`; game states: 48 px taskbar. */
  setMode(mode: UiMode): void {
    this.setTaskbarHeight(modeTaskbarHeight(mode));
  }

  /** The taskbar inset of the layout rect (the taskbar shell of WP-UI-14 reports its real height). */
  setTaskbarHeight(height: number): void {
    const h = Math.max(0, Math.trunc(height));
    if (this.#taskbar === h) return;
    this.#taskbar = h;
    this.#recompute();
  }

  /**
   * Follows the browser window: `innerWidth/innerHeight` now and on every `resize`. Returns the detach function.
   * The game's root view calls it once; the gallery and the tests drive `setCssViewport` themselves.
   */
  observeBrowser(win: BrowserViewport = window): () => void {
    const read = (): void => {
      this.setCssViewport(win.innerWidth, win.innerHeight);
    };
    read();
    win.addEventListener('resize', read);
    return () => {
      win.removeEventListener('resize', read);
    };
  }

  /** Detaches from the options store. */
  dispose(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
  }

  #compute(): UiScaleState {
    const option = this.#optionOverride ?? this.#options?.get().uiScale ?? 0;
    const css = this.#css;
    let scale: number;
    let width: number;
    let height: number;
    let virtual: { width: number; height: number };
    if (css) {
      width = css.width;
      height = css.height;
      scale = resolveUiScale(option, width, height);
      virtual = virtualViewport(width, height, scale);
    } else {
      scale = Number.isFinite(option) && option > 0 ? clampUiScale(option) : 1;
      virtual = this.#fixed;
      width = Math.round(virtual.width * scale);
      height = Math.round(virtual.height * scale);
    }
    return {
      css: { width, height },
      scale,
      virtual,
      taskbarHeight: this.#taskbar,
      pixelated: isPixelatedScale(scale),
    };
  }

  #recompute(): void {
    const next = this.#compute();
    if (!this.#state.set(next)) return;
    this.#push(next);
  }

  #push(s: UiScaleState): void {
    const vp = this.#wm.viewport;
    const lay = this.#wm.layoutRect;
    if (
      vp.r - vp.l === s.virtual.width &&
      vp.b - vp.t === s.virtual.height &&
      vp.b - lay.b === s.taskbarHeight
    ) {
      return;
    }
    this.#wm.setViewport(s.virtual.width, s.virtual.height, s.taskbarHeight);
  }
}
