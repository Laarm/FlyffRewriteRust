// `WndSelectChar` (`CWndSelectChar`, `src:_Interface/WndTitle.cpp:1430-2395`, ref/22 §17 B, Plan/42 §4.5, §8.12, 45 §4):
// `APP_SELECT_CHAR` (544 x 368, `WndSelectChar.tga`). Three slots of 158 x 238 px at 170 px pitch (`m_aRect`: client
// coordinates `(16, 16)`, `(186, 16)`, `(356, 16)`), each with a 3-D preview canvas (41's `charSelect` preset), the name
// (bold `0xff6060ff` for the selected slot, `0xff505050` for the others) at `rect.bottom + 10`, the selection highlight
// `ARGB(20, 80, 250, 80)` and a hover tooltip with the character's data (legacy `PutToolTip_Character`).
//
// Buttons: Back, Create (enabled on an empty selected slot), Delete and Accept (enabled on an occupied one); everything is
// disabled while a lobby request is in flight (45 §4.2). The left / right arrow KEYS (key-up) cycle the selection with
// wrap-around, a click selects, a double click or Enter starts. The controller (45 WP-CFEAT-03) decides about blocked slots
// (`TID_DIAG_0073`), the request gate and the network.

import {
  APP_SELECT_CHAR,
  WIDC_ACCEPT,
  WIDC_BACK,
  WIDC_CREATE,
  WIDC_DELETE,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { Static } from '../../controls/Static';
import type { Point, Rect } from '../../layout/geometry';
import { WBS_KEY, WM_LBUTTONDBLCLK, WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { FillBox } from './FillBox';
import type {
  CharSelectActions,
  CharSelectView,
  CharSlotView,
  PreviewHandleLike,
  PreviewPort,
  ViewSource,
} from './ports';

/** `MAX_CHARACTER_LIST`. */
export const CHAR_SLOTS = 3;

/** `m_aRect[i]`: `CRect(16, 16, 174, 254)` offset by 170 px per slot (client coordinates). */
export function slotRect(i: number): Rect {
  const l = 16 + 170 * i;
  return { l, t: 16, r: l + 158, b: 254 };
}

/** The name colours and the highlight of `OnDraw`. */
export const NAME_COLOR_SELECTED = 0xff6060ff;
export const NAME_COLOR_OTHER = 0xff505050;
/** The marker of a blocked character (a dark red name; the slot cannot be started, 45 §4.1). */
export const NAME_COLOR_BLOCKED = 0xff993030;
/** `D3DCOLOR_ARGB(20, 80, 250, 80)`. */
export const HIGHLIGHT_COLOR = 0x1450fa50;

/** The ids of the code-created controls (above every layout id of the window). */
const ID_NAME = 9100;
const ID_FILL = 9110;
const ID_HIT = 9120;

export interface SelectButtons {
  back: Button | null;
  create: Button | null;
  remove: Button | null;
  accept: Button | null;
}

interface PreviewEntry {
  readonly idPlayer: number;
  readonly handle: PreviewHandleLike;
  subject: unknown;
}

export class WndSelectChar extends WndNeuz {
  readonly appId = APP_SELECT_CHAR;
  readonly #model: ViewSource<CharSelectView>;
  readonly #actions: CharSelectActions;
  readonly #previews: PreviewPort | null;
  readonly #entries: (PreviewEntry | null)[] = [null, null, null];
  readonly #names: Static[] = [];
  readonly #fills: FillBox[] = [];
  readonly #hits: Static[] = [];
  #buttons: SelectButtons = { back: null, create: null, remove: null, accept: null };
  #off: (() => void) | null = null;

  constructor(
    model: ViewSource<CharSelectView>,
    actions: CharSelectActions,
    previews: PreviewPort | null = null,
  ) {
    super();
    this.#model = model;
    this.#actions = actions;
    this.#previews = previews;
    this.putRegInfo = false; // `SetPutRegInfo(FALSE)`
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_KEY | extraStyle);
  }

  /** The preview handle of a slot (tests, the probe). */
  previewHandle(slot: number): PreviewHandleLike | null {
    return this.#entries[slot]?.handle ?? null;
  }
  /** The four buttons (tests). */
  get buttons(): Readonly<SelectButtons> {
    return this.#buttons;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const back = this.getDlgItem<Button>(WIDC_BACK);
    const create = this.getDlgItem<Button>(WIDC_CREATE);
    const remove = this.getDlgItem<Button>(WIDC_DELETE);
    const accept = this.getDlgItem<Button>(WIDC_ACCEPT);
    this.#buttons = { back, create, remove, accept };
    accept?.setDefault(true);
    for (let i = 0; i < CHAR_SLOTS; i += 1) {
      const r = slotRect(i);
      // the selection highlight: rect shrunk by 5 px top and bottom, shifted by `i` px
      const fill = new FillBox();
      this.addControl(fill, ID_FILL + i, { l: r.l + i, t: r.t + 5, r: r.r + i, b: r.b - 5 });
      this.#fills.push(fill);
      // the name: `TextOut(rect.left, rect.bottom + 10)`; a static draws its caption at (2, 2) in its rect
      const name = new Static();
      this.addControl(name, ID_NAME + i, { l: r.l - 2, t: r.b + 8, r: r.r + 2, b: r.b + 30 }, 0);
      this.#names.push(name);
      // the slot hit area: clicks and the hover tooltip
      const hit = new Static();
      this.addControl(hit, ID_HIT + i, r, 0);
      this.#hits.push(hit);
    }
    this.moveParentCenter();
    this.#apply(this.#model.get());
    this.#off = this.#model.subscribe(() => {
      this.#apply(this.#model.get());
    });
    // the last control created holds the focus (KEEP): `pWndBack->SetFocus()` comes after every control exists
    back?.setFocus();
  }

  /** Applies the view with diffing setters. */
  #apply(m: CharSelectView): void {
    const enabled = !m.busy;
    const sel = m.slots[m.selected] ?? null;
    this.#buttons.back?.enableWindow(enabled);
    this.#buttons.create?.enableWindow(enabled && sel === null);
    this.#buttons.remove?.enableWindow(enabled && sel !== null && !sel.blocked);
    this.#buttons.accept?.enableWindow(enabled && sel !== null && !sel.blocked);
    for (let i = 0; i < CHAR_SLOTS; i += 1) {
      const slot = m.slots[i] ?? null;
      const selected = i === m.selected;
      const name = this.#names[i];
      name?.setTitle(slot ? slot.name : m.emptyLabel);
      name?.setTextColor(
        slot?.blocked === true
          ? NAME_COLOR_BLOCKED
          : selected
            ? NAME_COLOR_SELECTED
            : NAME_COLOR_OTHER,
      );
      this.#fills[i]?.setColor(selected ? HIGHLIGHT_COLOR : 0);
      this.#hits[i]?.setToolTip(slot ? m.infoText(slot) : '');
      this.#syncPreview(i, slot, selected);
    }
  }

  #syncPreview(i: number, slot: CharSlotView | null, selected: boolean): void {
    const entry = this.#entries[i] ?? null;
    if (slot === null || this.#previews === null) {
      if (entry) this.#closePreview(i);
      return;
    }
    if (entry && entry.idPlayer !== slot.idPlayer) this.#closePreview(i);
    const current = this.#entries[i] ?? null;
    if (current) {
      current.handle.setSelected(selected);
      if (current.subject !== slot.subject) {
        current.subject = slot.subject;
        current.handle.setSubject(slot.subject);
      }
      return;
    }
    const canvas = this.addCustomCanvas(`PREVIEW${String(i)}`, slotRect(i));
    if (!canvas) return;
    const handle = this.#previews.open({
      canvas,
      preset: 'charSelect',
      subject: slot.subject,
      selected,
    });
    if (handle) this.#entries[i] = { idPlayer: slot.idPlayer, handle, subject: slot.subject };
  }

  #closePreview(i: number): void {
    const entry = this.#entries[i];
    if (!entry) return;
    this.#entries[i] = null;
    try {
      entry.handle.close();
    } catch (e) {
      this.env.log.warn('ui', `closing the preview of slot ${String(i)}: ${String(e)}`);
    }
    this.removeCustomCanvas(`PREVIEW${String(i)}`);
  }

  /** The left / right arrow KEYS (key-up) cycle the selection with wrap-around (`OnKeyUp`). */
  override onKeyUp(code: string): void {
    const m = this.#model.get();
    if (m.busy) return;
    if (code === 'ArrowLeft') this.#actions.select((m.selected + CHAR_SLOTS - 1) % CHAR_SLOTS);
    else if (code === 'ArrowRight') this.#actions.select((m.selected + 1) % CHAR_SLOTS);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const m = this.#model.get();
    if (msg === Wnm.CLICKED) {
      // the title bar's close button is "Back" (`case 10000: // close msg`)
      if (id === WTBID_CLOSE || id === WIDC_BACK) {
        if (!m.busy) this.#actions.back();
        return true;
      }
      if (m.busy) return true;
      if (id === WIDC_CREATE) {
        if (m.slots[m.selected] === null || m.slots[m.selected] === undefined) {
          this.#actions.create(m.selected);
        }
        return true;
      }
      if (id === WIDC_DELETE) {
        const s = m.slots[m.selected];
        if (s && !s.blocked) this.#actions.remove(m.selected);
        return true;
      }
      if (id === WIDC_ACCEPT) {
        const s = m.slots[m.selected];
        if (s && !s.blocked) this.#actions.start(m.selected);
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  /**
   * `OnLButtonUp(point)`: a click inside the rect of a slot selects it (the press on the slot's hit area moved the
   * focus to the frame, so the frame receives the release with client coordinates).
   */
  override onLButtonUp(pt: Point): void {
    const m = this.#model.get();
    if (m.busy) return;
    for (let i = 0; i < CHAR_SLOTS; i += 1) {
      const r = slotRect(i);
      if (pt.x >= r.l && pt.x < r.r && pt.y >= r.t && pt.y < r.b) {
        this.#actions.select(i);
        return;
      }
    }
  }

  /** A double click on a slot area (`WM_LBUTTONDBLCLK` of its hit control): select and start an occupied one. */
  override onCommand(id: number, msg: number): boolean {
    const i = id - ID_HIT;
    if (i >= 0 && i < CHAR_SLOTS && msg === WM_LBUTTONDBLCLK) {
      const m = this.#model.get();
      if (m.busy) return true;
      const s = m.slots[i];
      if (s && !s.blocked) {
        if (m.selected !== i) this.#actions.select(i);
        this.#actions.start(i);
      } else {
        this.#actions.select(i);
      }
      return true;
    }
    return super.onCommand(id, msg);
  }

  override onDestroy(): void {
    this.#off?.();
    this.#off = null;
    for (let i = 0; i < CHAR_SLOTS; i += 1) this.#closePreview(i);
    super.onDestroy();
  }
}
