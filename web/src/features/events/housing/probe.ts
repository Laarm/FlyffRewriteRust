// The window probe of the housing and guild-house windows (`__flyffDebug.features.houseWindows()` and `housePoints()`, Plan/45 §2.6): what the Playwright flows (`e2e/house`) read instead of pixels.
//
//   houseWindows()   the plain texts of the open windows: the furniture list (rows, selection, description, buttons), the room list, the guild-house menu countdown, the storage list (section, rows, the SHOWONE item) and the upkeep box
//   housePoints()    the CSS pixel centres the spec clicks that no `controlPoint( app, id )` reaches: the rows of the furniture list and of the storage list, the seven row buttons

import type { ClientContext } from '../../../app/context';
import {
  APP_GH_SHOWONE,
  APP_HOUSING,
  APP_MINIROOM_LIST,
  APP_QUIT_ROOM,
} from '../../../gen/defs/resdata';
import type { ListBox } from '../../../ui/controls/ListBox';
import type { Rect } from '../../../ui/layout/geometry';
import type { HouseList } from '../../../ui/windows/housing/HouseList';
import type { WndHousing } from '../../../ui/windows/housing/WndHousing';
import type { WndRoomList } from '../../../ui/windows/housing/WndRoomList';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';

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

export function registerHouseProbe(ctx: ClientContext): void {
  // the CSS pixel centre of a rectangle in root (virtual screen) coordinates
  const css = (r: Rect): Pt => {
    const scale = fullUi(ctx)?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    return {
      x: Math.round((root?.left ?? 0) + ((r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + ((r.t + r.b) / 2) * scale),
    };
  };
  // the rows of a list box (the room list): the centre of each item rect
  const boxRows = (list: ListBox | null | undefined): (Pt & { index: number })[] => {
    if (!list) return [];
    const origin = list.getScreenClientRect();
    const out: (Pt & { index: number })[] = [];
    for (let i = 0; i < list.getCount(); i += 1) {
      const r = list.itemRect(i);
      if (r)
        out.push({
          index: i,
          ...css({ l: origin.l + r.l, t: origin.t + r.t, r: origin.l + r.r, b: origin.t + r.b }),
        });
    }
    return out;
  };
  const rowPoints = (list: HouseList | null | undefined): (Pt & { index: number })[] => {
    if (!list) return [];
    const out: (Pt & { index: number })[] = [];
    for (let i = 0; i < list.rowCount; i += 1) {
      const r = list.rowScreenRect(i);
      // a row below the visible page has no rect
      if (r && i < list.scrollRow + list.visibleRows) out.push({ index: i, ...css(r) });
    }
    return out;
  };

  addProbeMember(ctx, 'houseWindows', () => {
    const ui = fullUi(ctx);
    if (!ui) return null;
    const list = ui.wm.get(APP_HOUSING) as WndHousing | null;
    const rooms = ui.wm.get(APP_MINIROOM_LIST) as WndRoomList | null;
    const main = ui.housing.main();
    const storage = ui.housing.storage();
    const one = ui.housing.showOne();
    const upkeep = ui.housing.upkeep();
    return {
      list: list
        ? {
            rows: list.items.map((i) => i.name),
            selected: list.selected,
            description: plain(list.description?.getString() ?? ''),
            install: list.installButton?.isWindowEnabled() ?? false,
            remove: list.removeButton?.isWindowEnabled() ?? false,
            texts: (list.list?.rows ?? []).map((r) => r.cells.map((c) => c.text)),
          }
        : null,
      roomList: rooms ? { rows: rooms.rows } : null,
      quitRoom: ui.wm.isOpen(APP_QUIT_ROOM),
      main: main ? { countdown: plain(main.countdown?.getTitle() ?? '') } : null,
      storage: storage
        ? {
            section: storage.section,
            visible: storage.isVisible(),
            deploying: storage.deploying,
            rows: storage.rows.map((r) => ({
              slot: r.slot,
              name: r.name,
              installed: r.installed,
            })),
            selected: storage.selectedRow?.name ?? null,
            buttons: storage.rowButtons.map((b) => ({
              visible: b.isVisible(),
              enabled: b.isWindowEnabled(),
            })),
            cells: (storage.list?.rows ?? []).map((r) => r.cells.map((c) => c.text)),
          }
        : null,
      showOne: one
        ? { open: ui.wm.isOpen(APP_GH_SHOWONE), item: one.item.name, bar: one.barVisible }
        : null,
      upkeep: upkeep ? { days: upkeep.days, cost: plain(upkeep.costText?.getTitle() ?? '') } : null,
    };
  });

  addProbeMember(ctx, 'housePoints', () => {
    const ui = fullUi(ctx);
    if (!ui) return null;
    const list = ui.wm.get(APP_HOUSING) as WndHousing | null;
    const rooms = ui.wm.get(APP_MINIROOM_LIST) as WndRoomList | null;
    const storage = ui.housing.storage();
    return {
      listRows: rowPoints(list?.list),
      roomRows: boxRows(rooms?.list),
      storageRows: rowPoints(storage?.list),
      storageButtons: (storage?.rowButtons ?? []).map((b, i) => ({
        index: i,
        visible: b.isVisible(),
        ...css(b.getScreenRect()),
      })),
    };
  });
}
