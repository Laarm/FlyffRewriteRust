// `SpriteLayer` (WP-UI-38; the `OnDraw` of `CWndSkillTreeEx`, `src:_Interface/WndField.cpp:8078-8320`): a display-only control that paints
// independently positioned pictures - whole theme/icon files scaled to a size (the skill icons, 27 x 27 from a 32 x 32 file), or a
// rectangle cut out of a sprite sheet at natural size (the level numbers of `icon_IconSkillLevel`, the up/down arrows). The window
// computes the entries from its view and `setEntries` diffs them: an unchanged list emits nothing. Entries are painted in list
// order (later ones on top); the control never takes the pointer.

import { WTYPE_CUSTOM } from '../layout/styles';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { SpriteLayerView } from './SpriteLayerView';

export interface SpriteEntry {
  /** A stable identity (the React key). */
  readonly key: string;
  /** The image file (`icon/skill_x.dds`, `theme/lvup.bmp`, the texture of an atlas); `null` draws nothing. */
  readonly path: string | null;
  readonly x: number;
  readonly y: number;
  /** The drawn size; with `src` the size of the cut-out too (natural scale). */
  readonly w: number;
  readonly h: number;
  /** A rectangle of the image at natural scale (or at `scale`); absent = the whole image scaled to `w x h`. */
  readonly src?: { readonly x: number; readonly y: number };
  /**
   * The zoom of a cut-out (`Render(..., fScaleX, fScaleY)`: the element icons of the attribute window are 16 x 16 sprites drawn at 1.5);
   * `w` and `h` are then the SCALED size of the cut-out. Default 1.
   */
  readonly scale?: number;
  /** `0..1`, default 1. */
  readonly alpha?: number;
}

export interface SpriteLayerState extends ControlViewState {
  entries: readonly SpriteEntry[];
}

/** Element-wise equality of two entry lists. */
export function sameSprites(a: readonly SpriteEntry[], b: readonly SpriteEntry[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const p = a[i];
    const q = b[i];
    if (!p || !q) return false;
    if (p.key !== q.key || p.path !== q.path || p.x !== q.x || p.y !== q.y) return false;
    if (p.w !== q.w || p.h !== q.h || (p.alpha ?? 1) !== (q.alpha ?? 1)) return false;
    if (p.src?.x !== q.src?.x || p.src?.y !== q.src?.y || (p.scale ?? 1) !== (q.scale ?? 1))
      return false;
  }
  return true;
}

export class SpriteLayer extends Control<SpriteLayerState> {
  constructor() {
    super(WTYPE_CUSTOM, null, { entries: [] });
    this.view = viewOf<SpriteLayer>(SpriteLayerView);
  }

  /** Replaces the entries; returns `false` (and emits nothing) when they are the same as before. */
  setEntries(entries: readonly SpriteEntry[]): boolean {
    if (sameSprites(this.state.get().entries, entries)) return false;
    return this.state.patch({ entries });
  }

  get entries(): readonly SpriteEntry[] {
    return this.state.get().entries;
  }

  /** A display: presses and drops pass to the window below. */
  override isPickupSpace(): boolean {
    return false;
  }
}
