// `WndDialogEx` (`CWndDialog`, `src:_Interface/WndDialog.cpp`, ref/22 §17 E, Plan/42 §19.4 E, Plan/45 §11.3; WP-UI-24):
// `APP_DIALOG_EX` (512 x 320, the picture `WndDialogEx.tga`, MODAL). Four areas of the layout: the portrait (`WIDC_CUSTOM3`),
// the page text (`WIDC_CUSTOM1`), the key buttons (`WIDC_CUSTOM2`), the Next button (`WIDC_BUTTON1`) and the group box with
// the clicked key word (`WIDC_GROUP_BOX_TITLE`). Everything but the Next button and the four image answers is painted by the
// original's `OnDraw` and hit-tested by the window itself, so the shell does the same: it lays the words out
// (`dialogLayout.ts`), pushes them to a `DialogSurface` and answers the pointer events of the frame.
//
// What the dialog MEANS (the operations of `DialogOps`, the one-second click throttle, the sends) is behind the ports
// (`features/npc/dialog`). A click is `actions.click(..)`; the view changes in answer and the window re-applies it at once.

import {
  APP_DIALOG_EX,
  APP_INVENTORY,
  WIDC_BUTTON1,
  WIDC_GROUP_BOX_TITLE,
} from '../../../gen/defs/resdata';
import {
  TID_GAME_ABOUTQUESTION,
  TID_GAME_CURRENT_QUEST,
  TID_GAME_DIALOG,
  TID_GAME_EMPTY_CURRENT_QUEST,
  TID_GAME_EMPTY_NEW_QUEST,
  TID_GAME_NEW_QUEST,
} from '../../../gen/defs/text';
import { Button } from '../../controls/Button';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import { type Point, type Rect, rectContains, rectOffset } from '../../layout/geometry';
import { WBS_CHILD, WBS_MODAL, Wnm } from '../../layout/styles';
import { charWidth, FONT, lineHeight, textWidth } from '../../text/fonts';
import { type RichLine, layoutRich } from '../../text/layout';
import { parseMarkup } from '../../text/markup';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import {
  type ContextLink,
  isImageAnswer,
  layoutAnswers,
  layoutContextLinks,
  layoutKeyButtons,
  type Measure,
} from './dialogLayout';
import { DialogSurface, type SurfaceQuestList, type SurfaceWord } from './DialogSurface';
import type { DialogActions, DialogClick, DialogQuestEntry, DialogView, ViewSource } from './ports';

/** The window id of the surface and the first id of the answer image buttons (above the ids of the layout). */
const SURFACE_ID = 0x7b01;

/** The quest-list icons (`QuestUiPaper*.tga`, `RenderNewQuestListIcon` / `RenderCurrentQuestListIcon`). */
export const QUEST_ICONS = {
  new: 'QuestUiPaperGreen.tga',
  expected: 'QuestUiPaperRed.tga',
  current: 'QuestUiPaperGray.tga',
  complete: 'QuestUiPaperYellow.tga',
} as const;

/** `m_nSelectColor` of the quest lists and the line space of their rows (`QUEST_LIST_LINE_SPACE`). */
const QUEST_LINE_SPACE = 3;

export interface DialogDeps {
  readonly view: ViewSource<DialogView>;
  readonly actions: DialogActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

const MEASURE: Measure = {
  width: (text) => textWidth(FONT.text, text),
  cell: lineHeight(FONT.text),
};

export class WndDialogEx extends WndNeuz {
  readonly appId = APP_DIALOG_EX;
  readonly #deps: DialogDeps;
  #surface: DialogSurface | null = null;
  #next: Button | null = null;
  #title: Static | null = null;
  #text: Rect = { l: 0, t: 0, r: 0, b: 0 };
  #keysArea: Rect = { l: 0, t: 0, r: 0, b: 0 };
  #portraitAt: Point = { x: 0, y: 0 };
  #view: DialogView | null = null;
  #off: (() => void) | null = null;
  /** The pointer, in client coordinates (`null` outside). */
  #pointer: Point | null = null;

  // the layout of the applied view
  #lines: readonly RichLine[] = [];
  #keyRects: readonly { word: string; rect: Rect }[] = [];
  #answerLinks: readonly { word: string; rect: Rect; index: number }[] = [];
  #links: readonly ContextLink[] = [];
  #questRows: readonly { rect: Rect; list: 'new' | 'current'; index: number }[] = [];
  readonly #imageButtons = new Map<Button, number>();
  #imageKey = '';
  #destroyedNotified = false;

  constructor(deps: DialogDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  /** `InitDialog(APP_DIALOG_EX, WBS_MODAL)`. */
  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  // ---- accessors for tests and the feature layer ----------------------------------------------------------------------

  get surface(): DialogSurface | null {
    return this.#surface;
  }
  get nextButton(): Button | null {
    return this.#next;
  }
  get groupTitle(): Static | null {
    return this.#title;
  }
  get textArea(): Rect {
    return this.#text;
  }
  get keyArea(): Rect {
    return this.#keysArea;
  }
  /** The hit rects of the key buttons (client coordinates), in the order of the view. */
  get keyRects(): readonly { word: string; rect: Rect }[] {
    return this.#keyRects;
  }
  get answerLinkRects(): readonly { word: string; rect: Rect; index: number }[] {
    return this.#answerLinks;
  }
  get contextLinks(): readonly ContextLink[] {
    return this.#links;
  }
  /** The image answer buttons currently on the window. */
  get imageButtons(): readonly Button[] {
    return [...this.#imageButtons.keys()];
  }
  get lines(): readonly RichLine[] {
    return this.#lines;
  }

  // ---- OnInitialUpdate ------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const t = this.#deps.getText;
    this.#title = this.getDlgItem<Static>(WIDC_GROUP_BOX_TITLE);
    // `m_dwColor = D3DCOLOR_ARGB(255, 128, 0, 64)`, `SetTitle(TID_GAME_DIALOG)`
    this.#title?.setTextColor(0xff800040);
    this.#title?.setTitle(t(TID_GAME_DIALOG));
    this.#text = this.getWndCtrl('WIDC_CUSTOM1')?.rect ?? this.#text;
    this.#keysArea = this.getWndCtrl('WIDC_CUSTOM2')?.rect ?? this.#keysArea;
    const portrait = this.getWndCtrl('WIDC_CUSTOM3')?.rect;
    this.#portraitAt = portrait ? { x: portrait.l, y: portrait.t } : this.#portraitAt;
    this.#next = this.getDlgItem<Button>(WIDC_BUTTON1);
    this.#next?.setVisible(false);

    // the surface paints under the Next button and the answer buttons: it is the first control of the window
    const surface = new DialogSurface();
    this.addControl(surface, SURFACE_ID, this.getClientRect(false));
    this.controlList.splice(this.controlList.indexOf(surface), 1);
    this.controlList.unshift(surface);
    this.publish();
    this.#surface = surface;

    // centred at the top of the layout rect: `Move(CPoint(x, 10))`
    const layout = this.env.wm.layoutRect;
    const w = this.rectWindow.r - this.rectWindow.l;
    this.move({ x: layout.l + Math.trunc((layout.r - layout.l - w) / 2), y: 10 });

    this.#off = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
    this.#apply(this.#deps.view.get());
  }

  // ---- the view ------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#view) this.#apply(v);
    super.process(now);
  }

  #apply(v: DialogView): void {
    const first = this.#view === null;
    this.#view = v;
    const surface = this.#surface;
    if (!surface) return;
    const t = this.#deps.getText;
    this.#title?.setTitle(v.title === '' ? t(TID_GAME_DIALOG) : v.title);
    surface.setPortrait(v.portrait === null ? null : { path: v.portrait, at: this.#portraitAt });

    // the shown page: aligned in the text area (`m_string.Init(m_pFont, &rect)` + `SetEditString`)
    const width = this.#text.r - this.#text.l;
    this.#lines = v.shown ? layoutRich(v.shown.runs, width, FONT.text, true) : [];

    const lastPage = v.pageCount > 0 && v.page === v.pageCount - 1;
    // the Next button (`UpdateButtonEnable`): visible while more pages remain
    this.#next?.setVisible(v.pageCount > 0 && !lastPage);

    // key buttons
    const isQuestWord = (w: string): boolean =>
      w === t(TID_GAME_NEW_QUEST) || w === t(TID_GAME_CURRENT_QUEST);
    this.#keyRects = layoutKeyButtons(
      v.keys.map((k) => k.word),
      this.#keysArea,
      MEASURE,
      isQuestWord,
    );

    // answers (only on the last page; the buttons and links are gone after a click: the view has no answers then)
    const answers = lastPage ? v.answers : [];
    const lay = layoutAnswers(
      answers.map((a) => a.word),
      this.#lines.length,
      this.#text,
      MEASURE,
    );
    this.#answerLinks = lay.links;
    this.#syncImageButtons(lay.images, answers.length);

    // clickable words of the text (alive while the page list is: a click clears the links)
    this.#links =
      v.shown && v.pageCount > 0
        ? layoutContextLinks(this.#lines, v.shown.marks, v.shown.links, this.#text, MEASURE, (ch) =>
            charWidth(FONT.text, ch),
          )
        : [];

    // quest lists (a list is shown while the clicked key is its pseudo key and it has entries)
    this.#questRows = [];
    const quests = this.#questList(v);
    surface.setQuests(quests);

    // text / empty message
    const selected = v.keys[v.selectedKey]?.word ?? '';
    let empty: { text: string; at: Point } | null = null;
    if (!v.say && selected === t(TID_GAME_NEW_QUEST) && v.newQuests.length === 0) {
      empty = {
        text: t(TID_GAME_EMPTY_NEW_QUEST),
        at: { x: this.#text.l + 10, y: this.#text.t + 10 },
      };
    } else if (!v.say && selected === t(TID_GAME_CURRENT_QUEST) && v.currentQuests.length === 0) {
      empty = {
        text: t(TID_GAME_EMPTY_CURRENT_QUEST),
        at: { x: this.#text.l + 10, y: this.#text.t + 10 },
      };
    }
    surface.setEmpty(empty);
    surface.setBody(
      v.say && this.#lines.length > 0
        ? { lines: this.#lines, at: { x: this.#text.l, y: this.#text.t }, clip: this.#text }
        : null,
    );
    this.#paintWords();
    if (first) this.#repaintHover();
  }

  /** The visible quest list (`m_WndNewQuestListBox` / `m_WndCurrentQuestListBox`) as rows with their icons. */
  #questList(v: DialogView): SurfaceQuestList | null {
    const t = this.#deps.getText;
    const word = v.keys[v.selectedKey]?.word ?? '';
    let list: 'new' | 'current' | null = null;
    if (!v.say) {
      if (word === t(TID_GAME_NEW_QUEST) && v.newQuests.length > 0) list = 'new';
      else if (word === t(TID_GAME_CURRENT_QUEST) && v.currentQuests.length > 0) list = 'current';
    }
    if (list === null) return null;
    const entries = list === 'new' ? v.newQuests : v.currentQuests;
    const cell = lineHeight(FONT.text);
    const pitch = cell + QUEST_LINE_SPACE;
    const rows: SurfaceQuestList['rows'][number][] = [];
    const rects: { rect: Rect; list: 'new' | 'current'; index: number }[] = [];
    entries.forEach((e: DialogQuestEntry, i) => {
      const top = this.#text.t + 3 + i * pitch;
      // `if (y >= top && y + h <= bottom)`: only the rows that fit are drawn (the list scrolls in the original)
      if (top + pitch > this.#text.b) return;
      const rect: Rect = { l: this.#text.l, t: top, r: this.#text.r, b: top + pitch };
      const icon =
        list === 'new'
          ? e.valid
            ? QUEST_ICONS.new
            : QUEST_ICONS.expected
          : v.completable.has(e.questId)
            ? QUEST_ICONS.complete
            : QUEST_ICONS.current;
      const lines = layoutRich(parseMarkup(e.text, 0xff404040), width16(rect), FONT.text, true);
      rows.push({ rect, lines, icon, hover: false });
      rects.push({ rect, list: list, index: i });
    });
    this.#questRows = rects;
    return { area: this.#text, rows };
  }

  /** Creates, moves and removes the image answer buttons (`MakeAnswerButton` creates them again for every response). */
  #syncImageButtons(
    images: readonly {
      readonly index: number;
      readonly id: string;
      readonly texture: string;
      readonly rect: Rect;
    }[],
    answerCount: number,
  ): void {
    const key = images.map((i) => `${String(i.index)}:${i.texture}:${String(i.rect.l)}`).join('|');
    const sig = `${key}#${String(answerCount)}`;
    if (sig === this.#imageKey) return;
    this.#imageKey = sig;
    for (const b of this.#imageButtons.keys()) this.removeControl(b);
    this.#imageButtons.clear();
    images.forEach((img, n) => {
      const b = this.createControl<Button>(1, 0x7b10 + n, img.rect, WBS_CHILD);
      b.setTexture(img.texture);
      void b.fitTextureSize();
      this.#imageButtons.set(b, img.index);
    });
  }

  // ---- painting the words (hover state) ------------------------------------------------------------------------------------

  #hoverKey = -1;
  #hoverAnswer = -1;
  #hoverLink = -1;
  #hoverQuest = -1;

  #paintWords(): void {
    const surface = this.#surface;
    const v = this.#view;
    if (!surface || !v) return;
    const keys: SurfaceWord[] = this.#keyRects.map((k, i) => ({
      word: k.word,
      rect: k.rect,
      hover: i === this.#hoverKey,
      selected: i === v.selectedKey,
    }));
    surface.setKeys(keys);
    surface.setAnswers(
      this.#answerLinks.map((a, i) => ({
        word: a.word,
        rect: a.rect,
        hover: i === this.#hoverAnswer,
      })),
    );
    const link = this.#links[this.#hoverLink];
    surface.setLinks(
      this.#links.map((l, i) => ({
        word: l.word,
        rect: l.rect,
        hover:
          link !== undefined &&
          (i === this.#hoverLink || i === link.link || l.link === this.#hoverLink),
      })),
    );
    const q = surface.state.get().quests;
    if (q) {
      surface.setQuests({
        area: q.area,
        rows: q.rows.map((r, i) => ({ ...r, hover: i === this.#hoverQuest })),
      });
    }
  }

  /** `OnMouseWndSurface`: the word under the pointer (answers, then keys, then the clickable words). */
  #repaintHover(): void {
    const p = this.#pointer;
    this.#hoverKey = -1;
    this.#hoverAnswer = -1;
    this.#hoverLink = -1;
    this.#hoverQuest = -1;
    if (p) {
      const answer = this.#answerLinks.findIndex((a) => rectContains(a.rect, p));
      if (answer >= 0) {
        this.#hoverAnswer = answer;
      } else {
        const key = this.#keyRects.findIndex((k) => rectContains(k.rect, p));
        if (key >= 0) this.#hoverKey = key;
        else {
          const link = this.#links.findIndex((l) => rectContains(l.rect, p));
          if (link >= 0) this.#hoverLink = link;
          else this.#hoverQuest = this.#questRows.findIndex((r) => rectContains(r.rect, p));
        }
      }
    }
    this.#paintWords();
    this.#tooltip(p);
  }

  /** The key button tooltip (`TID_GAME_ABOUTQUESTION`, not in the German and French clients). */
  #tooltip(p: Point | null): void {
    if (this.#hoverKey < 0 || !p) {
      this.#deps.tooltip.cancel();
      return;
    }
    const k = this.#keyRects[this.#hoverKey];
    if (!k) return;
    const text = this.#deps.getText(TID_GAME_ABOUTQUESTION).replace(/%s/, k.word);
    if (text === '') return;
    const sc = this.getScreenClientRect();
    const r = rectOffset(k.rect, sc.l, sc.t);
    this.#deps.tooltip.put(this, text, { l: r.l - 2, t: r.t - 2, r: r.r + 2, b: r.b + 2 }, 0);
  }

  /** The cursor over a word: the hand over keys and answers, the help cursor over the clickable words (`SetMouseCursor`). */
  uiCursor(): number | null {
    if (this.#hoverKey >= 0 || this.#hoverAnswer >= 0 || this.#hoverQuest >= 0) return 10; // CUR_SELECT
    if (this.#hoverLink >= 0) return 11; // CUR_HELP
    return null;
  }

  override onMouseMove(pt: Point): void {
    this.#pointer = pt;
    this.#repaintHover();
  }

  override onMouseLeave(): void {
    this.#pointer = null;
    this.#repaintHover();
  }

  // ---- clicks ---------------------------------------------------------------------------------------------------------------

  override onLButtonUp(pt: Point): void {
    const click = this.#clickAt(pt);
    if (click) this.#click(click);
  }

  /** The thing under a client point (answers, keys, the clickable words, the quest entries), or `null`. */
  #clickAt(p: Point): DialogClick | null {
    const answer = this.#answerLinks.find((a) => rectContains(a.rect, p));
    if (answer) return { kind: 'answer', index: answer.index };
    const key = this.#keyRects.findIndex((k) => rectContains(k.rect, p));
    if (key >= 0) return { kind: 'key', index: key };
    const link = this.#links.find((l) => rectContains(l.rect, p));
    if (link) return { kind: 'link', key: link.key };
    const row = this.#questRows.find((r) => rectContains(r.rect, p));
    if (row) return { kind: 'quest', list: row.list, index: row.index };
    return null;
  }

  #click(c: DialogClick): void {
    this.#deps.tooltip.cancel();
    if (!this.#deps.actions.click(c)) return;
    this.env.host.sound?.click();
    // the view changed: apply it now rather than at the next poll
    const v = this.#deps.view.get();
    if (v !== this.#view) this.#apply(v);
    this.#repaintHover();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && this.#next && id === this.#next.id) {
      this.#deps.actions.next();
      const v = this.#deps.view.get();
      if (v !== this.#view) this.#apply(v);
      return true;
    }
    if (msg === Wnm.CLICKED && src instanceof Button) {
      const index = this.#imageButtons.get(src);
      if (index !== undefined) {
        this.#click({ kind: 'answer', index });
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    this.#deps.tooltip.cancel();
    if (!this.#destroyedNotified) {
      this.#destroyedNotified = true;
      this.#deps.actions.closed();
    }
    super.onDestroy();
  }
}

/** The width of a quest row's text (the row minus the icon). */
function width16(rect: Rect): number {
  return Math.max(1, rect.r - rect.l - 16);
}

export { isImageAnswer, APP_INVENTORY };
