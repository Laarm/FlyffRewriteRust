// The debug probe of the siege windows (`__flyffDebug.features.siegeWindows()` and `siegePoints()`, Plan/45 §2.6): what the Playwright flow (`e2e/siege`) reads instead of pixels.
//
//   siegeWindows()   the plain texts of the open siege windows: the bid window, the status list, the line-up editor, the gate choice, the boards and confirms, the result tabs, the ranking pages
//   siegePoints()    the CSS pixel centres the spec clicks that no `controlPoint( app, id )` reaches: the rows of the two line-up lists, the tab strips of the result and ranking windows

import type { ClientContext } from '../../../app/context';
import type { ListBox } from '../../../ui/controls/ListBox';
import type { TabCtrl } from '../../../ui/controls/TabCtrl';
import type { Rect } from '../../../ui/layout/geometry';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';

const ZERO: Rect = { l: 0, t: 0, r: 0, b: 0 };

interface Pt {
  x: number;
  y: number;
}

/** The text of a markup string without its colour and style tags (the probe compares plain texts). */
const plain = (text: string): string =>
  text
    .replace(/#c[0-9a-fA-F]{8}|#nc|#nb|#b|#nu|#u/g, '')
    .replace(/\s+/g, ' ')
    .trim();

function listTexts(list: ListBox | null | undefined): string[] {
  if (!list) return [];
  return Array.from({ length: list.getCount() }, (_, i) => plain(list.getPlainText(i)));
}

export function registerSiegeProbe(ctx: ClientContext): void {
  // the CSS pixel centre of a rectangle given in the coordinates of `origin` (root coordinates when `origin` is zero)
  const css = (origin: Rect, r: Rect): Pt => {
    const ui = fullUi(ctx);
    const scale = ui?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    return {
      x: Math.round((root?.left ?? 0) + (origin.l + (r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + (origin.t + (r.t + r.b) / 2) * scale),
    };
  };
  // the centre of the strip `index` of a tab control (the last 18 px, `clickLength` wide each)
  const strip = (tab: TabCtrl | null | undefined, index: number): Pt | null => {
    if (!tab) return null;
    const r = tab.getScreenRect();
    const len = tab.clickLength();
    return css(ZERO, { l: r.l + len * index, t: r.b - 18, r: r.l + len * (index + 1), b: r.b });
  };
  const rows = (list: ListBox | null | undefined): { index: number; x: number; y: number }[] => {
    if (!list) return [];
    const origin = list.getScreenClientRect();
    const out: { index: number; x: number; y: number }[] = [];
    for (let i = 0; i < list.getCount(); i += 1) {
      const r = list.itemRect(i);
      if (r) out.push({ index: i, ...css(origin, r) });
    }
    return out;
  };

  addProbeMember(ctx, 'siegeWindows', () => {
    const s = fullUi(ctx)?.siege;
    if (!s) return null;
    const offer = s.offer();
    const state = s.offerState();
    const sel = s.selection();
    const start = s.joinSelection();
    const result = s.result();
    const person = s.rankPerson();
    return {
      open: s.openApps(),
      offer: offer
        ? {
            mode: offer.mode,
            edit: offer.edit?.getString() ?? '',
            acceptEnabled: offer.acceptButton?.isWindowEnabled() ?? false,
            info: plain(offer.infoText?.getString() ?? ''),
          }
        : null,
      state: state
        ? {
            time: state.timeStatic?.getTitle() ?? '',
            rank: state.rankStatic?.getTitle() ?? '',
            bid: state.bidStatic?.getTitle() ?? '',
            prize: state.prizeStatic?.getTitle() ?? '',
            rows: listTexts(state.list),
          }
        : null,
      selection: sel
        ? {
            guildRows: listTexts(sel.guildList),
            lineupRows: listTexts(sel.lineupList),
            guildIds: [...sel.guildIds],
            lineup: [...sel.lineup],
            defender: sel.defender,
          }
        : null,
      selStart: start
        ? {
            text: plain(start.messageStatic?.getTitle() ?? ''),
            map: start.map,
            leftMs: start.leftMs(),
          }
        : null,
      board: plain(s.board()?.textCtrl?.getString() ?? ''),
      infoBox: plain(s.infoBox()?.textCtrl?.getString() ?? ''),
      cancel: plain(s.cancelConfirm()?.textCtrl?.getString() ?? ''),
      join: plain(s.joinConfirm()?.textCtrl?.getString() ?? ''),
      result: result
        ? {
            tab: result.tab?.getCurSel() ?? -1,
            guildRows: listTexts(result.ratePage?.guildList),
            personRows: listTexts(result.ratePage?.personList),
            log: plain(result.logPage?.textCtrl?.getString() ?? ''),
          }
        : null,
      rankPerson: person
        ? {
            tab: person.tab?.getCurSel() ?? -1,
            pages: person.pages.map((p) =>
              p.rows.map((r) => ({ rank: r.rank, name: r.name, job: r.job, points: r.points })),
            ),
          }
        : null,
    };
  });

  addProbeMember(ctx, 'siegePoints', () => {
    const s = fullUi(ctx)?.siege;
    if (!s) return null;
    const sel = s.selection();
    const result = s.result();
    const person = s.rankPerson();
    return {
      guildRows: rows(sel?.guildList).map((r) => ({ ...r, id: sel?.guildIds[r.index] ?? 0 })),
      lineupRows: rows(sel?.lineupList).map((r) => ({ ...r, id: sel?.lineup[r.index] ?? 0 })),
      resultTabs: [0, 1].map((i) => strip(result?.tab, i)),
      rankTabs: [0, 1, 2, 3, 4].map((i) => strip(person?.tab, i)),
    };
  });
}
