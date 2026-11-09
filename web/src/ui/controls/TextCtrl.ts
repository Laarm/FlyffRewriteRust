// `TextCtrl` (`CWndText`, WndControl.cpp:2668-3470; Plan/42 §8.6, ref/22 §8): rich text wrapped to the client width,
// line pitch `cell + lineSpace` (5; chat 2), an optional 15 px scroll bar (`WBS_VSCROLL`), wheel +-1 line per notch,
// the 9-tile `WndEditTile00` skin from the frame's bake. The text is NOT editable (edits are `EditCtrl`); selection and
// copy are the browser's, enabled only with `enableClipboard` (the original's `m_bEnableClipboard`).
//
// Strings: the initial caption of the layout is literal (`m_string = strTitle`); `setString`/`addString` parse the
// markup (`SetParsingString` with the default `PS_USE_MACRO`, source wins over the plan text that calls plain
// `setString` literal); `setLiteral` keeps text as is.

import { rectHeight, rectWidth } from '../layout/geometry';
import type { ControlLayout } from '../layout/registry';
import { WBS_VSCROLL, WTYPE_TEXT } from '../layout/styles';
import { FONT, type FontSpec, lineHeight, textMetricsEpoch } from '../text/fonts';
import { layoutRich, type RichLine } from '../text/layout';
import {
  DEFAULT_TEXT_COLOR,
  deleteRange,
  parseMarkup,
  plainRuns,
  type RichRun,
  runsText,
  stripCr,
} from '../text/markup';
import { Control, type ControlViewState } from '../wnd/Control';
import { viewOf } from './bind';
import { createOwnerScrollBar, ownerScrollBarRect, type ScrollBar } from './ScrollBar';
import { wheelActive, wheelPos } from './scrollMath';
import { TextCtrlView } from './TextCtrlView';

/** `STN_CLICKED`: sent to the parent when the text was clicked (`CWndText::OnLButtonUp`). */
export const STN_CLICKED = 0;
/** The line space of a text control (`m_nLineSpace`; chat 2). */
export const TEXT_LINE_SPACE = 5;

export interface TextState extends ControlViewState {
  /** The laid-out lines of the whole text. */
  lines: readonly RichLine[];
  /** The first visible line (the scroll position with `WBS_VSCROLL`, else 0). */
  first: number;
  /** How many lines to draw from `first` (everything that intersects the client rect). */
  count: number;
  lineSpace: number;
  /** `m_bEnableClipboard`: the text can be selected and copied. */
  clipboard: boolean;
  /** `CEditString::SetAlpha` (0-255): the text opacity; the chat window fades its text to 0 after 300 s of silence. */
  alpha: number;
}

export class TextCtrl extends Control<TextState> {
  font: FontSpec = FONT.text;
  #runs: RichRun[];
  #bar: ScrollBar | null = null;
  #epoch = textMetricsEpoch.get();

  constructor(layout: ControlLayout | null = null) {
    super(WTYPE_TEXT, layout, {
      lines: [],
      first: 0,
      count: 0,
      lineSpace: TEXT_LINE_SPACE,
      clipboard: false,
      alpha: 255,
    });
    this.view = viewOf<TextCtrl>(TextCtrlView);
    this.#runs = stripCr(plainRuns(layout?.caption ?? '', DEFAULT_TEXT_COLOR));
  }

  /** The scroll bar (created with the control; visible only with `WBS_VSCROLL`). */
  get scrollBar(): ScrollBar | null {
    return this.#bar;
  }

  // ---- life cycle ---------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    const bar = createOwnerScrollBar(this);
    bar.onScroll = () => {
      this.#publish();
    };
    bar.forwardWheel = (n) => {
      this.onMouseWheel(n);
    };
    this.#bar = bar;
    bar.setVisible((this.style & WBS_VSCROLL) !== 0);
    this.#layout();
  }

  override onSize(): void {
    const bar = this.#bar;
    if (bar) {
      bar.setWndRect(ownerScrollBarRect(this));
      bar.setVisible((this.style & WBS_VSCROLL) !== 0);
    }
    this.#layout();
  }

  override process(now: number): void {
    if (this.#epoch !== textMetricsEpoch.get()) this.#layout(); // a font finished loading: re-measure
    super.process(now);
  }

  // ---- the string (CWndText API) ----------------------------------------------------------------------------

  /**
   * `SetString(text, color)`: parses the markup (`#c`, `#b`, ... and `\n`). The scroll position stays where it was
   * (clamped to the new range, `UpdateScrollBar`); `scrollToEnd()` follows a log.
   */
  setString(text: string, color: number = DEFAULT_TEXT_COLOR): void {
    this.#runs = stripCr(parseMarkup(text, color));
    this.#layout();
  }

  /** `SetParsingString`: the same as {@link setString}. */
  setParsingString(text: string, color: number = DEFAULT_TEXT_COLOR): void {
    this.setString(text, color);
  }

  /** The text kept exactly as given (`m_string = text`: no markup, `\n` stays two characters). */
  setLiteral(text: string, color: number = DEFAULT_TEXT_COLOR): void {
    this.#runs = stripCr(plainRuns(text, color));
    this.#layout();
  }

  /** `AddString`: appends (markup parsed unless `macro` is false = `PS_NOT_MACRO`); the scroll position stays. */
  addString(text: string, color: number = DEFAULT_TEXT_COLOR, macro = true): void {
    this.#runs = stripCr([...this.#runs, ...parseMarkup(text, color, macro)]);
    this.#layout();
  }

  /** Several `AddString` calls with ONE layout pass (the chat refill of a tab: up to 500 lines). */
  addStrings(items: readonly { text: string; color: number }[], macro = true): void {
    if (items.length === 0) return;
    const runs = [...this.#runs];
    for (const it of items) runs.push(...parseMarkup(it.text, it.color, macro));
    this.#runs = stripCr(runs);
    this.#layout();
  }

  /** `CEditString::SetAlpha` (0-255). */
  setAlpha(alpha: number): void {
    this.state.patch({ alpha: Math.max(0, Math.min(255, Math.round(alpha))) });
  }
  getAlpha(): number {
    return this.state.get().alpha;
  }

  /** `ResetString`: lays the text out again (after a font or size change). */
  resetString(): void {
    this.#layout();
  }

  /** The plain text (markup removed). */
  getString(): string {
    return runsText(this.#runs);
  }

  /** `m_string.DeleteLine(from, n)`: removes whole lines. */
  deleteLines(from: number, n: number): void {
    const lines = this.state.get().lines;
    if (n <= 0 || from < 0 || from >= lines.length) return;
    const last = Math.min(lines.length, from + n) - 1;
    const first = lines[from];
    const end = lines[last];
    if (!first || !end) return;
    this.#runs = deleteRange(this.#runs, first.offset, end.offset + end.length);
    this.#layout();
  }

  getLineCount(): number {
    return this.state.get().lines.length;
  }

  /** `m_bEnableClipboard`. */
  setEnableClipboard(on: boolean): void {
    this.state.patch({ clipboard: on });
  }

  setLineSpace(space: number): void {
    this.state.patch({ lineSpace: space });
    this.#layout();
  }

  /** Scrolls so that the last line is visible. */
  scrollToEnd(): void {
    this.#bar?.setMaxScrollPos();
  }

  // ---- layout and scrolling ---------------------------------------------------------------------------------

  /** The number of fully visible lines: `(clientH + lineSpace) / pitch`. FIX: the original divides by the cell only. */
  page(): number {
    const pitch = lineHeight(this.font) + this.state.get().lineSpace;
    return Math.max(
      0,
      Math.floor((rectHeight(this.rectClientValue) + this.state.get().lineSpace) / pitch),
    );
  }

  #layout(): void {
    this.#epoch = textMetricsEpoch.get();
    const lines = layoutRich(
      this.#runs,
      Math.max(0, rectWidth(this.rectClientValue)),
      this.font,
      true,
    );
    this.state.patch({ lines, text: runsText(this.#runs) });
    this.#updateScrollBar(lines.length);
    this.#publish();
  }

  /** `UpdateScrollBar`: range = line count, page = visible lines; the position never passes `max - page`. */
  #updateScrollBar(lineCount: number): void {
    const bar = this.#bar;
    if (!bar || (this.style & WBS_VSCROLL) === 0) return;
    bar.setScrollRange(0, lineCount);
    bar.setScrollPage(this.page());
    if (bar.getScrollPos() > bar.getMaxScrollPos() - bar.getScrollPage()) bar.setMaxScrollPos();
  }

  #publish(): void {
    const s = this.state.get();
    const pitch = lineHeight(this.font) + s.lineSpace;
    const scroll = (this.style & WBS_VSCROLL) !== 0 && this.#bar ? this.#bar.getScrollPos() : 0;
    this.state.patch({
      first: scroll,
      count: Math.ceil(Math.max(0, rectHeight(this.rectClientValue)) / pitch) + 1,
    });
  }

  override onMouseWheel(notches: number): void {
    const bar = this.#bar;
    if (!bar || !wheelActive(bar.getMaxScrollPos(), bar.getScrollPage())) return;
    bar.setScrollPos(
      wheelPos(
        notches,
        bar.getScrollPos(),
        bar.getMinScrollPos(),
        bar.getMaxScrollPos(),
        bar.getScrollPage(),
      ),
    );
  }

  override onLButtonUp(): void {
    this.parent?.onChildNotify(STN_CLICKED, this.id, this);
  }
}
