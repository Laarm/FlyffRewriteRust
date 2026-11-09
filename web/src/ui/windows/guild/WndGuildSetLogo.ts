// `WndGuildSetLogo` (`CWndGuildSetLogo`, `src:_Interface/WndGuildTabInfo.cpp:200-340`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-46): `APP_GUILD_SETLOGO` (208 x 240), the logo picker.
// A MODAL window: logos 1..20 in a 5 x 4 grid of `Icon_CloakSLogo%02d.jpg` pictures (24 x 24), a click selects one (a magenta frame, two pixels wide, round it), OK sends the choice
// (`GuildLogoReq`) and closes, Cancel closes. A game master gets one more row with the logos 21..27 (the original had a language-dependent GM button whose logo table is not in
// the source; the server accepts 21..27 for a game master) and the window keeps its full height; everybody else gets the shorter window (`rectWindow.bottom = 208`) and
// no GM button. FIX: OK without a selection sends nothing (the original sent logo 0).

import { APP_GUILD_SETLOGO, WIDC_BUTTON1, WIDC_CANCEL, WIDC_OK } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import { RectLayer, type RectEntry } from '../../controls/RectLayer';
import { SpriteLayer, type SpriteEntry } from '../../controls/SpriteLayer';
import type { Point } from '../../layout/geometry';
import { WBS_MODAL, Wnm } from '../../layout/styles';
import type { Shortcut } from '../../dnd/shortcut';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { logoAt, logoCell } from './logoGrid';
import { GUILD_LOGO_COUNT, guildLogoPath, guildLogoSize } from './logoPath';
import type { LogoRequest } from './ports';

const ID_SPRITES = 0x7f41;
const ID_FRAME = 0x7f42;
/** The window height of a player who is not a game master (`rectWindow.bottom = 208`). */
export const PLAYER_HEIGHT = 208;
const MAGENTA = 'rgb(255, 0, 255)';

export class WndGuildSetLogo extends WndNeuz {
  readonly appId = APP_GUILD_SETLOGO;
  readonly #request: LogoRequest;
  #selected: number;
  #sprites: SpriteLayer | null = null;
  #frame: RectLayer | null = null;

  constructor(request: LogoRequest) {
    super();
    this.#request = request;
    this.#selected = request.current;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  /** `m_nSelectLogo`: 0 = none. */
  get selected(): number {
    return this.#selected;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_OK);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_CANCEL);
  }
  get frameLayer(): RectLayer | null {
    return this.#frame;
  }
  get spriteLayer(): SpriteLayer | null {
    return this.#sprites;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    // the GM button of the layout is replaced by the row of the logos 21..27
    const gm = this.getDlgItem<Button>(WIDC_BUTTON1);
    if (gm) {
      gm.setVisible(false);
      gm.enableWindow(false);
    }
    if (!this.#request.gm) {
      const r = this.rectWindow;
      this.setWndRect({ l: r.l, t: r.t, r: r.r, b: r.t + PLAYER_HEIGHT });
    }
    const client = this.getClientRect(false);
    this.#sprites = this.addControl(new SpriteLayer(), ID_SPRITES, client);
    this.#frame = this.addControl(new RectLayer(), ID_FRAME, client);
    this.#paint();
    this.moveParentCenter();
  }

  #paint(): void {
    const last = this.#request.gm ? GUILD_LOGO_COUNT : GUILD_LOGO_COUNT - 7;
    const sprites: SpriteEntry[] = [];
    for (let n = 1; n <= last; n += 1) {
      const at = logoCell(n);
      const path = guildLogoPath(n);
      if (!at || path === null) continue;
      sprites.push({
        key: `logo${String(n)}`,
        path,
        x: at.l,
        y: at.t,
        w: guildLogoSize(n),
        h: guildLogoSize(n),
      });
    }
    this.#sprites?.setEntries(sprites);
    const frames: RectEntry[] = [];
    const sel = logoCell(this.#selected);
    if (sel) {
      for (const grow of [1, 2]) {
        frames.push({
          key: `frame${String(grow)}`,
          x: sel.l - grow,
          y: sel.t - grow,
          w: sel.r - sel.l + 2 * grow,
          h: sel.b - sel.t + 2 * grow,
          outline: { color: MAGENTA, width: 1 },
        });
      }
    }
    this.#frame?.setEntries(frames);
  }

  /** `OnLButtonUp`: a click on a picture selects it. */
  override onLButtonUp(pt: Point): void {
    const n = logoAt(pt.x, pt.y, this.#request.gm);
    if (n === 0) return;
    this.#selected = n;
    this.#paint();
  }

  /** Selects logo `n` (tests, the probe). */
  select(n: number): void {
    if (logoCell(n) === null) return;
    this.#selected = n;
    this.#paint();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED) {
      if (id === WIDC_OK) {
        if (this.#selected === 0) return true;
        this.#request.pick(this.#selected);
        this.destroy();
        return true;
      }
      if (id === WIDC_CANCEL) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
