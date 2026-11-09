// The legacy English strings of `Kia::CWndNewUpgrade` (`src:_Interface/WndNewUpgrade.cpp`; they are literals in the original, not `TID_*` rows, Plan/45 §18.2).

import type { SmeltSetting } from '../../../ui/windows/upgrade/ports';

/** `SetTitle( tempTitle )` of `UpdateReturn`: `"Tries: N\tSuccess: S   Failure: F"`. */
export function triesLine(successes: number, failures: number): string {
  return `Tries: ${String(failures + successes)}\tSuccess: ${String(successes)}   Failure: ${String(failures)}`;
}

/** `PutString` on result 3. */
export const MAXED_OUT = "Failed to upgrade. It's already maxed out";

/** `ResetMats`: the title and the "now" caption of a window without a setting. */
export const RESET_TITLE = 'Safe Upgrade';
export const RESET_CAPTION = 'Upgrade';

/** `OnInitialUpdate`: the caption of the maximum. */
export const MAX_CAPTION = 'Maximum';

/** `SetMode<>()`: the title of the window and the caption of `WIDC_TITLE_NOW_GRADE` of each setting. */
export const MODE_TEXTS: Readonly<Record<SmeltSetting, { title: string; caption: string }>> = {
  none: { title: 'Upgrade', caption: 'Upgrade' },
  normal: { title: 'Normal Upgrade', caption: 'General Upgrading' },
  ultimate: { title: 'Ultimate Upgrade', caption: 'Ultimate Upgrading' },
  jewel: { title: 'Accessory Upgrade', caption: 'Accessory Upgrading' },
  element: { title: 'Element Upgrade', caption: 'Element Upgrade' },
  piercing: { title: 'Piercing', caption: 'Piercing' },
};

/** `SetItem` of a target: `SetTitle( "Upgrade" )`. */
export const TARGET_TITLE = 'Upgrade';
