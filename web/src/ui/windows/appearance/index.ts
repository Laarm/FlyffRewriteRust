// `AppearanceWindowsService` (`ui.appearance`, Plan/42 §19.4 L; WP-UI-35): the face window, the sex-change window and the item-transy window.
//
//   ui.appearance.openChangeFace(request)   APP_CHANGEFACE: five faces, the chosen one framed in blue
//   ui.appearance.openChangeSex(request)    APP_CHANGESEX: the preview of the other sex with five faces to page through
//   ui.appearance.openItemTransy(request)   APP_ITEM_TRANSY: the armour slot and its opposite-gender twin
//
// Each `open*` replaces the window of the same kind that is still open (the original's `SAFE_DELETE(m_pWndX); new ...`). The requests carry the
// data and the callbacks; what the callbacks send is 45's (`features/appearance`). The beauty shops (hair, face, their confirmation) are the NPC
// windows' (`ui.npc`).

import { APP_CHANGEFACE, APP_CHANGESEX, APP_ITEM_TRANSY } from '../../../gen/defs/resdata';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { APPEARANCE_LAZY } from './lazy';
import type { ChangeFaceRequest, ChangeSexRequest, ItemTransyRequest } from './ports';
import type { WndChangeFace } from './WndChangeFace';
import type { WndChangeSex } from './WndChangeSex';
import type { WndItemTransy } from './WndItemTransy';

export * from './ports';
// the shells load on demand (`lazy.ts`): only their types are exported here, the helpers live in `geometry.ts`
export type { WndChangeFace, WndChangeSex, WndItemTransy };
export { faceAt, FACE_LEFT, FACE_PITCH, PREVIEW_SHIFT } from './geometry';

export interface AppearanceWindowsDeps {
  readonly wm: WindowManager;
  readonly tooltip: TooltipService;
}

export class AppearanceWindowsService {
  readonly #wm: WindowManager;
  readonly #tooltip: TooltipService;

  constructor(deps: AppearanceWindowsDeps) {
    this.#wm = deps.wm;
    this.#tooltip = deps.tooltip;
  }

  #replace(app: number): void {
    const old = this.#wm.get(app);
    if (!old) return;
    old.destroy();
    this.#wm.flushDestroyed();
  }

  /** Opens the window of a lazy class (`null` while its chunk is still loading: the window then opens by itself). */
  #open<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      this.#replace(a);
    });
  }

  openChangeFace(request: ChangeFaceRequest): WndChangeFace | null {
    return this.#open(APP_CHANGEFACE, APPEARANCE_LAZY.WndChangeFace, (C) => new C(request));
  }

  openChangeSex(request: ChangeSexRequest): WndChangeSex | null {
    return this.#open(APP_CHANGESEX, APPEARANCE_LAZY.WndChangeSex, (C) => new C(request));
  }

  openItemTransy(request: ItemTransyRequest): WndItemTransy | null {
    return this.#open(
      APP_ITEM_TRANSY,
      APPEARANCE_LAZY.WndItemTransy,
      (C) => new C({ request, tooltip: this.#tooltip }),
    );
  }
}
