// `EditCtrl` (`CWndEdit`, WndEditCtrl.cpp; Plan/42 §8.7, ref/22 §8): a native `<input>` (`<textarea>` with
// `EBS_MULTILINE`) over the baked `WndEditTile00` skin. The browser gives caret, selection, clipboard and IME; the
// control keeps the original's semantics:
//  * `EBS_PASSWORD` masks, `EBS_NUMBER` accepts digits only (and a `maxLength` as `SetMaxStringNumber`),
//    `EBS_UPPERCASE` / `EBS_LOWERCASE` convert, `EBS_READONLY` is read-only, `setMaxLength(n)`;
//  * `EN_CHANGE` on user input (never on `setString`), `EN_RETURN` on Enter (with `EBS_WANTRETURN` the newline is
//    inserted first), `EN_UP` on ArrowUp (the parent answers `true` when it replaced the text: the caret goes to the end);
//  * Esc does nothing inside an edit; F1-F9 are `preventDefault`ed and left to the game (the edit does not consume them,
//    `consumesKey`); Tab in a `WBS_KEY` frame moves to the next tab stop through the window manager.
// The DOM element is attached by the view (`attach`); the control focuses and blurs it when the UI focus moves.

import type { ControlLayout } from '../layout/registry';
import {
  EBS_LOWERCASE,
  EBS_MULTILINE,
  EBS_NUMBER,
  EBS_PASSWORD,
  EBS_READONLY,
  EBS_UPPERCASE,
  EBS_WANTRETURN,
  WBS_KEY,
  WTYPE_EDITCTRL,
  Wnm,
} from '../layout/styles';
import { escapesEdit } from '../wm/keyContract';
import { Control, type ControlViewState } from '../wnd/Control';
import type { WndBase } from '../wnd/WndBase';
import { viewOf } from './bind';
import { EditCtrlView } from './EditCtrlView';

export interface EditState extends ControlViewState {
  /** The text of the edit (kept in step with the DOM element). */
  value: string;
  /** `maxLength` of the element; 0 = unlimited. */
  maxLength: number;
}

type Field = HTMLInputElement | HTMLTextAreaElement;

/** What an `EBS_*` mode does to a candidate value (`beforeinput`, paste, IME, `setString`). */
export function sanitizeEditValue(style: number, value: string, maxLength: number): string {
  let v = value;
  if ((style & EBS_NUMBER) !== 0) v = v.replace(/[^0-9]/g, '');
  if ((style & EBS_UPPERCASE) !== 0) v = v.toUpperCase();
  else if ((style & EBS_LOWERCASE) !== 0) v = v.toLowerCase();
  if ((style & EBS_MULTILINE) === 0) v = v.replace(/[\r\n]/g, '');
  if (maxLength > 0) v = Array.from(v).slice(0, maxLength).join('');
  return v;
}

export class EditCtrl extends Control<EditState> {
  #el: Field | null = null;
  /** An Enter that must report `EN_RETURN` after the browser inserted its newline (`EBS_WANTRETURN`). */
  pendingReturn = false;

  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_EDITCTRL, layout, { value: '', maxLength: 0 });
    this.view = viewOf<EditCtrl>(EditCtrlView);
  }

  /** The DOM element the view mounted (`null` until it did, and in Node tests). */
  get element(): Field | null {
    return this.#el;
  }
  attach(el: Field | null): void {
    this.#el = el;
    // the UI focus was put on this edit before its element was mounted (a window that focuses its edit while it is built): the DOM focus follows now
    if (
      el &&
      this.isCreated &&
      this.env.wm.curFocus === this &&
      (this.style & EBS_READONLY) === 0
    ) {
      el.focus();
    }
  }

  // ---- API (CWndEdit / CWndText) ----------------------------------------------------------------------------

  /** `SetString`: replaces the text; programmatic changes send no `EN_CHANGE` (the original does not either). */
  setString(text: string): void {
    const v = sanitizeEditValue(this.style, text, this.state.get().maxLength);
    this.state.patch({ value: v });
    if (this.#el && this.#el.value !== v) this.#el.value = v;
  }
  getString(): string {
    return this.state.get().value;
  }
  /** `Empty`. */
  empty(): void {
    this.setString('');
  }
  /** `SetMaxStringNumber` / the `maxLength` of the element (0 = unlimited). */
  setMaxLength(n: number): void {
    this.state.patch({ maxLength: Math.max(0, Math.floor(n)) });
    const v = this.state.get().value;
    const cut = sanitizeEditValue(this.style, v, Math.max(0, Math.floor(n)));
    if (cut !== v) this.setString(cut);
  }
  setMaxStringNumber(n: number): void {
    this.setMaxLength(n);
  }
  /** Puts the caret at the end of the text (`OnKeyEnd`). */
  caretToEnd(): void {
    const el = this.#el;
    if (!el) return;
    const n = el.value.length;
    el.setSelectionRange(n, n);
  }
  selectAll(): void {
    this.#el?.select();
  }

  /** The edit keeps every key but Esc and F1-F9 (the typing rule of Plan/42 §5.6). */
  consumesKey(code: string): boolean {
    return !escapesEdit(code);
  }

  // ---- focus: the UI focus and the DOM focus follow each other -----------------------------------------------

  override onSetFocus(): void {
    if ((this.style & EBS_READONLY) !== 0) return; // `CWndEdit::OnSetFocus` returns for read-only edits (VER15)
    this.#el?.focus();
  }
  override onKillFocus(newWnd: WndBase | null): void {
    super.onKillFocus(newWnd);
    const el = this.#el;
    if (el && typeof document !== 'undefined' && document.activeElement === el) el.blur();
  }

  // ---- the events of the element (called by the view) --------------------------------------------------------

  /** The DOM took the focus by itself (a click on the element, a touch): the UI focus follows. */
  domFocus(): void {
    if (this.env.wm.curFocus !== this) this.setFocus();
  }

  /** `input` / `change` of the element with the candidate value; sends `EN_CHANGE` when the text changed. */
  domInput(value: string): void {
    const cleaned = sanitizeEditValue(this.style, value, this.state.get().maxLength);
    const el = this.#el;
    if (el && el.value !== cleaned) el.value = cleaned;
    const changed = this.state.patch({ value: cleaned });
    if (changed) this.parent?.onChildNotify(Wnm.EN_CHANGE, this.id, this);
    if (this.pendingReturn) {
      this.pendingReturn = false;
      this.parent?.onChildNotify(Wnm.EN_RETURN, this.id, this);
    }
  }

  /**
   * `keydown` of the element. Returns `true` when the event's default must be prevented (Enter without
   * `EBS_WANTRETURN`, handled ArrowUp, F1-F9, Tab of a `WBS_KEY` frame).
   */
  domKeyDown(code: string, shift: boolean): boolean {
    if (code === 'Enter' || code === 'NumpadEnter') {
      const multi = (this.style & EBS_MULTILINE) !== 0;
      const want = (this.style & EBS_WANTRETURN) !== 0;
      if (multi && want) {
        // the browser inserts the newline, `EN_RETURN` follows the `input` event (the original inserts first)
        this.pendingReturn = true;
        return false;
      }
      this.parent?.onChildNotify(Wnm.EN_RETURN, this.id, this);
      return true;
    }
    if (code === 'ArrowUp') {
      const handled = this.parent?.onChildNotify(Wnm.EN_UP, this.id, this) ?? false;
      if (handled) this.caretToEnd();
      return handled;
    }
    if (/^F[1-9]$/.test(code)) return true; // keep the browser's own F-key actions away; the router sees the key too
    if (code === 'Tab' && this.getFrameWnd()?.isWndStyle(WBS_KEY) === true) {
      this.env.wm.keyDown('Tab', { shift, ctrl: false, alt: false });
      return true;
    }
    return false;
  }

  /** The style bits the element depends on (read by the view). */
  isPassword(): boolean {
    return (this.style & EBS_PASSWORD) !== 0;
  }
}
