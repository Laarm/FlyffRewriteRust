// The debug probe of the secret-room windows (`__flyffDebug.features.secretRoomWindows()` and `secretRoomPoints()`, Plan/45 §2.6): what the Playwright flow (`e2e/secretroom`) reads instead of pixels.
//
//   secretRoomWindows()   the plain texts of the open secret-room windows: the bid, the status list, the line-up editor, the board and boxes, the tax windows, the member list
//   secretRoomPoints()    the CSS pixel centres of the rows of the two line-up lists

import type { ClientContext } from '../../../app/context';
import type { ListBox } from '../../../ui/controls/ListBox';
import type { Rect } from '../../../ui/layout/geometry';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';

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

export function registerSecretRoomProbe(ctx: ClientContext): void {
  // the CSS pixel centre of a rectangle given in the coordinates of `origin`
  const css = (origin: Rect, r: Rect): { x: number; y: number } => {
    const ui = fullUi(ctx);
    const scale = ui?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    return {
      x: Math.round((root?.left ?? 0) + (origin.l + (r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + (origin.t + (r.t + r.b) / 2) * scale),
    };
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

  addProbeMember(ctx, 'secretRoomWindows', () => {
    const s = fullUi(ctx)?.secretRoom;
    if (!s) return null;
    const offer = s.offer();
    const state = s.offerState();
    const sel = s.selection();
    const change = s.changeTaxRate();
    const check = s.checkTaxRate();
    const quick = s.quick();
    return {
      open: s.openApps(),
      offer: offer
        ? {
            edit: offer.edit?.getString() ?? '',
            info: plain(offer.infoText?.getString() ?? ''),
            backup: offer.backupGold,
          }
        : null,
      state: state
        ? {
            time: state.timeStatic?.getTitle() ?? '',
            rank: state.rankStatic?.getTitle() ?? '',
            tender: state.tenderStatic?.getTitle() ?? '',
            rows: listTexts(state.list),
          }
        : null,
      selection: sel
        ? {
            guildRows: listTexts(sel.guildList),
            lineupRows: listTexts(sel.lineupList),
            guildIds: [...sel.guildIds],
            lineup: [...sel.lineup],
          }
        : null,
      board: plain(s.board()?.textCtrl?.getString() ?? ''),
      infoBox: plain(s.infoBox()?.textCtrl?.getString() ?? ''),
      cancel: plain(s.cancelConfirm()?.textCtrl?.getString() ?? ''),
      changeTax: change
        ? {
            continent: change.continent,
            salesNow: change.salesNow?.getTitle() ?? '',
            purchaseNow: change.purchaseNow?.getTitle() ?? '',
            salesNew: change.salesNew?.getTitle() ?? '',
            purchaseNew: change.purchaseNew?.getTitle() ?? '',
          }
        : null,
      checkTax: check
        ? {
            sales: check.salesStatic?.getTitle() ?? '',
            purchase: check.purchaseStatic?.getTitle() ?? '',
            guild: check.guildStatic?.getTitle() ?? '',
            master: check.masterStatic?.getTitle() ?? '',
          }
        : null,
      quick: quick
        ? {
            mini: quick.isMini,
            focus: quick.focusId,
            rows: quick.shownRows.map((r) => ({
              id: r.id,
              text: r.text,
              color: r.color,
              hp: r.hp,
            })),
          }
        : null,
    };
  });

  addProbeMember(ctx, 'secretRoomPoints', () => {
    const s = fullUi(ctx)?.secretRoom;
    if (!s) return null;
    const sel = s.selection();
    return {
      guildRows: rows(sel?.guildList).map((r) => ({ ...r, id: sel?.guildIds[r.index] ?? 0 })),
      lineupRows: rows(sel?.lineupList).map((r) => ({ ...r, id: sel?.lineup[r.index] ?? 0 })),
    };
  });
}
