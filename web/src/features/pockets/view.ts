// The read model of the pocket window (`PocketView` of `ui/windows/pockets/ports.ts`, Plan/45 §9.7; WP-CFEAT-29): the store's pockets turned into cells of the
// capacity of each pocket, the availability of `CPocketController::IsAvailable`, and the tooltip text of the bag icons. The view keeps its identity while
// nothing it shows changed (the window polls it at 10 Hz).

import type { ClientDataIndex } from '../../data';
import { TID_TOOLTIP_BAG_INFO, TID_TOOLTIP_BAG_TIME } from '../../gen/defs/text';
import type { PocketPanelView, PocketView } from '../../ui/windows/pockets/ports';
import { ItemViewCache } from '../common/itemView';
import { formatTemplate } from '../common/text';
import { pocketAvailable, POCKET_COUNT, type PocketsState } from './state';

export interface PocketViewEnv {
  data(): Pick<ClientDataIndex, 'item' | 'version'> | null | undefined;
  /** `features.perm_extrabags`. */
  permExtrabags(): boolean;
  /** `features.new_font_color`. */
  newFontColor(): boolean;
  /** Seconds since the epoch. */
  nowSec(): number;
  /** The template of a text id. */
  text(tid: number): string;
}

/** `CTimeSpan( remain )`: days, hours and minutes of a duration in seconds (never negative). */
export function timeLeft(seconds: number): { days: number; hours: number; minutes: number } {
  const s = Math.max(0, Math.trunc(seconds));
  return {
    days: Math.floor(s / 86_400),
    hours: Math.floor((s % 86_400) / 3_600),
    minutes: Math.floor((s % 3_600) / 60),
  };
}

export class PocketViewBuilder {
  readonly #items: ItemViewCache;
  #last: PocketView | null = null;
  #lastKey: readonly unknown[] = [];

  constructor(private readonly env: PocketViewEnv) {
    this.#items = new ItemViewCache(() => env.data());
  }

  /** `IsAvailable( pocket )`. */
  available(state: PocketsState, i: number): boolean {
    const p = state.pockets[i];
    return pocketAvailable(this.env.permExtrabags(), i, p?.exists ?? false, p?.expired ?? true);
  }

  build(state: PocketsState): PocketView {
    const permanent = this.env.permExtrabags();
    const fontColor = this.env.newFontColor();
    const key = [state.version, this.env.data()?.version ?? -1, permanent, fontColor];
    if (
      this.#last &&
      key.length === this.#lastKey.length &&
      key.every((v, i) => Object.is(v, this.#lastKey[i]))
    ) {
      return this.#last;
    }
    const pockets = Array.from({ length: POCKET_COUNT }, (_, i): PocketPanelView => {
      const p = state.pockets[i];
      const available = this.available(state, i);
      // an unavailable bag shows nothing (`GetPocket( i )` is NULL, `InitItem` empties the grid)
      const items = Array.from({ length: p?.container.indexNum ?? 0 }, (_x, slot) => {
        const it = available ? (p?.container.getAtSlot(slot) ?? null) : null;
        return it ? this.#items.view(it) : null;
      });
      return {
        index: i,
        available,
        items,
        iconTip: () => this.#tip(state, i, available),
      };
    });
    const view: PocketView = {
      version: state.version,
      permanent,
      newFontColor: fontColor,
      pockets,
    };
    this.#last = view;
    this.#lastKey = key;
    return view;
  }

  /** `OnMouseWndSurface`: the time that is left of an available bag, else the hint; none for pocket 0 and with permanent bags. */
  #tip(state: PocketsState, i: number, available: boolean): string | null {
    if (i === 0 || this.env.permExtrabags()) return null;
    if (!available) return this.env.text(TID_TOOLTIP_BAG_INFO);
    const left = timeLeft((state.pockets[i]?.expireAt ?? 0) - this.env.nowSec());
    return formatTemplate(this.env.text(TID_TOOLTIP_BAG_TIME), [
      left.days,
      left.hours,
      left.minutes,
    ]);
  }
}
