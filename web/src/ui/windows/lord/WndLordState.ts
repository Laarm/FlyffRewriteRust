// `WndLordState` (`CWndLordState`, `src:_Interface/WndLord.cpp:28-178`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-54): `APP_LORD_STATE` (722, 432 x 320, "Candidate registration Status"), the list of the
// ten candidates. `OnDraw` writes the rows by hand: the rank with the name at the list rectangle + 15, the job at + 220 and the word "open" at + 313, each at y = top + 8 + (i - 1) * 16, black; the
// `LISTBOX1` only holds blank rows so that a click can pick a row. The clicked row gets a translucent red band (ARGB 60, 240, 0, 0; right edge - 4) and opens the pledge window of that rank (a new
// click replaces it). `WIDC_LIVE` tells whether the votes reach the requirement ("Satisfied" / "Not satisfied"); `BUTTON1` closes (and takes the pledge window with it: the original deleted it in
// its destructor). The window follows the view: names that arrive later are drawn, and the rows are rebuilt when the number of candidates changes.

import { APP_LORD_STATE, WIDC_BUTTON1, WIDC_LISTBOX1, WIDC_LIVE } from '../../../gen/defs/resdata';
import { TID_GAME_LORD_MINREQ_NO, TID_GAME_LORD_MINREQ_OK } from '../../../gen/defs/text';
import type { ListBox } from '../../controls/ListBox';
import { RectLayer } from '../../controls/RectLayer';
import type { Static } from '../../controls/Static';
import { type TextItem, TextLayer } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordWindowDeps, LordView, LordWindowOps } from './ports';
import {
  bandY,
  LIST_ROWS,
  ROW_COLOR,
  ROW_PITCH,
  SELECT_BAND_FILL,
  STATE_COLS,
  stateRowText,
  textY,
} from './rows';

const NO_RECT: Rect = { l: 0, t: 0, r: 0, b: 0 };
const ID_TEXTS = 0x7c10;
const ID_BANDS = 0x7c11;

export class WndLordState extends WndNeuz {
  readonly appId = APP_LORD_STATE;
  readonly #deps: LordWindowDeps;
  readonly #ops: LordWindowOps;
  #list: ListBox | null = null;
  #live: Static | null = null;
  #texts: TextLayer | null = null;
  #bands: RectLayer | null = null;
  #rect: Rect = NO_RECT;
  #view: LordView | null = null;
  /** `m_nSelect`: the rank of the clicked row (0 = none). */
  #select = 0;

  constructor(deps: LordWindowDeps, ops: LordWindowOps) {
    super();
    this.#deps = deps;
    this.#ops = ops;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.#list;
  }
  get liveStatic(): Static | null {
    return this.#live;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  get bands(): RectLayer | null {
    return this.#bands;
  }
  /** `m_nSelect`. */
  get selectedRank(): number {
    return this.#select;
  }
  /** The rectangle of the list in client coordinates (the geometry of the text rows). */
  get listRect(): Rect {
    return this.#rect;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#list = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#live = this.getDlgItem<Static>(WIDC_LIVE);
    this.#rect = this.getWndCtrl(WIDC_LISTBOX1)?.rect ?? NO_RECT;
    const client = this.getClientRect(false);
    this.#bands = this.addControl(new RectLayer(), ID_BANDS, client);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.moveParentCenter();
    this.#apply(this.#deps.ports.view.get());
  }

  /** The candidates the rows show: `GetRanker( i )` for i = 1..10 with a player id. */
  #rows(view: LordView): LordView['candidates'] {
    return view.candidates.slice(0, LIST_ROWS).filter((c) => c.id > 0);
  }

  #apply(view: LordView): void {
    this.#view = view;
    const rows = this.#rows(view);
    // the blank rows of the list box (hit test only); the selection is kept while the row count is
    const list = this.#list;
    if (list && list.getCount() !== rows.length) {
      list.resetContent();
      rows.forEach(() => {
        list.addString(' '.repeat(77));
      });
    }
    const host = this.#deps.host;
    this.#live?.setTitle(
      host.text(view.requirementOk ? TID_GAME_LORD_MINREQ_OK : TID_GAME_LORD_MINREQ_NO),
    );
    this.#paint(rows);
  }

  #paint(rows: LordView['candidates']): void {
    const r = this.#rect;
    const items: TextItem[] = [];
    rows.forEach((c, index) => {
      const rank = index + 1;
      const y = textY(r.t, rank);
      // the rank text is never empty (`strName` holds the rank too); an unknown job name is not drawn (`strClass.IsEmpty()`)
      items.push({
        x: r.l + STATE_COLS.name,
        y,
        text: stateRowText(rank, c.name),
        color: ROW_COLOR,
      });
      if (c.job !== '') items.push({ x: r.l + STATE_COLS.job, y, text: c.job, color: ROW_COLOR });
      items.push({ x: r.l + STATE_COLS.open, y, text: 'open', color: ROW_COLOR });
    });
    this.#texts?.setItems(items);
    const sel = this.#select;
    this.#bands?.setEntries(
      sel > 0 && sel <= rows.length
        ? [
            {
              key: 'select',
              x: r.l,
              y: bandY(r.t, sel),
              w: r.r - 4 - r.l,
              h: ROW_PITCH,
              fill: SELECT_BAND_FILL,
            },
          ]
        : [],
    );
  }

  override process(now: number): void {
    const view = this.#deps.ports.view.get();
    if (view !== this.#view) this.#apply(view);
    super.process(now);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (id === WIDC_LISTBOX1 && (msg === Wnm.SELCHANGE || msg === Wnm.DBLCLK)) {
      const rank = (this.#list?.getCurSel() ?? -1) + 1;
      const view = this.#view ?? this.#deps.ports.view.get();
      // a click below the last row picks nothing
      if (rank > 0 && rank <= this.#rows(view).length) {
        this.#ops.openPledge(rank);
        this.#select = rank;
        this.#paint(this.#rows(view));
      }
      return true;
    }
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    // `~CWndLordState`: `SAFE_DELETE( m_pWndPledge )`
    this.#ops.closePledge();
    super.onDestroy();
  }
}
