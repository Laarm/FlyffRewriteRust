// Style bits, window types and notification codes of the window framework (Plan/42 §3.5, §4.4; ref/22 §7.4).
// The WBS_/WSS_/WTYPE_ values of `WndStyle.h` come from the generated defs (`gen/defs/wndstyle`); this file adds
// what the original keeps outside that header: the frame low bits (`WndNeuz.h:24-30`), the edit bits that
// `WndStyle.h` comments out (`WndControl.h`), the title-bar button ids, the `Wnm` notification codes and the
// `AppDefine.h` aliases.

import {
  WBS_CHILD,
  WBS_DOCKING,
  WBS_NODRAWFRAME,
  WBS_NOFRAME,
  WTYPE_BUTTON,
  WTYPE_COMBOBOX,
  WTYPE_CUSTOM,
  WTYPE_EDITCTRL,
  WTYPE_LISTBOX,
  WTYPE_LISTCTRL,
  WTYPE_MENU,
  WTYPE_SCROLLBAR,
  WTYPE_STATIC,
  WTYPE_TABCTRL,
  WTYPE_TEXT,
  WTYPE_TREECTRL,
} from '../../gen/defs/wndstyle';

export * from '../../gen/defs/wndstyle';

/** `WBS_*` low bits of a FRAME (`CWndNeuz`, `WndNeuz.h:24-30`): the same bits mean button flags for controls. */
export const WBS_MINIMIZEBOX = 0x1;
export const WBS_MAXIMIZEBOX = 0x2;
export const WBS_HELP = 0x4;
export const WBS_VIEW = 0x8;
export const WBS_PIN = 0x10;
export const WBS_EXTENSION = 0x20;
export const WBS_THICKFRAME = 0x40;

/**
 * Edit control bits (`EBS_*`, `WndControl.h`; `WndStyle.h` has them commented out), hand-written (Plan/42 §3.5).
 * `EBS_LEFT` is 0 (the absence of the other alignment bits).
 */
export const EBS_LEFT = 0x0;
export const EBS_CENTER = 0x1;
export const EBS_RIGHT = 0x2;
export const EBS_MULTILINE = 0x4;
export const EBS_UPPERCASE = 0x8;
export const EBS_LOWERCASE = 0x10;
export const EBS_PASSWORD = 0x20;
export const EBS_AUTOVSCROLL = 0x40;
export const EBS_AUTOHSCROLL = 0x80;
export const EBS_NOHIDESEL = 0x100;
export const EBS_OEMCONVERT = 0x400;
export const EBS_READONLY = 0x800;
export const EBS_WANTRETURN = 0x1000;
export const EBS_NUMBER = 0x2000;

/** Combo box styles (`WCBS_*`, ref/22 §7.4). */
export const WCBS_SIMPLE = 0x1;
export const WCBS_DROPDOWN = 0x2;
export const WCBS_DROPDOWNLIST = 0x3;

/** Style of the frames the framework creates itself. */
export const STYLE_TITLE_BAR = WBS_CHILD | WBS_DOCKING | WBS_NODRAWFRAME | WBS_NOFRAME;

/** Title-bar button ids (`WTBID_*`, Plan/42 §4.4). */
export const WTBID_CLOSE = 10000;
export const WTBID_MAX = 10001;
export const WTBID_MIN = 10002;
export const WTBID_HELP = 10003;
export const WTBID_REPORT = 10004;
export const WTBID_EXTENSION = 10005;
export const WTBID_PIN = 10006;
/** The id of the title bar child itself (`m_wndTitleBar.Create(..., 100000)`). */
export const WID_TITLE_BAR = 100000;

/** Notification codes of `OnChildNotify` (`WNM_*`, `WIN_*`, `EN_*`; Plan/42 §4.4). */
export const Wnm = {
  SELCHANGE: 1,
  DBLCLK: 2,
  SELCANCEL: 3,
  SETFOCUS: 4,
  KILLFOCUS: 5,
  CLICKED: 6,
  WIN_DBLCLK: 40,
  WIN_ITEMDROP: 41,
  EN_CHANGE: 0x300,
  EN_RETURN: 0x603,
  EN_UP: 0x604,
} as const;
export type Wnm = (typeof Wnm)[keyof typeof Wnm];

/** Win32 message ids passed as the `msg` argument of `onCommand` (Plan/42 §4.4). */
export const WM_LBUTTONUP = 0x0202;
export const WM_LBUTTONDBLCLK = 0x0203;
export const WM_RBUTTONUP = 0x0205;

/** The control kinds of a layout row (`WTYPE_*`); `WTYPE_BASE` (0) never appears in a layout. */
export type WType = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12;
/** Alias of {@link WType}: the kind of a control (layout rows and code-created controls). */
export type ControlKind = WType;

/** Human-readable name of a `WTYPE_*` value (dev tools, debug output). */
export function wtypeName(t: number): string {
  switch (t) {
    case WTYPE_BUTTON:
      return 'button';
    case WTYPE_LISTBOX:
      return 'listbox';
    case WTYPE_LISTCTRL:
      return 'listctrl';
    case WTYPE_STATIC:
      return 'static';
    case WTYPE_COMBOBOX:
      return 'combobox';
    case WTYPE_SCROLLBAR:
      return 'scrollbar';
    case WTYPE_EDITCTRL:
      return 'edit';
    case WTYPE_TREECTRL:
      return 'tree';
    case WTYPE_TABCTRL:
      return 'tabctrl';
    case WTYPE_CUSTOM:
      return 'custom';
    case WTYPE_MENU:
      return 'menu';
    case WTYPE_TEXT:
      return 'text';
    default:
      return 'base';
  }
}

/** `true` when `style` has any of the bits in `bits` (`CWndBase::IsWndStyle`: `(style & bits) != 0`). */
export function hasStyle(style: number, bits: number): boolean {
  return (style & bits) !== 0;
}

/** The three `AppDefine.h` aliases that `gen/defs` does not contain (Plan/42 §3.2). */
export const APP_ALIASES: Readonly<Record<string, number>> = {
  APP_SHOP_: 280, // APP_SHOPEX
  APP_CONFIRM_BUY_: 306, // APP_CONFIRM_BUYEX
  APP_MESSENGER_: 302, // APP_MESSENGER_EX
};

/** Ids that code references but that are absent from `resdata.inc` (dead features, ref/22 §7.5): never created. */
export const APP_IDS_WITHOUT_LAYOUT: readonly number[] = [238, 883, 887, 892];
