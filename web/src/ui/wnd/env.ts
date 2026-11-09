// The environment every window of one UI instance shares (the globals of the original: `m_pWndRoot`, `m_resMng`,
// `m_textureMng`, `m_Theme`; Plan/42 §4). It is created by the `WindowManager` for the root window and inherited
// by every child through `create()`, so there are no module-level singletons.

import type { Logger } from '../../platform/log';
import type { ControlFactory } from '../controls/factory';
import type { LayoutRegistry } from '../layout/registry';
import type { SkinBaker } from '../skin/baker';
import type { UiImageCache } from '../skin/images';
import type { UiHost } from '../host';
import type { WindowManager } from '../wm/WindowManager';

export interface WndEnv {
  readonly wm: WindowManager;
  readonly registry: LayoutRegistry;
  readonly images: UiImageCache;
  readonly baker: SkinBaker;
  /** Creates the control object of a layout row (`controls/factory.ts`). */
  readonly controls: ControlFactory;
  readonly host: UiHost;
  readonly log: Logger;
}
