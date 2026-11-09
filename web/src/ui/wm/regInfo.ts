// Window position persistence (Plan/42 §17; `regInfo.dat` of `CWndMgr::PutRegInfo` / `SetRegInfo`,
// WndManager.cpp:7280-7473; PL-125, PL-128).
//
// One record per character in the browser: `safeStorage.local["flyff.ui.v1.reginfo.<idPlayer>"]` =
// `{v: 1, uiRes: [w, h], windows: {[appId]: {rect: [l, t, r, b], extra?, extraVersion?}}}`.
//
//  - SAVED for applet windows only: when one is destroyed (`DestroyApplet`, through the manager's `rectStore.save`) and
//    for all open applets when the world is left (`saveAll`). A window can opt out (`putRegInfo = false`, `IsPutRegInfo`).
//  - RESTORED when an applet window is created (`wm.open()` after `initialize()` calls `restore`): the `extra` state
//    goes to `restoreRegInfo` FIRST (the original deserialises before it positions), then `WBS_THICKFRAME` frames get
//    the stored rect, every other frame only the stored top-left (the size comes from the layout).
//  - The original applied a record only when the saved resolution equalled the current one; with a browser window of
//    any size we always apply it (PL-125) and clamp it into the layout rect: negative left/top become 0, a window
//    bigger than the layout shrinks to it, then the §5.4 `snapMove` clamp.
//  - The stored "open" flag is not used to reopen windows (the original's code for it is commented out, KEEP).
//  - Every storage access is guarded: a throwing, full or corrupt storage leaves the UI working (windows open at the
//    positions of their layout).

import type { Logger } from '../../platform/log';
import { safeStorage } from '../../platform/storage';
import { type Rect, rectHeight, rectWidth } from '../layout/geometry';
import {
  WBS_CHILD,
  WBS_MANAGER,
  WBS_MODAL,
  WBS_POPUP,
  WBS_THICKFRAME,
  WBS_TOPMOST,
} from '../layout/styles';
import type { WndNeuz } from '../wnd/WndNeuz';
import { snapMove } from './dragMath';
import type { WindowRectStore } from './WindowManager';

/** The key prefix of the per-character record. */
export const REGINFO_KEY_PREFIX = 'flyff.ui.v1.reginfo.';
/** The record format version (`v`). */
export const REGINFO_VERSION = 1;

export interface RegInfoEntry {
  rect: [number, number, number, number];
  extra?: unknown;
  extraVersion?: number;
}

export interface RegInfoRecord {
  v: 1;
  uiRes: [number, number];
  windows: Record<string, RegInfoEntry>;
}

/** The part of `SafeStorage` the store uses (a stub in tests). */
export interface RegInfoStorage {
  get(key: string): string | null;
  set(key: string, value: string): boolean;
}

export interface RegInfoOptions {
  /** `selfStore.idPlayer` of the logged-in character; `null` before login (nothing is read or written then). */
  idPlayer: () => number | null | undefined;
  /** `wm.layoutRect`: where windows may be. */
  layout: () => Rect;
  /** `wm.viewport`: its size is stored as `uiRes`. */
  viewport: () => Rect;
  /** The open frames (`saveAll`): `wm.order()`. */
  frames: () => Iterable<WndNeuz>;
  /** Default: `safeStorage.local` (40's wrapper with in-memory fallback). */
  storage?: RegInfoStorage;
  /** Which frames are applets; default {@link defaultIsApplet}. WP-UI-15's applet registry replaces it. */
  isApplet?: (frame: WndNeuz) => boolean;
  log?: Logger;
}

/**
 * The default notion of "applet window" (`CreateApplet` windows): a real app id and none of the layers that are not
 * applets (manager layer, modal dialogs, popups, topmost, child frames), and no `putRegInfo = false` opt-out.
 */
export function defaultIsApplet(frame: WndNeuz): boolean {
  if (frame.appId <= 0 || !frame.putRegInfo) return false;
  return !(
    frame.isWndStyle(WBS_MANAGER) ||
    frame.isWndStyle(WBS_MODAL) ||
    frame.isWndStyle(WBS_POPUP) ||
    frame.isWndStyle(WBS_TOPMOST) ||
    frame.isWndStyle(WBS_CHILD)
  );
}

const isIntLike = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);

function parseRect(v: unknown): [number, number, number, number] | null {
  if (!Array.isArray(v) || v.length !== 4) return null;
  const [l, t, r, b] = v as unknown[];
  if (!isIntLike(l) || !isIntLike(t) || !isIntLike(r) || !isIntLike(b)) return null;
  return [Math.trunc(l), Math.trunc(t), Math.trunc(r), Math.trunc(b)];
}

/** Parses and validates a stored record; `null` for a missing, corrupt or foreign-version one. */
export function parseRegInfo(text: string | null): RegInfoRecord | null {
  if (text === null || text === '') return null;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof raw !== 'object' || raw === null) return null;
  const rec = raw as { v?: unknown; uiRes?: unknown; windows?: unknown };
  if (rec.v !== REGINFO_VERSION) return null;
  const res = rec.uiRes;
  const uiRes: [number, number] =
    Array.isArray(res) && isIntLike(res[0]) && isIntLike(res[1]) ? [res[0], res[1]] : [0, 0];
  const windows: Record<string, RegInfoEntry> = {};
  if (typeof rec.windows === 'object' && rec.windows !== null) {
    for (const [k, v] of Object.entries(rec.windows as Record<string, unknown>)) {
      if (typeof v !== 'object' || v === null) continue;
      const e = v as { rect?: unknown; extra?: unknown; extraVersion?: unknown };
      const rect = parseRect(e.rect);
      if (!rect) continue;
      const entry: RegInfoEntry = { rect };
      if (e.extra !== undefined) entry.extra = e.extra;
      if (isIntLike(e.extraVersion)) entry.extraVersion = e.extraVersion;
      windows[k] = entry;
    }
  }
  return { v: REGINFO_VERSION, uiRes, windows };
}

/**
 * The rect a frame takes from a stored one: THICKFRAME frames get the stored rect (after the frame's own
 * `constrainResize`), the others keep their size and take the stored top-left; both are clamped into the layout.
 * `null` when the stored rect is empty or inverted.
 */
export function restoredRect(
  frame: WndNeuz,
  stored: readonly [number, number, number, number],
  layout: Rect,
): Rect | null {
  const [sl, st, sr, sb] = stored;
  const current = frame.rectWindow;
  let w: number;
  let h: number;
  let l = sl;
  let t = st;
  if (frame.isWndStyle(WBS_THICKFRAME)) {
    if (sr <= sl || sb <= st) return null;
    let r: Rect = { l: sl, t: st, r: sr, b: sb };
    if (frame.constrainResize) r = frame.constrainResize(r, current);
    l = r.l;
    t = r.t;
    w = Math.min(rectWidth(r), rectWidth(layout));
    h = Math.min(rectHeight(r), rectHeight(layout));
  } else {
    w = rectWidth(current);
    h = rectHeight(current);
  }
  l = Math.max(0, l);
  t = Math.max(0, t);
  const p = snapMove({ x: l, y: t }, w, h, layout);
  return { l: p.x, t: p.y, r: p.x + w, b: p.y + h };
}

export class RegInfoStore implements WindowRectStore {
  readonly #o: RegInfoOptions;
  readonly #storage: RegInfoStorage;

  constructor(options: RegInfoOptions) {
    this.#o = options;
    this.#storage = options.storage ?? safeStorage.local;
  }

  /** The storage key of the current character, `null` while there is none. */
  get key(): string | null {
    let id: number | null | undefined;
    try {
      id = this.#o.idPlayer();
    } catch {
      return null;
    }
    return typeof id === 'number' && Number.isFinite(id)
      ? `${REGINFO_KEY_PREFIX}${String(id)}`
      : null;
  }

  #isApplet(frame: WndNeuz): boolean {
    try {
      return (this.#o.isApplet ?? defaultIsApplet)(frame);
    } catch {
      return false;
    }
  }

  /** The stored record of the current character (`null`: no character, nothing stored, or unreadable). */
  read(): RegInfoRecord | null {
    const key = this.key;
    if (key === null) return null;
    try {
      return parseRegInfo(this.#storage.get(key));
    } catch (e) {
      this.#o.log?.warn('ui', `regInfo read failed: ${String(e)}`);
      return null;
    }
  }

  /** `SetRegInfo` of one applet: `extra` first, then the rect (§17). Never throws. */
  restore(frame: WndNeuz): void {
    if (!this.#isApplet(frame)) return;
    const entry = this.read()?.windows[String(frame.appId)];
    if (!entry) return;
    try {
      if (entry.extra !== undefined && frame.restoreRegInfo) {
        frame.restoreRegInfo(entry.extra, entry.extraVersion ?? 0);
      }
    } catch (e) {
      this.#o.log?.error('ui', `restoreRegInfo of ${String(frame.appId)} failed: ${String(e)}`);
    }
    try {
      const rect = restoredRect(frame, entry.rect, this.#o.layout());
      if (rect) frame.setWndRect(rect, true);
    } catch (e) {
      this.#o.log?.error('ui', `regInfo rect of ${String(frame.appId)} failed: ${String(e)}`);
    }
  }

  /** `PutRegInfo` of one applet (`DestroyApplet`). Never throws. */
  save(frame: WndNeuz): void {
    this.#write([frame]);
  }

  /** `PutRegInfo` of every open applet (leaving the world): one write. */
  saveAll(): void {
    let frames: WndNeuz[];
    try {
      frames = [...this.#o.frames()];
    } catch {
      return;
    }
    this.#write(frames);
  }

  /** Forgets the stored positions of the current character. */
  clear(): void {
    const key = this.key;
    if (key === null) return;
    try {
      this.#storage.set(key, JSON.stringify({ v: REGINFO_VERSION, uiRes: [0, 0], windows: {} }));
    } catch {
      // storage unavailable: nothing was stored anyway
    }
  }

  #write(frames: readonly WndNeuz[]): void {
    const key = this.key;
    if (key === null) return;
    const applets = frames.filter((f) => this.#isApplet(f));
    if (applets.length === 0) return;
    try {
      const rec = this.read() ?? { v: REGINFO_VERSION, uiRes: [0, 0], windows: {} };
      const vp = this.#o.viewport();
      const next: RegInfoRecord = {
        v: REGINFO_VERSION,
        uiRes: [rectWidth(vp), rectHeight(vp)],
        windows: { ...rec.windows },
      };
      for (const f of applets) {
        const r = f.getWindowRect(true);
        const entry: RegInfoEntry = { rect: [r.l, r.t, r.r, r.b] };
        try {
          const extra = f.serializeRegInfo?.();
          if (extra !== undefined) {
            entry.extra = extra;
            entry.extraVersion = f.regInfoVersion;
          }
        } catch (e) {
          this.#o.log?.error('ui', `serializeRegInfo of ${String(f.appId)} failed: ${String(e)}`);
        }
        next.windows[String(f.appId)] = entry;
      }
      this.#storage.set(key, JSON.stringify(next));
    } catch (e) {
      this.#o.log?.warn('ui', `regInfo write failed: ${String(e)}`);
    }
  }
}
