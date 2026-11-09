// `CommandWnd` (`CWndCommand : CWndListBox`, WndCommand.cpp; Plan/42 §13.4): the command auto-complete list above the chat
// edit. A top-most frame without caption or skin around one `ListBox` (`WBS_VSCROLL`): the names come from the
// `CommandListView` port (the commands the issuer may use, sorted), 160 px wide and 5 to 20 rows high. The chat window
// owns it: it keeps the keyboard, selects by prefix while the user types and reads the selection on Enter; a double
// click reports `APP_COMMAND` / `DBLCLK` to the chat window.

import { APP_COMMAND } from '../../gen/defs/resdata';
import { ListBox } from '../controls/ListBox';
import { type FrameRects, type Rect, rectHeight, rectWidth } from '../layout/geometry';
import { WBS_NOFRAME, WBS_TOPMOST, WBS_VSCROLL, Wnm } from '../layout/styles';
import type { BakeSpec } from '../skin/baker';
import type { Shortcut } from '../dnd/shortcut';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';

/** Fixed width of the list (`CRect(0, 0, 160, ...)`). */
export const COMMAND_LIST_WIDTH = 160;
/** The row count is clamped into this range (`nNum`). */
export const COMMAND_ROWS_MIN = 5;
export const COMMAND_ROWS_MAX = 20;

/** The window height for `n` commands: `rows * pitch + 8`. */
export function commandListHeight(n: number, pitch: number): number {
  const rows = Math.min(COMMAND_ROWS_MAX, Math.max(COMMAND_ROWS_MIN, n));
  return rows * pitch + 8;
}

export class CommandWnd extends WndNeuz {
  readonly appId = APP_COMMAND;
  readonly #names: readonly string[];
  /** `m_pWndEdit`: the edit gets the keyboard back when the list was built. */
  readonly #edit: WndBase;
  list!: ListBox;
  /** Called once when the window was removed (the owner forgets it: `OnDestroyChildWnd`). */
  onClosed: ((w: CommandWnd) => void) | null = null;

  constructor(names: readonly string[], edit: WndBase) {
    super();
    this.#names = names;
    this.#edit = edit;
  }

  /** `Initialize`: `WBS_TOPMOST | WBS_VSCROLL` child of the chat window, no frame. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return this.create(
      WBS_TOPMOST | WBS_NOFRAME | extraStyle,
      { l: 0, t: 0, r: COMMAND_LIST_WIDTH, b: 100 },
      parent,
      APP_COMMAND,
    );
  }

  /** The window is the list: client = window. */
  protected override computeRects(window: Rect): FrameRects {
    return { window: { ...window }, client: { ...window }, layout: { ...window } };
  }

  override onInitialUpdate(): void {
    // no layout, no title bar: the frame is the list
    const list = new ListBox();
    this.list = this.addControl(
      list,
      1,
      { l: 0, t: 0, r: COMMAND_LIST_WIDTH, b: 100 },
      WBS_VSCROLL,
    );
    list.setSkin('WndEditTile00.tga', true);
    for (const n of this.#names) list.addString(n);
    list.sortListBox();
    const h = commandListHeight(this.#names.length, list.pitch());
    this.setWndRect({ l: 0, t: 0, r: COMMAND_LIST_WIDTH, b: h }, true);
    // the edit keeps the keyboard (`m_pWndEdit->SetFocus()` at the end of `OnInitialUpdate`)
    this.#edit.setFocus();
  }

  override onSize(type: number, cx: number, cy: number): void {
    this.list.setWndRect(
      { l: 0, t: 0, r: rectWidth(this.rectClientValue), b: rectHeight(this.rectClientValue) },
      true,
    );
    super.onSize(type, cx, cy);
  }

  /** The list is the only skin: the 9-tile edit background. */
  override buildBakeSpec(): BakeSpec | null {
    const r = this.rectWindowValue;
    const width = rectWidth(r);
    const height = rectHeight(r);
    if (width <= 0 || height <= 0) return null;
    return { width, height, ops: this.list.skinOps() };
  }

  /** The text of the selected row, `null` for none. */
  selectedName(): string | null {
    const i = this.list.getCurSel();
    return i < 0 ? null : this.list.getText(i);
  }

  /** `w` is this window or one of its controls. */
  contains(w: WndBase | null): boolean {
    return w !== null && (w === this || w === this.list || w.getFrameWnd() === this);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean;
  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === this.list.id) {
      if (msg === Wnm.DBLCLK) {
        // `CWndCommand::OnLButtonDblClk`: the notification goes to the owner with the id of the list (`APP_COMMAND`)
        this.parent?.onChildNotify(Wnm.DBLCLK, APP_COMMAND, this);
        return true;
      }
      if (msg === Wnm.SELCHANGE) {
        // `CWndCommand::OnLButtonUp`: the keyboard goes back to the chat window
        this.parent?.setFocus();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** `OnKillFocus(pNewWnd)`: the list closes when the focus goes anywhere but its owner. */
  override onKillFocus(newWnd: WndBase | null): void {
    super.onKillFocus(newWnd);
    if (newWnd !== this.parent && !this.destroyed) this.destroy();
  }

  override onDestroy(): void {
    this.onClosed?.(this);
    super.onDestroy();
  }
}
