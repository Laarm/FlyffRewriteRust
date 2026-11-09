// `WndLordVote` (`CWndLordVote`, `src:_Interface/WndLord.cpp:457-615`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-54): `APP_LORD_VOTE` (724, 352 x 288, "Vote"), the ballot. `OnDraw` writes the rows by hand:
// the level at the list rectangle + 15, the name at + 60 ("%s (%d)" with the votes while `features.select_lord` is on) and the job at + 220, each at y = top + 8 + (i - 1) * 16, black. With
// `select_lord` a click on a row puts the name into `EDIT1`; without it the player types the name. `BUTTON1` votes: an empty edit does nothing, the candidate whose name equals the text
// (case-sensitive, the `CString` compare) gets `ElectionIncVoteReq`, no match does nothing (KEEP: `TID_GAME_NO_CANDIDATE` was commented out). The confirmation is the box `TID_GAME_VOTE_COMPLETE`, which
// the election answer opens while it closes this window (`features/events/lord/reducers.ts`). FIX: the list box of the original was never filled, so its click found no row (`GetCurSel() == -1`); it
// holds one blank row per candidate here, so that the click works as the plan describes it.

import { APP_LORD_VOTE, WIDC_BUTTON1, WIDC_EDIT1, WIDC_LISTBOX1 } from '../../../gen/defs/resdata';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { ListBox } from '../../controls/ListBox';
import { type TextItem, TextLayer } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordView, LordWindowDeps } from './ports';
import { LIST_ROWS, ROW_COLOR, textY, VOTE_COLS } from './rows';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_TEXTS = 0x7c20;

export class WndLordVote extends WndNeuz {
  readonly appId = APP_LORD_VOTE;
  readonly #deps: LordWindowDeps;
  #list: ListBox | null = null;
  #texts: TextLayer | null = null;
  #rect: Rect = NO_RECT;
  #view: LordView | null = null;

  constructor(deps: LordWindowDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.#list;
  }
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  get listRect(): Rect {
    return this.#rect;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#rect = this.getWndCtrl(WIDC_LISTBOX1)?.rect ?? NO_RECT;
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, this.getClientRect(false));
    this.moveParentCenter();
    this.#apply(this.#deps.ports.view.get());
  }

  #rows(view: LordView): LordView['candidates'] {
    return view.candidates.slice(0, LIST_ROWS).filter((c) => c.id > 0);
  }

  #apply(view: LordView): void {
    this.#view = view;
    const rows = this.#rows(view);
    const list = this.#list;
    if (list && list.getCount() !== rows.length) {
      list.resetContent();
      rows.forEach(() => {
        list.addString(' '.repeat(77));
      });
    }
    const r = this.#rect;
    const items: TextItem[] = [];
    rows.forEach((c, index) => {
      const y = textY(r.t, index + 1);
      const name = view.selectLord ? `${c.name} (${String(c.votes)})` : c.name;
      items.push(
        { x: r.l + VOTE_COLS.level, y, text: String(c.level), color: ROW_COLOR },
        { x: r.l + VOTE_COLS.name, y, text: name, color: ROW_COLOR },
        { x: r.l + VOTE_COLS.job, y, text: c.job, color: ROW_COLOR },
      );
    });
    this.#texts?.setItems(items);
  }

  override process(now: number): void {
    const view = this.#deps.ports.view.get();
    if (view !== this.#view) this.#apply(view);
    super.process(now);
  }

  /** `BUTTON1`: votes for the candidate named in the edit; returns the candidate id that was voted for, `null` when nothing was sent. */
  vote(): number | null {
    const typed = this.edit?.getString() ?? '';
    if (typed === '') return null;
    const view = this.#view ?? this.#deps.ports.view.get();
    let sent: number | null = null;
    for (const c of this.#rows(view)) {
      if (c.name === typed) {
        this.#deps.ports.actions.incVote(c.id);
        sent = c.id;
      }
    }
    return sent;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_LISTBOX1 && msg === Wnm.SELCHANGE) {
      const view = this.#view ?? this.#deps.ports.view.get();
      if (view.selectLord) {
        const c = this.#rows(view)[this.#list?.getCurSel() ?? -1];
        // a candidate whose name is unknown fills nothing (`GetPlayerData` answered a placeholder)
        if (c && c.name !== '') this.edit?.setString(c.name);
      }
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.vote();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
