// The control names, the texture and the geometry constants of the bank window, apart from the shell so that the service and the tests can read them
// without loading it (`ui/wnd/lazy.ts`).

/** The tab controls of the three compartments (`WIDC_BANK_0`, `_1` and `_3`: the layout has no `_2`). */
export const BANK_TAB_NAMES = ['WIDC_BANK_0', 'WIDC_BANK_1', 'WIDC_BANK_3'] as const;
export const BANK_GOLD_NAMES = ['WIDC_BANKGOLD_0', 'WIDC_BANKGOLD_1', 'WIDC_BANKGOLD_2'] as const;
export const BANK_COIN_NAMES = ['WIDC_BANKGO_0', 'WIDC_BANKGO_1', 'WIDC_BANKGO_2'] as const;
/** The placeholders that the grey picture of an unusable compartment covers. */
export const BANK_NOUSE_NAMES = ['WIDC_CUSTOM4', 'WIDC_CUSTOM5', 'WIDC_CUSTOM6'] as const;
/** `WndCommonBankNotUse.tga` (144 x 214) at alpha 50. */
export const NOUSE_TEXTURE = 'WndCommonBankNotUse.tga';
export const NOUSE_ALPHA = 50 / 255;
/** `ptInventory - rectInventory.Width() * 2.525`: how far left of the inventory the bank sits when the inventory is on the right half. */
export const LEFT_OF_INVENTORY = 2.525;
/** The default PIN of an account that never set one. */
export const DEFAULT_PIN = '0000';
