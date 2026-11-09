// `WndRainbowRaceRanking` (`CWndRainbowRaceRanking`, `src:_Interface/WndRainbowRace.cpp:121-268`, ref/22 §17 P, Plan/42 §19.4 P; WP-UI-60): `APP_RAINBOWRACE_RANKING` (772, 272 x 256), the previous ranking. `STATIC4` is the
// background picture of the layout, `STATIC1-3` its column captions; the five rows are drawn at `y = 55 + 24 i`: "`i + 1`." at x 30, the name at x 80 (black) and the job marks of `icon/icon_playerdata.inc`
// (the type mark at x 196, the job at x 218), all read from the player-data cache at DRAW time (`request.player( id )`) and drawn only for a known name. OK closes. Centred.

import { APP_RAINBOWRACE_RANKING, WIDC_OK } from '../../../gen/defs/resdata';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { UiAtlases } from '../../skin/atlas';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { jobMarks, PLAYER_DATA_ATLAS } from '../messenger/icons';
import type { RankingRequest } from './ports';

const ID_TEXT = 0x5200;
const ID_MARKS = 0x5201;
/** The row pitch and the first row (`55 + 24 i`). */
export const RANK_FIRST_Y = 55;
export const RANK_PITCH = 24;
export const RANK_ROWS = 5;
/** The x positions of the number, the name and the two marks. */
export const RANK_X = { number: 30, name: 80, type: 196, job: 218 } as const;
const BLACK = 0xff000000;

export interface RankingDeps {
  readonly atlases: UiAtlases;
}

export class WndRainbowRaceRanking extends WndNeuz {
  readonly appId = APP_RAINBOWRACE_RANKING;
  readonly #request: RankingRequest;
  readonly #deps: RankingDeps;
  #text: TextLayer | null = null;
  #marks: SpriteLayer | null = null;

  constructor(request: RankingRequest, deps: RankingDeps) {
    super();
    this.#request = request;
    this.#deps = deps;
    this.putRegInfo = false;
  }

  get textLayer(): TextLayer | null {
    return this.#text;
  }
  get markLayer(): SpriteLayer | null {
    return this.#marks;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const client = this.getClientRect(false);
    this.#text = this.addControl(new TextLayer(), ID_TEXT, client);
    this.#marks = this.addControl(new SpriteLayer(), ID_MARKS, client);
    this.#refresh();
    this.moveParentCenter();
  }

  /** The rows from the cache now (`OnDraw`). */
  #refresh(): void {
    const items: TextItem[] = [];
    const sprites: SpriteEntry[] = [];
    const atlas = this.#deps.atlases.info(PLAYER_DATA_ATLAS);
    for (let i = 0; i < RANK_ROWS; i += 1) {
      const y = RANK_FIRST_Y + RANK_PITCH * i;
      items.push({ x: RANK_X.number, y, text: `${String(i + 1)}.`, color: BLACK });
      const id = this.#request.ids[i] ?? 0;
      const p = id !== 0 ? this.#request.player(id) : null;
      if (!p || p.name.length === 0) continue;
      items.push({ x: RANK_X.name, y, text: p.name, color: BLACK });
      const marks = jobMarks(p.jobType, p.job, p.level);
      const draw = (index: number | null, x: number, key: string): void => {
        const cut = index === null ? undefined : atlas?.sprites[index];
        if (!atlas || !cut) return;
        sprites.push({
          key: `${key}${String(i)}`,
          path: atlas.texture,
          x,
          y,
          w: cut.w,
          h: cut.h,
          src: { x: cut.x, y: cut.y },
        });
      };
      draw(marks.type, RANK_X.type, 't');
      draw(marks.job, RANK_X.job, 'j');
    }
    this.#text?.setItems(items);
    this.#marks?.setEntries(sprites);
  }

  /** The cache fills while the window is open: the rows follow (10 Hz). */
  override process(now: number): void {
    super.process(now);
    this.#refresh();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_OK) {
      this.destroy();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }
}
