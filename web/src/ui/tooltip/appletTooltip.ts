// The applet tooltip format (`CWndTaskBar` / start menu, WndTaskBar.cpp:2810-2857; Plan/42 §9.8): the description, and
// below it `[Hotkey X]` when the applet has a letter hotkey.

import { TID_GAME_TOOLTIP_HOTKEY } from '../../gen/defs/text';

/** `desc` or `desc + "\n[" + getText(TID_GAME_TOOLTIP_HOTKEY) + " " + key + "]"`. */
export function appletTooltipText(
  description: string,
  hotkey: string | null | undefined,
  getText: (tid: number) => string,
): string {
  if (hotkey === null || hotkey === undefined || hotkey === '') return description;
  return `${description}\n[${getText(TID_GAME_TOOLTIP_HOTKEY)} ${hotkey}]`;
}
