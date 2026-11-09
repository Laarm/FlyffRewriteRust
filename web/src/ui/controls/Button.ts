// `Button` (`CWndButton`, WndControl.cpp:276-760 and the `CTheme::RenderWndButton*` functions of ITheme.cpp:1259-1609;
// Plan/42 §8.5, ref/22 §6.6): push buttons, check boxes, radio buttons, text buttons and menu items.
//
//  * textured push button: a strip of FOUR equal frames - 0 hover, 1 normal, 2 pushed or checked, 3 disabled - drawn at
//    its natural size; a textured push button draws NO caption (the text is in the bitmap);
//  * check / radio: a strip of SIX frames - unchecked 0 hover / 1 normal, checked 2 hover / 3 normal, disabled 4
//    unchecked / 5 checked; the caption sits at `frameW + 2`, vertically centred;
//  * every press plays `SND_INF_CLICK` (`host.sound.click()`);
//  * radio groups: a click walks back to the nearest radio with `group = true`, unchecks the radios from there up to the
//    next group start, checks the clicked one (a radio without a group start before it is NOT checked, KEEP-QUIRK: 180
//    of the 193 radios of the layouts rely on their window calling `setGroup(true)`);
//  * Space and Enter in `WBS_KEY` frames press the focused / default button (`onKeyPress` / `onKeyRelease`).

import type { ControlLayout } from '../layout/registry';
import {
  WBS_CHECK,
  WBS_HIGHLIGHT,
  WBS_MENUITEM,
  WBS_NOFRAME,
  WBS_RADIO,
  WTYPE_BUTTON,
  Wnm,
} from '../layout/styles';
import type { Shortcut } from '../dnd/shortcut';
import type { FrameRects, Rect } from '../layout/geometry';
import { Control, type ControlViewState } from '../wnd/Control';
import type { WndBase } from '../wnd/WndBase';
import { viewOf } from './bind';
import { ButtonView } from './ButtonView';
import { loadSprite, peekSprite, type Sprite } from './sprites';

/** The default textures of check boxes and radio buttons (`DEF_CTRL_CHECK`, `DEF_CTRL_RADIO`). */
export const DEF_CTRL_CHECK = 'ButtCheck.bmp';
export const DEF_CTRL_RADIO = 'ButtRadio.bmp';

/** `m_nFontColor`, `m_nHighlightColor`, `m_nPushColor`, `m_nDisableColor` (ARGB). */
export const BUTTON_COLORS = {
  font: 0xff404040,
  highlight: 0xff4040ff,
  push: 0xffffff64,
  disable: 0xff8c8c8c,
} as const;

export interface ButtonState extends ControlViewState {
  /** The pointer is inside the button. */
  hover: boolean;
  /** `m_bPush`: the button is held down (and the pointer is over it) or held by Space/Enter. */
  pushed: boolean;
  /** `m_bCheck` (check boxes and radios; also the "checked" mark of a menu item). */
  checked: boolean;
  /** The sprite strip (theme file name); `null` = untextured. */
  texture: string | null;
  /** The key of a menu item, drawn at `right - 32` (`m_cHotkey`). */
  hotkey: string;
  /** The button opens a sub-menu (`m_pWndMenu`) and whether that menu is visible. */
  hasMenu: boolean;
  menuOpen: boolean;
  fontColor: number;
  highlightColor: number;
  pushColor: number;
  disableColor: number;
  /** The icon of a menu item (theme/icon path, 20 x 20). */
  icon: string | null;
}

/** The frame of a four-frame strip: disabled 3; plain 0 (highlight) / 1; pushed or checked 2. */
export function frame4(
  enabled: boolean,
  push: boolean,
  checked: boolean,
  highlight: boolean,
): number {
  if (!enabled) return 3;
  if (!push && !checked) return highlight ? 0 : 1;
  return 2;
}

/** The frame of a six-frame strip (check / radio). */
export function frame6(
  enabled: boolean,
  push: boolean,
  checked: boolean,
  highlight: boolean,
): number {
  if (!enabled) return !push && !checked ? 4 : 5;
  if (!push && !checked) return highlight ? 0 : 1;
  return highlight ? 2 : 3;
}

/** `CTheme::GetButtonFontColor`. */
export function buttonFontColor(s: ButtonState, highlight: boolean): number {
  if (!s.enabled) return s.disableColor;
  if (highlight) return s.highlightColor;
  return s.pushed ? s.pushColor : s.fontColor;
}

export class Button extends Control<ButtonState> {
  /** `m_shortcut`: what a dragged menu item carries (`SHORTCUT_APPLET` ...). */
  shortcut: Shortcut | null = null;
  /** The pointer entered the button (a menu opens the sub-menu of the item under the pointer). */
  onHoverEnter: (() => void) | null = null;
  /** The sub-menu window of a menu item (`SetMenu`). */
  #menu: WndBase | null = null;
  #down = false;
  #keyHeld = false;

  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_BUTTON, layout, {
      hover: false,
      pushed: false,
      checked: false,
      texture: layout?.texture === undefined || layout.texture === '' ? null : layout.texture,
      hotkey: '',
      hasMenu: false,
      menuOpen: false,
      fontColor: BUTTON_COLORS.font,
      highlightColor: BUTTON_COLORS.highlight,
      pushColor: BUTTON_COLORS.push,
      disableColor: BUTTON_COLORS.disable,
      icon: null,
    });
    this.view = viewOf<Button>(ButtonView);
  }

  /** `CWndButton::Create` adds `WBS_NOFRAME`: the client rect is the window rect (also for code-created buttons). */
  protected override computeRects(window: Rect): FrameRects {
    const copy = (): Rect => ({ ...window });
    return { window: copy(), client: copy(), layout: copy() };
  }

  override onInitialUpdate(): void {
    this.addWndStyle(WBS_HIGHLIGHT | WBS_NOFRAME); // `Create`: `dwStyle |= WBS_HIGHLIGHT`, `| WBS_NOFRAME`
    // `Create` gives check boxes and radios their default strip; a layout texture replaces it afterwards
    if (this.state.get().texture === null) {
      if ((this.style & WBS_CHECK) !== 0) this.state.patch({ texture: DEF_CTRL_CHECK });
      else if ((this.style & WBS_RADIO) !== 0) this.state.patch({ texture: DEF_CTRL_RADIO });
    }
  }

  // ---- API (CWndButton) ---------------------------------------------------------------------------------------

  /** `SetCheck` (0 / 1). */
  setCheck(check: number | boolean): void {
    this.state.patch({ checked: check === true || (typeof check === 'number' && check !== 0) });
  }
  /** `GetCheck`: 1 when checked, else 0. */
  getCheck(): number {
    return this.state.get().checked ? 1 : 0;
  }
  /** `SetGroup` (`m_bGroup`: the first radio of a group). */
  setGroup(group: boolean): void {
    this.group = group;
  }
  isGroup(): boolean {
    return this.group;
  }
  /** `SetTexture`: the sprite strip (a theme file name), or `null` for the untextured look. */
  setTexture(path: string | null): void {
    this.state.patch({ texture: path });
  }
  /**
   * `FitTextureSize`: the window takes the size of ONE frame of the strip (width / 4, or / 6 for check and radio;
   * the height unchanged). Applied at once when the image is already decoded, else when it arrives; the promise
   * resolves after that (no-op without a texture).
   */
  fitTextureSize(): Promise<void> {
    const tex = this.state.get().texture;
    if (tex === null) return Promise.resolve();
    const apply = (sprite: Sprite | null | 'missing'): void => {
      if (sprite === null || sprite === 'missing' || this.destroyed) return;
      const frames = (this.style & (WBS_RADIO | WBS_CHECK)) !== 0 ? 6 : 4;
      const r = this.rectWindow;
      this.setWndRect({ l: r.l, t: r.t, r: r.l + Math.floor(sprite.width / frames), b: r.b });
    };
    const cached = peekSprite(this.env, tex);
    if (cached !== undefined) {
      apply(cached);
      return Promise.resolve();
    }
    return loadSprite(this.env, tex).then(apply);
  }
  setFontColor(color: number): void {
    this.state.patch({ fontColor: color >>> 0 });
  }
  setHighlightColor(color: number): void {
    this.state.patch({ highlightColor: color >>> 0 });
  }
  setPushColor(color: number): void {
    this.state.patch({ pushColor: color >>> 0 });
  }
  setDisableColor(color: number): void {
    this.state.patch({ disableColor: color >>> 0 });
  }
  /** `m_cHotkey` of a menu item. */
  setHotkey(key: string): void {
    this.state.patch({ hotkey: key });
  }
  /** The icon of a menu item (`Icon/...`, drawn 20 x 20). */
  setIcon(path: string | null): void {
    this.state.patch({ icon: path });
  }
  /** `SetMenu`: this menu item opens `menu` (a `Menu`); the arrow shows and follows its visibility. */
  setMenu(menu: WndBase | null): void {
    this.#menu = menu;
    this.state.patch({ hasMenu: menu !== null });
  }
  get menu(): WndBase | null {
    return this.#menu;
  }
  /** The sub-menu opened or closed (the arrow of a menu item shows it). */
  setMenuOpen(open: boolean): void {
    this.state.patch({ menuOpen: open });
  }

  /** `IsPush()`: held down by the pointer or by a key. */
  isPush(): boolean {
    return this.#down || this.#keyHeld;
  }
  /** `IsHighLight()`: the pointer is over an enabled button and no shortcut is being dragged (`hover` knows). */
  isHighLight(): boolean {
    const s = this.state.get();
    return (this.style & WBS_HIGHLIGHT) !== 0 && s.hover && s.enabled;
  }

  // ---- pointer ------------------------------------------------------------------------------------------------

  override onMouseEnter(): void {
    this.state.patch({
      hover: !this.env.wm.isDraggingShortcut(),
      pushed: this.#down || this.#keyHeld,
    });
    this.onHoverEnter?.();
  }
  override onMouseLeave(): void {
    this.state.patch({ hover: false, pushed: this.#keyHeld });
  }
  override onLButtonDown(): void {
    this.#down = true;
    this.state.patch({ hover: true, pushed: true });
    this.env.host.sound?.click(); // PLAYSND(SND_INF_CLICK)
    // dragging a menu item (pushed + move) starts a `SHORTCUT_APPLET` drag: start menu -> taskbar
    if ((this.style & WBS_MENUITEM) !== 0 && this.#menu === null && this.shortcut !== null) {
      this.env.wm.drag.arm(this, () => this.dragPayload());
    }
  }
  /**
   * `WM_LBUTTONDBLCLK` (a second press within 500 ms): the original sets `m_bPush` for it as well, so the release that
   * follows is a click again - rapid clicks on a `+` button all count. (`CWndButton::OnLButtonDblClk` itself is empty;
   * the sound and the menu-item drag belong to the plain press.)
   */
  override onLButtonDblClk(): void {
    this.#down = true;
    this.state.patch({ hover: true, pushed: true });
  }
  /** What a dragged menu item carries (the armed drag builds it at the first pointer move). */
  protected dragPayload(): Shortcut | null {
    if (this.shortcut === null) return null;
    const icon = this.state.get().icon;
    return {
      ...this.shortcut,
      id: this.id,
      from: this,
      text: this.getTitle(),
      ...(icon === null ? {} : { icon }),
    };
  }
  override onMouseMove(): void {
    // the same drag from the hover path (the press may not have armed it: a key press)
    if (
      (this.style & WBS_MENUITEM) !== 0 &&
      this.#menu === null &&
      this.isPush() &&
      this.shortcut !== null &&
      !this.env.wm.isDraggingShortcut()
    ) {
      const sc = this.dragPayload();
      if (sc) this.env.wm.drag.begin(sc);
    }
  }
  override onPointerRelease(): void {
    this.#down = false;
    this.state.patch({ pushed: this.#keyHeld });
  }

  /** `CWndButton::OnLButtonUp`: a release of a pushed button is a click. */
  override onLButtonUp(): void {
    if (!this.isPush()) return;
    this.#down = false;
    this.state.patch({ pushed: false });
    this.#click();
  }

  override onKeyPress(): void {
    if (this.#keyHeld) return;
    this.#keyHeld = true;
    this.state.patch({ pushed: true });
    this.env.host.sound?.click();
  }
  override onKeyRelease(): void {
    if (!this.#keyHeld) return;
    this.#down = true; // `IsPush()` of the click below
    this.#keyHeld = false;
    this.state.patch({ pushed: false });
    this.#click();
    this.#down = false;
  }

  /** The click: toggle a check box, notify the parent, then run the radio group (the original's order). */
  #click(): void {
    if ((this.style & WBS_CHECK) !== 0) this.setCheck(this.getCheck() === 0);
    this.parent?.onChildNotify(Wnm.CLICKED, this.id, this);
    if ((this.style & WBS_RADIO) !== 0) this.#checkRadioGroup();
  }

  #checkRadioGroup(): void {
    const kids = this.parent?.childWnds ?? [];
    const idx = kids.indexOf(this);
    for (let i2 = idx; i2 >= 0; i2 -= 1) {
      const start = kids[i2];
      if (start instanceof Button && start.isGroup() && start.isWndStyle(WBS_RADIO)) {
        for (let i3 = i2; i3 < kids.length; i3 += 1) {
          const o = kids[i3];
          if (i3 > i2 && o instanceof Button && o.isGroup()) break;
          if (o instanceof Button && o.isWndStyle(WBS_RADIO)) o.setCheck(0);
          else break;
        }
        this.setCheck(1);
        return;
      }
    }
  }

  /** `OnDropIcon`: a dropped shortcut is reported to the parent as `WIN_ITEMDROP`. */
  override onDropIcon(sc: Shortcut): boolean {
    return this.parent?.onChildNotify(Wnm.WIN_ITEMDROP, this.id, sc) ?? false;
  }

  override process(now: number): void {
    // the highlight is off while a shortcut is dragged (no pointer event announces the start of a drag)
    if (this.state.get().hover && this.env.wm.isDraggingShortcut())
      this.state.patch({ hover: false });
    super.process(now);
  }
}
