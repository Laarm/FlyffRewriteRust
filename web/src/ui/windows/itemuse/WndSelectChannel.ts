// `WndSelectChannel` (`CWndSelectCh`, `src:_Interface/WndSelectCh.cpp`, `__AZRIA_1023`, ref/22 §17 B, Plan/42 §19.4 B, 45 §9.6; WP-UI-37 /
// WP-CFEAT-15): `APP_SELECT_CHANNEL` (176 x 256), the layer selector of a ticket used outside its own world. The list box holds `count`
// entries, entry `i` reads `"<Channel>\t<i + 1>"`; a click on an entry selects it and sends `SendDoUseItemInput(item, "-<i>")` (the base
// layer is `"0"`: `nSelect = GetCurSel() * -1`, "the keys of the layers the system defines decrease from 0, those of the layers players make
// (housing, instance dungeons) are positive"), then the window closes. The tests of the key strings are the contract of the server side (M7).

import { APP_SELECT_CHANNEL, WIDC_LISTBOX1 } from '../../../gen/defs/resdata';
import type { ListBox } from '../../controls/ListBox';
import type { Shortcut } from '../../dnd/shortcut';
import { Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { SelectChannelRequest } from './ports';

/** The key string of the entry `index`: `"0"`, `"-1"`, `"-2"` ... (`_itoa(GetCurSel() * -1)`; the 0 has no sign). */
export function channelKey(index: number): string {
  return index === 0 ? '0' : String(-index);
}

export class WndSelectChannel extends WndNeuz {
  readonly appId = APP_SELECT_CHANNEL;
  readonly #request: SelectChannelRequest;
  #list: ListBox | null = null;

  constructor(request: SelectChannelRequest) {
    super();
    this.#request = request;
    this.putRegInfo = false;
  }

  get list(): ListBox | null {
    return this.#list;
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const list = this.getDlgItem<ListBox>(WIDC_LISTBOX1);
    this.#list = list;
    list?.resetContent();
    for (let i = 0; i < this.#request.count; i += 1) {
      list?.addString(`${this.#request.label}\t${String(i + 1)}`);
    }
    this.moveParentCenter();
  }

  /** The click on entry `index` (the `WIDC_LISTBOX1` notification of the original). */
  choose(index: number): void {
    if (index < 0 || index >= this.#request.count) return;
    this.#request.choose(channelKey(index));
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.SELCHANGE && id === WIDC_LISTBOX1) {
      this.choose(this.#list?.getCurSel() ?? -1);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}
