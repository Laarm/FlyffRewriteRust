// `WndSecretRoomGuildMember` (`CWndSecretRoomGuildMember`, `src:_Interface/WndSecretRoom.cpp:1286-1835`, ref/22 §17 K, Plan/42 §19.4 K; WP-UI-57): `APP_SECRETROOM_GUILDMEMBER` (752, 144 x 80), the one-member
// box of the old quick window: the title is the member's name, `STATIC1` holds "Lv.%d" (black, grey out of view) and the HP gauge 22 px under the top (`GauEmptyNormal.bmp` /
// `GauFillNormal.bmp`, fill `0x64ff0000`, empty out of view). V15 comments the class out (no opener): a SHELL over the real layout for CA1 / CA2 (`reachable: false`).

import { APP_SECRETROOM_GUILDMEMBER, WIDC_STATIC1 } from '../../../gen/defs/resdata';
import { Gauge } from '../../controls/Gauge';
import type { Static } from '../../controls/Static';
import { TextLayer } from '../../controls/TextLayer';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { GuildMemberRequest } from './ports';

const GAUGE_EMPTY = 'GauEmptyNormal.bmp';
const GAUGE_FILL = 'GauFillNormal.bmp';
const ID_TEXT = 0x7d71;
const ID_GAUGE = 0x7d72;
const STEPS = 1000;

export class WndSecretRoomGuildMember extends WndNeuz {
  readonly appId = APP_SECRETROOM_GUILDMEMBER;
  readonly #request: GuildMemberRequest;
  #text: TextLayer | null = null;
  #gauge: Gauge | null = null;

  constructor(request: GuildMemberRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get box(): Static | null {
    return this.getDlgItem<Static>(WIDC_STATIC1);
  }
  get gauge(): Gauge | null {
    return this.#gauge;
  }
  get textLayer(): TextLayer | null {
    return this.#text;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const r = this.#request;
    this.setTitle(r.name);
    const rect = this.getWndCtrl(WIDC_STATIC1)?.rect ?? { l: 4, t: 6, r: 128, b: 42 };
    this.#text = this.addControl(new TextLayer(), ID_TEXT, this.getClientRect(false));
    this.#text.setItems([
      {
        x: 60,
        y: rect.t + 8,
        text: `Lv.${String(r.level)}`,
        color: r.inView ? 0xff000000 : 0xff878787,
      },
    ]);
    this.#gauge = this.addControl(
      new Gauge({ background: GAUGE_EMPTY, fill: GAUGE_FILL, color: 0x64ff0000 }),
      ID_GAUGE,
      { l: rect.l + 2, t: rect.t + 22, r: rect.r - 2, b: rect.b },
    );
    this.#gauge.set(r.inView ? Math.round(Math.max(0, Math.min(1, r.hp)) * STEPS) : 0, STEPS);
    this.moveParentCenter();
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
