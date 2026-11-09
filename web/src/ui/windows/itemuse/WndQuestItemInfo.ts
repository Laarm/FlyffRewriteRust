// `WndQuestItemInfo` (`CWndQuestItemInfo`, `src:_Interface/WndField.cpp:11847-11925`, ref/22 §17 D, Plan/42 §19.4 D, 45 §9.4; WP-UI-28):
// `APP_QUESTITEM_INFO` (240 x 208), the information window of a quest item. The layout's edit is hidden; the window draws over its
// rect (deflated by 10): the item picture at the top-left (alpha 200), the name in blue (0xff0000ff) at (+34, +16 - 5) and the
// description wrapped to the width, one line below the picture row (+40).

import { APP_QUESTITEM_INFO, WIDC_EDIT1 } from '../../../gen/defs/resdata';
import { IconStrip } from '../../controls/IconStrip';
import { TextLayer, type TextItem } from '../../controls/TextLayer';
import { type Rect, rectDeflate, rectWidth } from '../../layout/geometry';
import { FONT, lineHeight } from '../../text/fonts';
import { layoutRich, lineText } from '../../text/layout';
import { parseMarkup } from '../../text/markup';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { QuestItemInfoRequest } from './ports';

export class WndQuestItemInfo extends WndNeuz {
  readonly appId = APP_QUESTITEM_INFO;
  readonly #request: QuestItemInfoRequest;
  #strip: IconStrip | null = null;
  #texts: TextLayer | null = null;
  #area: Rect = { l: 0, t: 0, r: 0, b: 0 };

  constructor(request: QuestItemInfoRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get strip(): IconStrip | null {
    return this.#strip;
  }
  get texts(): TextLayer | null {
    return this.#texts;
  }
  /** The drawing area: the edit's rect deflated by 10. */
  get area(): Rect {
    return this.#area;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const edit = this.getDlgItem(WIDC_EDIT1);
    edit?.setVisible(false);
    edit?.enableWindow(false);
    const rect = this.getWndCtrl(WIDC_EDIT1)?.rect ?? { l: 0, t: 0, r: 0, b: 0 };
    this.#area = rectDeflate(rect, 10, 10, 10, 10);
    this.#strip = this.addControl(new IconStrip(), 0x7e01, this.getClientRect(false));
    this.#texts = this.addControl(new TextLayer(), 0x7e02, this.getClientRect(false));
    const a = this.#area;
    this.#strip.setEntries([
      { key: 'icon', icon: this.#request.icon, x: a.l, y: a.t, w: 32, h: 32, alpha: 200 },
    ]);
    const items: TextItem[] = [
      { x: a.l + 34, y: a.t + 16 - 5, text: this.#request.name, color: 0xff0000ff },
    ];
    const lines = layoutRich(
      parseMarkup(this.#request.description, 0xff000000),
      rectWidth(a),
      FONT.text,
    );
    const cell = lineHeight(FONT.text);
    lines.forEach((line, i) => {
      items.push({ x: a.l, y: a.t + 40 + i * cell, text: lineText(line), color: 0xff000000 });
    });
    this.#texts.setItems(items);
    this.moveParentCenter();
  }
}
