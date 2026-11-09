// `CoupleInfoPage` (`CWndCoupleTabInfo`, `src:_Interface/WndField.cpp:23800-23965`, ref/22 §17 K, Plan/42 §19.4 K, 45 §24.2; WP-UI-52): the "Couple Information" tab of `APP_COUPLE_MAIN`
// (`APP_COUPLE_TAB_INFO`, 288 x 302). The original redraws every frame from the couple record and the player-data cache: the partner's name, job and level and the couple level in the
// colour 0xff3f3f3f, the master / hero mark of the partner beside the level and the EXP gauge in the lower group box; every text is empty and the gauge empty while the player is single (the
// window still opens). The "break up" button (`ButtBreakUp.bmp`) acts only with a partner whose name the cache already knows (FIX: the original asked about an empty name).

import {
  APP_COUPLE_TAB_INFO,
  WIDC_BUTTON1,
  WIDC_COUPLE_LEVEL,
  WIDC_JOB,
  WIDC_LEVEL,
  WIDC_NAME,
  WIDC_STATIC4,
} from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { Gauge } from '../../controls/Gauge';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Static } from '../../controls/Static';
import type { Shortcut } from '../../dnd/shortcut';
import type { Rect } from '../../layout/geometry';
import { Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { PLAYER_DATA_ATLAS } from '../messenger/icons';
import {
  type CoupleActions,
  type CoupleInfoView,
  EMPTY_COUPLE_INFO,
  type ViewSource,
} from './ports';

/** `m_dwColor` of the statics once a partner is known. */
export const INFO_TEXT_COLOR = 0xff3f3f3f;
/** `SetTexture( ... "ButtBreakUp.bmp" ...)`. */
export const BREAK_UP_TEXTURE = 'ButtBreakUp.bmp';
/** The theme files of the gauge (`m_texGauEmptyNormal`, `m_texGauFillNormal`) and the vertex colour of the fill. */
export const INFO_GAUGE_EMPTY = 'GauEmptyNormal.bmp';
export const INFO_GAUGE_FILL = 'GauFillNormal.bmp';
export const INFO_GAUGE_COLOR = 0x64ff0000;

const ID_MARKS = 0x7e61;
const ID_GAUGE = 0x7e62;

/**
 * The rect of the EXP bar in the lower group box (`WIDC_STATIC4`): `x` from `left + 70` to `right - 10`, `y` from `top + 36` to `top + 48`.
 */
export function gaugeRectOf(group: Rect): Rect {
  return { l: group.l + 70, t: group.t + 36, r: group.r - 10, b: group.t + 48 };
}

/** The place of the master / hero mark: `( WIDC_LEVEL.right - 60, WIDC_LEVEL.top - 1 )`. */
export function markPointOf(level: Rect): { x: number; y: number } {
  return { x: level.r - 60, y: level.t - 1 };
}

export interface CoupleInfoDeps {
  readonly view: ViewSource<CoupleInfoView>;
  readonly actions: CoupleActions;
  readonly atlases: UiAtlases;
}

export class CoupleInfoPage extends WndNeuz {
  readonly appId = APP_COUPLE_TAB_INFO;
  readonly #deps: CoupleInfoDeps;
  #marks: SpriteLayer | null = null;
  #gauge: Gauge | null = null;
  #gaugeWidth = 0;
  #applied: CoupleInfoView | null = null;
  #unsubscribe: (() => void) | null = null;

  constructor(deps: CoupleInfoDeps) {
    super();
    this.#deps = deps;
  }

  get gauge(): Gauge | null {
    return this.#gauge;
  }
  get markLayer(): SpriteLayer | null {
    return this.#marks;
  }
  /** `WIDC_BUTTON1`: the break-up button. */
  get breakButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  stat(id: number): Static | null {
    return this.getDlgItem<Static>(id);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.breakButton?.setTexture(BREAK_UP_TEXTURE);
    const client = this.getClientRect(false);
    this.#marks = this.addControl(new SpriteLayer(), ID_MARKS, client);
    const group = this.getWndCtrl(WIDC_STATIC4)?.rect;
    if (group) {
      const r = gaugeRectOf(group);
      this.#gaugeWidth = r.r - r.l;
      this.#gauge = this.addControl(
        new Gauge({ background: INFO_GAUGE_EMPTY, fill: INFO_GAUGE_FILL, color: INFO_GAUGE_COLOR }),
        ID_GAUGE,
        r,
      );
    }
    this.#apply(this.#deps.view.get());
    this.#unsubscribe = this.#deps.view.subscribe(() => {
      this.#apply(this.#deps.view.get());
    });
  }

  override process(now: number): void {
    const v = this.#deps.view.get();
    if (v !== this.#applied) this.#apply(v);
    super.process(now);
  }

  /** `OnDraw`: the statics, the mark and the gauge follow the view. */
  #apply(v: CoupleInfoView): void {
    this.#applied = v;
    const set = (id: number, text: string): void => {
      const s = this.stat(id);
      if (!s) return;
      s.setTextColor(INFO_TEXT_COLOR);
      s.setTitle(text);
    };
    set(WIDC_NAME, v.name);
    set(WIDC_JOB, v.job);
    set(WIDC_LEVEL, v.level);
    set(WIDC_COUPLE_LEVEL, v.coupleLevel);
    this.#paintMark(v);
    // `rectTemp.right = rectTemp.left + (int)( rect.Width() * fRate )`: the fill is in whole pixels of the bar
    const fill = Math.trunc(Math.fround(Math.fround(this.#gaugeWidth) * Math.fround(v.rate)));
    this.#gauge?.set(Math.max(0, Math.min(fill, this.#gaugeWidth)), this.#gaugeWidth);
  }

  #paintMark(v: CoupleInfoView): void {
    const entries: SpriteEntry[] = [];
    const level = this.getWndCtrl(WIDC_LEVEL)?.rect;
    const dto = this.#deps.atlases.info(PLAYER_DATA_ATLAS);
    const cut = v.mark === null ? undefined : dto?.sprites[v.mark];
    if (level && dto && cut) {
      const at = markPointOf(level);
      entries.push({
        key: 'mark',
        path: dto.texture,
        x: at.x,
        y: at.y,
        w: cut.w,
        h: cut.h,
        src: { x: cut.x, y: cut.y },
      });
    }
    this.#marks?.setEntries(entries);
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON1) {
      // `if( m_pCouple )` + the partner's data: the view says when both hold
      if ((this.#applied ?? EMPTY_COUPLE_INFO).canBreak) this.#deps.actions.breakUp();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = null;
    super.onDestroy();
  }
}
