// `CharInfoPage` (`CWndCharInfo`, `src:_Interface/WndField.cpp:3123-4600`, Plan/42 §19.4 D, 45 §20; WP-UI-21): the first tab of
// `APP_CHARACTER3`, a page that is built by code on one of two pictures (`WndNewCharacter01.tga`, or `02` with the eight
// statistics lines). It prints the character data and the derived values, lets the player distribute the free stat points
// (an edit and a `+` / `-` pair per stat, `Apply` opens the confirm window, `Reset` clears) and shows the tooltips of the
// stat rows. The numbers come from the view (45 computes them with `rules.derivedStats` / `rules.statPreview`); the pending
// allocation is the page's own state, exactly like `m_nStrCount` ... of the original.

import {
  TID_DIAG_0082,
  TID_GAME_CHARSTATUS_APPLY_Q,
  TID_TOOLTIP_CHARSTATUS_APPLY,
  TID_TOOLTIP_CHARSTATUS_RESET,
} from '../../../gen/defs/text';
import { type Button } from '../../controls/Button';
import { type EditCtrl } from '../../controls/EditCtrl';
import { TextLayer } from '../../controls/TextLayer';
import { WTYPE_BUTTON, WTYPE_EDITCTRL, EBS_NUMBER, Wnm } from '../../layout/styles';
import { type Point, rectOffset } from '../../layout/geometry';
import type { BakeSpec } from '../../skin/baker';
import type { Shortcut } from '../../dnd/shortcut';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { CodePage } from './CodePage';
import {
  CHAR_INFO_TEXTURE,
  CHAR_INFO_TEXTURE_STATISTICS,
  type CharInfoGeometry,
  blinkOn,
  charInfoGeometry,
  charInfoTexts,
  charInfoTip,
  legendMark,
  NO_PENDING,
  pendingSum,
} from './charInfoLayout';
import { PaperDoll } from './PaperDoll';
import type {
  CharacterActions,
  CharacterView,
  StateConfirmRequest,
  StatDelta,
  ViewSource,
} from './ports';

/** The window ids of the controls (`100 + 3 * i` = the edit of stat `i`, `+1` its plus, `+2` its minus; 112 Apply, 113 Reset). */
export const ID_EDIT = [100, 103, 106, 109] as const;
export const ID_PLUS = [101, 104, 107, 110] as const;
export const ID_MINUS = [102, 105, 108, 111] as const;
export const ID_APPLY = 112;
export const ID_RESET = 113;
const ID_TEXTS = 0x7b01;
const ID_MARK = 0x7b02;

const STAT_KEYS = ['str', 'sta', 'dex', 'int'] as const;
type StatKey = (typeof STAT_KEYS)[number];
const statKey = (i: number): StatKey => STAT_KEYS[i] ?? 'str';

export interface CharInfoDeps {
  readonly view: ViewSource<CharacterView>;
  readonly actions: CharacterActions;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
  /** Opens `APP_STATE_CONFIRM` (Apply). */
  readonly openStateConfirm: (request: StateConfirmRequest) => void;
}

export class CharInfoPage extends CodePage {
  readonly #deps: CharInfoDeps;
  #pending: StatDelta = NO_PENDING;
  #geometry: CharInfoGeometry | null = null;
  #texts: TextLayer | null = null;
  #marks: PaperDoll | null = null;
  readonly #edits: EditCtrl[] = [];
  readonly #plus: Button[] = [];
  readonly #minus: Button[] = [];
  #apply: Button | null = null;
  #reset: Button | null = null;
  #lastTip = '';
  #view: CharacterView | null = null;

  constructor(deps: CharInfoDeps) {
    super();
    this.#deps = deps;
  }

  // ---- accessors (tests, the feature layer) -----------------------------------------------------------------------------

  /** `m_nStrCount` ...: the allocation that is not applied yet. */
  get pending(): StatDelta {
    return this.#pending;
  }
  get edits(): readonly EditCtrl[] {
    return this.#edits;
  }
  get plusButtons(): readonly Button[] {
    return this.#plus;
  }
  get minusButtons(): readonly Button[] {
    return this.#minus;
  }
  get applyButton(): Button | null {
    return this.#apply;
  }
  get resetButton(): Button | null {
    return this.#reset;
  }
  get textLayer(): TextLayer | null {
    return this.#texts;
  }
  get markLayer(): PaperDoll | null {
    return this.#marks;
  }

  // ---- OnInitialUpdate ------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const view = this.#deps.view.get();
    this.#view = view;
    const g = charInfoGeometry(view.statistics);
    this.#geometry = g;
    this.texture = view.statistics ? CHAR_INFO_TEXTURE_STATISTICS : CHAR_INFO_TEXTURE;
    this.tile = false;

    const client = this.getClientRect(false);
    this.#texts = this.addControl(new TextLayer(), ID_TEXTS, client);
    this.#marks = this.addControl(new PaperDoll(), ID_MARK, client);

    for (let i = 0; i < 4; i += 1) {
      const edit = this.createControl<EditCtrl>(
        WTYPE_EDITCTRL,
        ID_EDIT[i] ?? 0,
        g.edit(i),
        EBS_NUMBER,
      );
      edit.setString('0');
      this.#edits.push(edit);
      const plus = this.createControl<Button>(WTYPE_BUTTON, ID_PLUS[i] ?? 0, g.plus(i), 0);
      plus.setTexture('ButtVScrUp.tga');
      this.#plus.push(plus);
      const minus = this.createControl<Button>(WTYPE_BUTTON, ID_MINUS[i] ?? 0, g.minus(i), 0);
      minus.setTexture('ButtVScrDown.tga');
      this.#minus.push(minus);
    }
    this.#apply = this.createControl<Button>(WTYPE_BUTTON, ID_APPLY, g.apply, 0);
    this.#apply.setTexture('ButtCharApply.tga');
    this.#reset = this.createControl<Button>(WTYPE_BUTTON, ID_RESET, g.reset, 0);
    this.#reset.setTexture('ButtCharReset.tga');
    if (!view.statistics) {
      this.#apply.setToolTip(this.#deps.getText(TID_TOOLTIP_CHARSTATUS_APPLY), 3);
      this.#reset.setToolTip(this.#deps.getText(TID_TOOLTIP_CHARSTATUS_RESET), 3);
    }
    this.#refresh(0);
    this.requestBake();
  }

  /** The picture plus the skins of the controls (the edit fields are not part of the picture). */
  override buildBakeSpec(): BakeSpec | null {
    const base = super.buildBakeSpec();
    if (!base) return null;
    const ops = [...base.ops];
    for (const c of this.controls) {
      if (!c.destroyed && c.isVisible()) ops.push(...c.skinOps());
    }
    return { ...base, ops };
  }

  // ---- the pending allocation -----------------------------------------------------------------------------------------------

  /** `m_nGpPoint`: the free points not yet distributed. */
  get freePoints(): number {
    return (this.#view?.remainGp ?? 0) - pendingSum(this.#pending);
  }

  #setPending(p: StatDelta): void {
    this.#pending = p;
  }

  /** Clears the allocation (`Reset`, and after the confirm). */
  clearPending(): void {
    this.#setPending(NO_PENDING);
    this.#refresh(0);
  }

  /** `Process`: the enable states and the edit strings follow the counts; the texts follow the view and the blink phase. */
  #refresh(now: number): void {
    const view = this.#deps.view.get();
    this.#view = view;
    const p = this.#pending;
    const gp = view.remainGp - pendingSum(p);
    STAT_KEYS.forEach((key, i) => {
      this.#minus[i]?.enableWindow(p[key] > 0);
      this.#plus[i]?.enableWindow(gp > 0);
      const edit = this.#edits[i];
      if (edit) {
        edit.enableWindow(gp > 0);
        const s = String(p[key]);
        if (edit.getString() !== s) edit.setString(s);
      }
    });
    const any = pendingSum(p) > 0;
    this.#apply?.enableWindow(any);
    this.#reset?.enableWindow(any);
    this.#texts?.setItems(
      charInfoTexts({ view, pending: p, blink: blinkOn(now), getText: this.#deps.getText }),
    );
    const mark = legendMark(view.legend, view.level);
    this.#marks?.setLayers(
      mark
        ? [
            {
              key: 'mark',
              x: mark.x,
              y: mark.y,
              size: mark.w,
              height: mark.h,
              icon: mark.icon,
              alpha: 1,
              tint: false,
            },
          ]
        : [],
      true,
    );
  }

  override process(now: number): void {
    this.#refresh(now);
    super.process(now);
  }

  // ---- notifications (OnChildNotify) ----------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const editIdx = ID_EDIT.findIndex((e) => e === id);
    if (editIdx >= 0 && msg === Wnm.EN_CHANGE) {
      // `atoi`, clamped to what the other three leave of the free points
      const key = statKey(editIdx);
      const edit = this.#edits[editIdx];
      const typed = Number.parseInt(edit?.getString() ?? '', 10);
      const others = pendingSum(this.#pending) - this.#pending[key];
      const room = Math.max(0, (this.#view?.remainGp ?? 0) - others);
      const n = Math.min(Number.isFinite(typed) ? typed : 0, room);
      this.#setPending({ ...this.#pending, [key]: n });
      this.#refresh(0);
      return true;
    }
    if (msg === Wnm.CLICKED) {
      const plusIdx = ID_PLUS.findIndex((e) => e === id);
      const minusIdx = ID_MINUS.findIndex((e) => e === id);
      if (plusIdx >= 0) {
        const key = statKey(plusIdx);
        this.#setPending({ ...this.#pending, [key]: this.#pending[key] + 1 });
        this.#refresh(0);
        return true;
      }
      if (minusIdx >= 0) {
        const key = statKey(minusIdx);
        this.#setPending({ ...this.#pending, [key]: Math.max(0, this.#pending[key] - 1) });
        this.#refresh(0);
        return true;
      }
      if (id === ID_APPLY) {
        if (pendingSum(this.#pending) !== 0) this.#openConfirm();
        return true;
      }
      if (id === ID_RESET) {
        this.clearPending();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /** Apply: `APP_STATE_CONFIRM` with the question (plus the vagrant warning); Yes sends the allocation and clears it. */
  #openConfirm(): void {
    const view = this.#deps.view.get();
    const text = [
      this.#deps.getText(TID_GAME_CHARSTATUS_APPLY_Q),
      ...(view.baseJob ? [this.#deps.getText(TID_DIAG_0082)] : []),
    ].join('\n');
    const delta = this.#pending;
    this.#deps.openStateConfirm({
      text,
      yes: () => {
        this.#deps.actions.modifyStatus(delta);
        this.clearPending();
      },
      no: () => undefined,
    });
  }

  // ---- tooltips ---------------------------------------------------------------------------------------------------------------

  override onMouseMove(pt: Point): void {
    const view = this.#view;
    if (!view || this.env.wm.isDraggingShortcut()) return;
    const tip = charInfoTip(pt, view, this.#pending, this.#deps.getText);
    if (!tip) {
      this.#lastTip = '';
      return;
    }
    const key = `${String(tip.rect.l)},${String(tip.rect.t)},${String(tip.rect.r)},${String(tip.rect.b)}`;
    if (this.#lastTip === key) return;
    this.#lastTip = key;
    const s = this.getScreenClientRect();
    this.#deps.tooltip.put(this, tip.runs, rectOffset(tip.rect, s.l, s.t), 3);
  }

  override onMouseLeave(): void {
    this.#lastTip = '';
  }

  /** The geometry in use (tests). */
  get geometry(): CharInfoGeometry | null {
    return this.#geometry;
  }
}
