// The `exchange` feature module (Plan/45 §11.2, §18.6; WP-CFEAT-26): the item-exchange lists of the NPCs over `ui.exchange`. The original opens
// `APP_DIALOG_EVENT` from the `default:` branch of `CWndWorld::OnCommand`, so EVERY `MMI_*` id that has no window of its own is an exchange menu;
// here the menu feature's fallback does the same for the ids that the `item-ext` bundle (`Exchange_Script.txt`) lists (Plan/19 §16: 24 menus, 8 of
// them offered by NPCs of the shipped data). The window shows the description pages and one entry per set; OK sends `ExchangeReq{mmi, set}`; the
// answer `ExchangeNtf{state: 1, result}` goes to the window, which shows the success or failure text of the set in a message box.

import type { ClientContext } from '../../app/context';
import type { ExchangeMenu } from '../../gen/bundles';
import { APP_DIALOG_EVENT } from '../../gen/defs/resdata';
import { TID_GAME_LACKSPACE, TID_GAME_NOWUSING } from '../../gen/defs/text';
import type { ExchangeRequest } from '../../ui/windows/exchange/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { menuOf } from '../npc';
import { createExchangeSenders, type ExchangeSenders } from './senders';

/** `EXCHANGE_RESULT`: the only state of `ExchangeNtf` the client handles. */
export const EXCHANGE_RESULT = 1;

export interface ExchangeFeature {
  readonly senders: ExchangeSenders;
  /** The exchange menu of an `MMI_*` id from the bundle, `undefined` when the script has none. */
  menu(mmi: number): ExchangeMenu | undefined;
  /** Opens the list of `mmi` as the menu fallback does (`TID_GAME_NOWUSING` when open); `false` when the script has no such menu or no sets. */
  open(mmi: number): boolean;
  readonly stats: { opened: number; sent: number; results: Record<string, number> };
}

const features = new WeakMap<ClientContext, ExchangeFeature>();

/** The exchange feature of a context; `undefined` before the module registered. */
export function exchangeOf(ctx: ClientContext): ExchangeFeature | undefined {
  return features.get(ctx);
}

export const exchangeModule: FeatureModule = {
  id: 'exchange',
  milestone: 4,
  register(fc) {
    registerExchange(fc);
  },
};

function registerExchange(fc: FeatureContext): ExchangeFeature {
  const ctx = fc.app;
  const senders = createExchangeSenders(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const stats: ExchangeFeature['stats'] = { opened: 0, sent: 0, results: {} };

  const menu = (mmi: number): ExchangeMenu | undefined =>
    ctx.data?.bundle('item-ext')?.exchange.find((m) => m.menu === mmi);

  /** The request of the window for one menu: the texts are resolved here, the window never reads the text table. */
  const requestOf = (m: ExchangeMenu): ExchangeRequest => ({
    mmi: m.menu,
    entries: m.sets.map((s) => text(s.tid)),
    pages: m.desc_tids.map(text),
    results: (i) => {
      const r = m.sets[i]?.result_msgs;
      // `if( vResult.size() == 2 )`: a set with another count says nothing here (the server prints the details itself)
      return r?.length === 2 ? [text(r[0] ?? 0), text(r[1] ?? 0)] : null;
    },
    lackSpace: text(TID_GAME_LACKSPACE),
    send: (index) => {
      senders.exchange(m.menu, index);
      stats.sent += 1;
    },
    messageBox: (t) => {
      fullUi(ctx)?.msgbox.open(t);
    },
  });

  const open = (mmi: number): boolean => {
    const ui = fullUi(ctx);
    const m = menu(mmi);
    // `GetListTextId( nMMI )` empty: the window destroys itself at once, so nothing opens
    if (!ui || !m || m.sets.length === 0) return false;
    if (ui.wm.isOpen(APP_DIALOG_EVENT)) {
      sysChat(fc.texts, ui.chat, TID_GAME_NOWUSING);
      return true;
    }
    const w = ui.exchange.open(requestOf(m));
    if (w) stats.opened += 1;
    return w !== null;
  };

  menuOf(ctx)?.registerFallback((mmi) => open(mmi));

  fc.net.on('exchangeNtf', (m) => {
    if (m.state !== EXCHANGE_RESULT) return;
    const key = String(m.result);
    stats.results[key] = (stats.results[key] ?? 0) + 1;
    fullUi(ctx)?.exchange.window()?.receiveResult(m.result);
  });

  const feature: ExchangeFeature = { senders, menu, open, stats };
  features.set(ctx, feature);
  addProbeMember(ctx, 'exchange', () => ({
    ...stats,
    results: { ...stats.results },
    open: fullUi(ctx)?.wm.isOpen(APP_DIALOG_EVENT) ?? false,
    menus: (ctx.data?.bundle('item-ext')?.exchange ?? []).map((x) => x.menu),
  }));
  addProbeMember(ctx, 'openExchange', (mmi?: number) => (mmi === undefined ? false : open(mmi)));
  return feature;
}
