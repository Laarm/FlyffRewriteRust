// `CommandHelpWnd` (BFIX1 #006): the window of the browser's `/help`. The server's `/help` sent one green centre notice per command, of
// which the centre keeps the last 6 (`CENTRE_MAX_LINES`); the browser now answers the command itself (`features/chat/helpCommand.ts`) and
// shows the whole list here: a normal captioned frame, movable, with one `ListBox` (`WBS_VSCROLL`) that holds the header and one row per
// command in the given order. The wheel and the scroll bar scroll it; the title-bar close button and Esc close it (it is neither top-most
// nor modal, so `WindowManager.topClosableWindow` finds it). It has no layout row (`APP_*` ids are the layouts'): the private id is
// {@link APP_COMMAND_HELP}, the frame is created by code and not through `initDialog`.
//
// Size: as wide as the widest row (at least {@link COMMAND_HELP_MIN_WIDTH}), 432 high, both rounded up to 16 like every tiled frame and
// limited to the layout rect minus 32; centred in the layout rect (not in the root: that one includes the taskbar). A row wider than the
// window is clipped at the right edge, a row is never wrapped.

import { ListBox } from '../controls/ListBox';
import { SCROLLBAR_WIDTH } from '../controls/scrollMath';
import { type Rect, rectHeight, rectWidth } from '../layout/geometry';
import { WBS_CAPTION, WBS_MOVE, WBS_VSCROLL } from '../layout/styles';
import { FONT, textWidth } from '../text/fonts';
import type { WndBase } from '../wnd/WndBase';
import { WndNeuz } from '../wnd/WndNeuz';

/** The private app id of the window (resdata ids end at 2031, `APP_BUFF_STRIP` is 0x6b01). */
export const APP_COMMAND_HELP = 0x6b20;
/** The control id of its list (the e2e finds it with the `controlPoint` probe). */
export const COMMAND_HELP_LIST_ID = 1;
/** The least width of the window. */
export const COMMAND_HELP_MIN_WIDTH = 368;
/** The height of the window. */
export const COMMAND_HELP_HEIGHT = 432;
/** The space around the text of a row: frame (4 + 8), list border (4 + 4), row margin (3), scroll bar and a few pixels of air. */
const ROW_EXTRA = 12 + 8 + 3 + SCROLLBAR_WIDTH + 10;
/** The window is at most this much smaller than the layout rect (16 px of air on each side). */
const EDGE = 32;

/** What `ChatService.openCommandHelp` shows. */
export interface CommandHelpView {
  readonly title: string;
  readonly header: string;
  readonly lines: readonly string[];
}

const floor16 = (n: number): number => Math.floor(n / 16) * 16;
const ceil16 = (n: number): number => Math.ceil(n / 16) * 16;

export class CommandHelpWnd extends WndNeuz {
  readonly appId = APP_COMMAND_HELP;
  #view: CommandHelpView;
  list!: ListBox;

  constructor(view: CommandHelpView) {
    super();
    this.#view = view;
  }

  /** The window size for a set of rows in a layout rect: the widest row, the minimum width, the limit of the layout. */
  static sizeFor(rows: readonly string[], area: Rect): { w: number; h: number } {
    const widest = rows.reduce((m, r) => Math.max(m, textWidth(FONT.text, r)), 0);
    const maxW = Math.max(64, floor16(rectWidth(area) - EDGE));
    const maxH = Math.max(64, floor16(rectHeight(area) - EDGE));
    return {
      w: Math.min(maxW, Math.max(COMMAND_HELP_MIN_WIDTH, ceil16(widest + ROW_EXTRA))),
      h: Math.min(maxH, COMMAND_HELP_HEIGHT),
    };
  }

  /** A captioned, movable frame centred in the layout rect (no `initDialog`: there is no layout row). */
  override initialize(parent: WndBase): boolean {
    const area = parent.env.wm.layoutRect;
    const { w, h } = CommandHelpWnd.sizeFor(this.#rows(), area);
    const ok = this.create(
      WBS_CAPTION | WBS_MOVE,
      { l: 0, t: 0, r: w, b: h },
      parent,
      APP_COMMAND_HELP,
    );
    this.moveRectCenter(area);
    return ok;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.setTitle(this.#view.title);
    this.tile = true;
    this.texture = 'WndTile00.tga';
    const list = new ListBox();
    this.list = this.addControl(list, COMMAND_HELP_LIST_ID, this.#listRect(), WBS_VSCROLL);
    list.setSkin('WndEditTile00.tga', true);
    this.#fill();
  }

  /** The list fills the client rect (`CommandWnd.onSize`). */
  override onSize(type: number, cx: number, cy: number): void {
    this.list.setWndRect(this.#listRect(), true);
    super.onSize(type, cx, cy);
  }

  /** A second `/help`: the same window with the rows of the new answer (the scroll position goes back to the top). */
  setRows(view: CommandHelpView): void {
    this.#view = view;
    this.setTitle(view.title);
    this.list.resetContent();
    this.#fill();
  }

  /** The header and the rows in the given order (the list is NOT sorted). */
  rows(): string[] {
    return this.#rows();
  }

  #rows(): string[] {
    return [this.#view.header, ...this.#view.lines];
  }

  #fill(): void {
    for (const row of this.#rows()) this.list.addString(row);
  }

  #listRect(): Rect {
    return {
      l: 0,
      t: 0,
      r: rectWidth(this.rectClientValue),
      b: rectHeight(this.rectClientValue),
    };
  }
}
