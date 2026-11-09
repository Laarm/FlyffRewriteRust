// `IconStrip` (WP-UI-28; `CWndWorld::RenderBuffIcon` / `CWndBuffStatus::OnDraw`, ref/22 §17 C): a display-only control that paints
// independently positioned pictures - the buff icons of the world strip and of the status window, the material pictures of the
// summon window, the icon rows of the duration list. Each entry is one image at (x, y) with a size, an alpha, an optional flash
// (the CSS animation `ui-strip-flash`: alpha 64..192 in steps of 6 per 60 Hz frame, `m_nAlpha` of the original), an optional
// multiply tint (the evil-buff colour (255, 120, 255)) and text items (timers) in the layer's own coordinates. The window computes
// the entries from its view and `setEntries` diffs them: an unchanged list emits nothing.

import { WTYPE_CUSTOM } from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { IconStripView } from './IconStripView';
import type { TextItem } from './TextLayer';

export interface StripEntry {
  /** A stable identity (the React key). */
  readonly key: string;
  /** The image file under the resource root (`icon/skill_x.dds`, `item/itm_x.dds`); `null` draws only the texts. */
  readonly icon: string | null;
  readonly x: number;
  readonly y: number;
  /** The drawn size (32 x 32 for the strip, 24 for the target row). */
  readonly w: number;
  readonly h: number;
  /** `0..255` of the `RenderTexture` call (192 for item buffs, 255 skills). */
  readonly alpha: number;
  /** The icon pulses (`< 20 s` left). */
  readonly flash?: boolean;
  /** Multiplied into the picture, `[r, g, b]`; `null` = none. */
  readonly tint?: readonly [number, number, number] | null;
  /** Texts over the picture, in the strip's coordinates. */
  readonly texts?: readonly TextItem[];
}

export interface IconStripState extends ControlViewState {
  entries: readonly StripEntry[];
}

function sameTexts(
  a: readonly TextItem[] | undefined,
  b: readonly TextItem[] | undefined,
): boolean {
  const p = a ?? [];
  const q = b ?? [];
  if (p.length !== q.length) return false;
  for (let i = 0; i < p.length; i += 1) {
    const x = p[i];
    const y = q[i];
    if (x?.x !== y?.x || x?.y !== y?.y || x?.text !== y?.text) return false;
    if (x?.color !== y?.color || x?.font !== y?.font) return false;
  }
  return true;
}

/** Element-wise equality of two entry lists. */
export function sameEntries(a: readonly StripEntry[], b: readonly StripEntry[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const p = a[i];
    const q = b[i];
    if (!p || !q) return false;
    if (p.key !== q.key || p.icon !== q.icon || p.x !== q.x || p.y !== q.y) return false;
    if (p.w !== q.w || p.h !== q.h || p.alpha !== q.alpha || p.flash !== q.flash) return false;
    const tp = p.tint ?? null;
    const tq = q.tint ?? null;
    if ((tp === null) !== (tq === null)) return false;
    if (tp && tq && (tp[0] !== tq[0] || tp[1] !== tq[1] || tp[2] !== tq[2])) return false;
    if (!sameTexts(p.texts, q.texts)) return false;
  }
  return true;
}

export class IconStrip extends Control<IconStripState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { entries: [] });
    this.view = viewOf<IconStrip>(IconStripView);
  }

  /** Replaces the entries; returns `false` (and emits nothing) when they are the same as before. */
  setEntries(entries: readonly StripEntry[]): boolean {
    if (sameEntries(this.state.get().entries, entries)) return false;
    return this.state.patch({ entries });
  }

  get entries(): readonly StripEntry[] {
    return this.state.get().entries;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}
