// The status window's read model (`StatusView` of `ui/windows/char/ports.ts`; Plan/45 §7.2, WP-CFEAT-09): the gauges and the
// title from `selfStore`, the display switches from `features.*`, the EXP stop state from the mode word.

import type { ClientContext } from '../../../app/context';
import type { TextApi } from '../../../data';
import { TID_GAME_WND_STATUS_PLAYER_INFORMATION } from '../../../gen/defs/text';
import type { StatusView } from '../../../ui/windows/char/ports';
import type { SelfState } from '../../../stores/self';
import { formatTid } from '../../common/text';
import { derivedView, type ViewSource } from '../../common/view';
import { eventOf } from '../../events/api';

/** `MODE_EXPUP_STOP` (0x0004_0000) of the mode word: the experience gain is stopped (the button shows its pressed frame). */
export const MODE_EXPUP_STOP = 0x0004_0000;

/** The window title `TID_GAME_WND_STATUS_PLAYER_INFORMATION(name, level)`; the pre-V9 form `"%s Lv %d"` without the texts. */
export function statusTitle(texts: TextApi, name: string, level: number): string {
  const t = formatTid(texts, TID_GAME_WND_STATUS_PLAYER_INFORMATION, [name, level]);
  return t === '' ? `${name} Lv ${String(level)}` : t;
}

const NO_TEXTS: TextApi = { text: () => '', textColor: () => 0xff000000 };

/** The view of one self state under the feature switches. */
export function statusViewOf(
  s: SelfState,
  title: string,
  f: { improved: boolean; expStop: boolean; removeBtExp: boolean; restPercent?: number },
  subject?: unknown,
): StatusView {
  return {
    title,
    hp: s.hp,
    hpMax: s.maxHp,
    mp: s.mp,
    mpMax: s.maxMp,
    fp: s.fp,
    fpMax: s.maxFp,
    expPercentX100: s.expPercentX100,
    // the guild-house rest bonus (`GuildHouse->m_nExtraExp`, 0 .. 100): its value is the width of the AEXP gauge in px; 0 outside a guild house session
    restPoint: f.restPercent ?? 0,
    // the percent text of the rest bonus is drawn unless `features.remove_bt_exp` (`__REMOVE_BT_EXP`) is on
    restText: !f.removeBtExp,
    improved: f.improved,
    expStopFeature: f.expStop,
    expStopped: (s.modeFlags & MODE_EXPUP_STOP) !== 0,
    ...(subject === undefined ? {} : { subject }),
  };
}

/** The `ViewSource` of the status window: recomputed when `selfStore` changes (<= 10 Hz) or the texts arrive. */
export function createStatusView(ctx: ClientContext): ViewSource<StatusView> {
  const { selfStore } = ctx.stores;
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  // the events feature registers after this one: its store is looked up when the first window subscribes
  const eventsSource = {
    subscribe: (fn: () => void) => eventOf(ctx)?.store.subscribe(fn) ?? (() => undefined),
  };
  return derivedView(() => {
    const s = selfStore.getState();
    const f = ctx.features.features;
    return statusViewOf(
      s,
      statusTitle(ctx.data ?? NO_TEXTS, s.name, s.level),
      {
        improved: f.improved_status,
        expStop: f.exp_stop,
        removeBtExp: f.remove_bt_exp,
        restPercent: eventOf(ctx)?.store.getState().guildHouse.restPercent ?? 0,
      },
      ctx.entities.self()?.visual,
    );
  }, [selfStore, dataSource, eventsSource]);
}
