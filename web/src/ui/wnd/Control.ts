// `Control`: the base of every child control of a frame (Plan/42 §4.1, §8; `CWndStatic`, `CWndButton`, ... of
// WndControl.cpp collapse into subclasses of this class, WP-UI-05..07). WP-UI-03 provides the base: rects, the
// observable view state, the skin contribution of the control and the placeholder view; the behaviour of the
// concrete kinds is added by the control work packages through `createControlOfKind` (controls/factory.ts).

import type { ComponentType } from 'react';
import {
  controlClientRect,
  type FrameRects,
  frameRects,
  type Point,
  type Rect,
  rectEq,
  rectOffset,
} from '../layout/geometry';
import type { ControlLayout } from '../layout/registry';
import {
  type ControlKind,
  WBS_NODRAWFRAME,
  WSS_GROUPBOX,
  WSS_PICTURE,
  WTYPE_STATIC,
} from '../layout/styles';
import { controlSkinOp, type SkinOp } from '../skin/baker';
import { ControlState } from './observable';
import { type PointerInfo, WndBase } from './WndBase';

/** The state every control view renders (the part shared by all kinds). */
export interface ControlViewState {
  /** The window rect in the parent's client coordinates. */
  rect: Rect;
  visible: boolean;
  enabled: boolean;
  /** The caption / initial string of statics, buttons and texts. */
  text: string;
  tooltip: string;
  /** `m_dwStyle` (the WBS_/WSS_/EBS_ bits the view depends on; republished by `addWndStyle`/`delWndStyle`). */
  style: number;
}

/** What a control view component receives. */
export interface ControlViewProps {
  control: Control;
}

export class Control<S extends ControlViewState = ControlViewState> extends WndBase {
  readonly kind: ControlKind;
  /** The layout row this control was created from (`null` for code-created controls). */
  readonly layout: ControlLayout | null;
  /** `WIDC_*` name (`''` for code-created controls). */
  readonly name: string;
  /** Observable view state; setters write through it and unchanged writes emit nothing. */
  readonly state: ControlState<S>;
  /** `m_strTexture` / `m_bTile`: what the skin baker paints for this control (initial: the layout's). */
  skinTexture: string;
  skinTile: boolean;
  /** `m_bGroup`: starts a radio group (buttons). */
  group = false;
  /** The React component that renders this control (kinds replace it). */
  view: ComponentType<ControlViewProps> | null = null;

  /** @param extra the fields a subclass adds to the view state (`S` minus the shared part) */
  constructor(
    kind: ControlKind,
    layout: ControlLayout | null = null,
    extra?: Omit<S, keyof ControlViewState>,
  ) {
    super();
    this.kind = kind;
    this.layout = layout;
    this.name = layout?.name ?? '';
    this.skinTexture = layout?.texture ?? '';
    this.skinTile = layout?.tile ?? false;
    this.group = layout?.group ?? false;
    this.state = new ControlState<S>({
      rect: { l: 0, t: 0, r: 0, b: 0 },
      visible: true,
      enabled: true,
      text: layout?.caption ?? '',
      tooltip: layout?.tooltip ?? '',
      style: 0,
      ...extra,
    } as S);
    this.toolTip = layout?.tooltip ?? '';
  }

  /** The client rect follows the control kind (`SetWndRect` overrides of the control classes, §3.4). */
  protected override computeRects(window: Rect): FrameRects {
    const client = controlClientRect(this.kind, this.style, window);
    const base = frameRects(window, this.style);
    return { window: base.window, client, layout: client };
  }

  protected override onRectChanged(): void {
    const cur = this.state.get().rect;
    if (!rectEq(cur, this.rectWindowValue))
      this.state.patch({ rect: { ...this.rectWindowValue } } as Partial<S>);
    // `create()` ORs the style in before the first `setWndRect`, so this publishes the final bits
    if (this.state.get().style !== this.style)
      this.state.patch({ style: this.style } as Partial<S>);
  }

  override addWndStyle(bits: number): void {
    super.addWndStyle(bits);
    this.state.patch({ style: this.style } as Partial<S>);
  }
  override delWndStyle(bits: number): void {
    super.delWndStyle(bits);
    this.state.patch({ style: this.style } as Partial<S>);
  }
  protected override onVisibleChanged(): void {
    this.state.patch({ visible: this.visibleValue } as Partial<S>);
  }
  protected override onEnabledChanged(): void {
    this.state.patch({ enabled: this.enabledValue } as Partial<S>);
  }

  /** `SetTitle`: the caption text. */
  setTitle(text: string): void {
    this.state.patch({ text } as Partial<S>);
  }
  getTitle(): string {
    return this.state.get().text;
  }

  override setToolTip(text: string, pos: 0 | 1 | 2 | 3 = 0): void {
    super.setToolTip(text, pos);
    this.state.patch({ tooltip: text } as Partial<S>);
  }

  /**
   * `CWndStatic::IsPickupSpace`: group boxes and picture statics are never hit; a plain static is hit over its
   * whole rect (its press focuses the parent frame and never starts a window move). Other kinds: the base rule.
   */
  override isPickupSpace(ptWindow: Point): boolean {
    if (this.kind === WTYPE_STATIC) {
      if (this.isWndStyle(WSS_GROUPBOX | WSS_PICTURE)) return false;
      const r = this.getWindowRect(false);
      return ptWindow.x >= r.l && ptWindow.x < r.r && ptWindow.y >= r.t && ptWindow.y < r.b;
    }
    return super.isPickupSpace(ptWindow);
  }

  /** A press or release on a static focuses its parent frame (`CWndStatic::OnLButtonDown/Up`). */
  // overload + shorter implementation: the control classes override with the pointer arguments they need
  override onLButtonDown(pt?: Point, info?: PointerInfo): void;
  override onLButtonDown(): void {
    if (this.kind === WTYPE_STATIC) this.parent?.setFocus();
  }
  override onLButtonUp(pt?: Point, info?: PointerInfo): void;
  override onLButtonUp(): void {
    if (this.kind === WTYPE_STATIC) this.parent?.setFocus();
  }

  /** The owning frame re-bakes its skin (a skin texture changed, a tab was selected, ...). */
  requestBake(): void {
    this.getFrameWnd()?.requestBake();
  }

  /** Changes the skin texture/tile of the control and re-bakes the frame. */
  setSkin(texture: string, tile: boolean): void {
    if (this.skinTexture === texture && this.skinTile === tile) return;
    this.skinTexture = texture;
    this.skinTile = tile;
    this.requestBake();
  }

  /**
   * The skin ops this control contributes to its frame's bake (`AdditionalSkinTexture`, Plan/42 §7.2). The base
   * paints the kind's default/layout skin at the control's current rect; tab controls override it.
   */
  skinOps(): SkinOp[] {
    // `CWndBase::AdditionalSkinTexture` returns for `WBS_NODRAWFRAME` (WndBase.cpp:2526): the chat text area
    if (!this.visibleValue || this.destroyed || (this.style & WBS_NODRAWFRAME) !== 0) return [];
    const parent = this.parent;
    if (!parent) return [];
    // frame-window coordinates: the parent's client offset added to the control rect (WndBase.cpp:2536-2545)
    const o = parent.isRoot
      ? { x: 0, y: 0 }
      : {
          x: parent.rectClient.l - parent.rectWindow.l,
          y: parent.rectClient.t - parent.rectWindow.t,
        };
    const rect = rectOffset(this.rectWindowValue, o.x, o.y);
    const op = controlSkinOp(this.kind, this.style, this.skinTexture, this.skinTile, rect);
    return op ? [op] : [];
  }
}
