// The debug probe of the 1:1 line-up editor (`__flyffDebug.features.gc1to1Selection()`, Plan/45 §2.6): what the Playwright flow (`e2e/minigames/gc1to1.spec.ts`) reads instead of pixels.
//
//   gc1to1Selection()   `null` while the editor (672) is closed, else the plain texts of both lists, the guild-member ids behind the rows and the CSS pixel centres of the rows
//
// The lord feature's `gc1to1Windows()` stays the probe of the status window (676).

import type { ClientContext } from '../../../app/context';
import type { ListBox } from '../../../ui/controls/ListBox';
import type { Rect } from '../../../ui/layout/geometry';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';

/** One row of a list: its index, the member id behind it and the CSS pixel centre. */
export interface Gc1to1SelectionRow {
  index: number;
  id: number;
  x: number;
  y: number;
}

/** What `gc1to1Selection()` returns. */
export interface Gc1to1SelectionProbe {
  guildRows: string[];
  lineupRows: string[];
  guildIds: number[];
  lineup: number[];
  guildPoints: Gc1to1SelectionRow[];
  lineupPoints: Gc1to1SelectionRow[];
}

const plain = (text: string): string =>
  text
    .replace(/#c[0-9a-fA-F]{8}|#nc|#nb|#b|#nu|#u/g, '')
    .replace(/\s+/g, ' ')
    .trim();

const listTexts = (list: ListBox | null | undefined): string[] =>
  list ? Array.from({ length: list.getCount() }, (_, i) => plain(list.getPlainText(i))) : [];

export function registerGc1to1Probe(ctx: ClientContext): void {
  // the CSS pixel centre of a rectangle given in the coordinates of `origin`
  const css = (origin: Rect, r: Rect): { x: number; y: number } => {
    const scale = fullUi(ctx)?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    return {
      x: Math.round((root?.left ?? 0) + (origin.l + (r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + (origin.t + (r.t + r.b) / 2) * scale),
    };
  };
  const points = (list: ListBox | null | undefined, ids: readonly number[]) => {
    const out: Gc1to1SelectionRow[] = [];
    if (!list) return out;
    const origin = list.getScreenClientRect();
    for (let i = 0; i < list.getCount(); i += 1) {
      const r = list.itemRect(i);
      if (r) out.push({ index: i, id: ids[i] ?? 0, ...css(origin, r) });
    }
    return out;
  };

  addProbeMember(ctx, 'gc1to1Selection', (): Gc1to1SelectionProbe | null => {
    const sel = fullUi(ctx)?.secretRoom.gc11Selection();
    if (!sel) return null;
    return {
      guildRows: listTexts(sel.guildList),
      lineupRows: listTexts(sel.lineupList),
      guildIds: [...sel.guildIds],
      lineup: [...sel.lineup],
      guildPoints: points(sel.guildList, sel.guildIds),
      lineupPoints: points(sel.lineupList, sel.lineup),
    };
  });
}
