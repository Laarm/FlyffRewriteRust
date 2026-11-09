// The control names, textures and geometry of the pocket window, apart from the shell (the service and the tests read them without loading it).

/** The three tab controls: the basic pocket and the two extra bags (`WIDC_BASIC`, `WIDC_EXBAG1`, `WIDC_EXBAG2`). */
export const POCKET_TAB_NAMES = ['WIDC_BASIC', 'WIDC_EXBAG1', 'WIDC_EXBAG2'] as const;
/** The icons of the extra bags (`nIco`) and the placeholders that the grey picture of an unavailable bag starts at (`nBag`). */
export const BAG_ICON_NAMES = ['WIDC_ICO_BAG1', 'WIDC_ICO_BAG2'] as const;
export const BAG_NOUSE_NAMES = ['WIDC_CUSTOM4', 'WIDC_CUSTOM5'] as const;
/** `InventoryBagEff.tga` (201 x 134) at alpha 150 over an unavailable bag. */
export const NOUSE_TEXTURE = 'InventoryBagEff.tga';
export const NOUSE_SIZE = { w: 201, h: 134 } as const;
export const NOUSE_ALPHA = 150 / 255;
/** `Itm_SysSysScrBagBag01.bmp` (a bag in use) and `...Bag02.bmp` (an empty bag icon), 32 x 32, at alpha 200. */
export const BAG_ICON_USED = 'Itm_SysSysScrBagBag01.bmp';
export const BAG_ICON_EMPTY = 'Itm_SysSysScrBagBag02.bmp';
export const BAG_ICON_SIZE = 32;
export const BAG_ICON_ALPHA = 200 / 255;
/** The layer id of the code-created sprite layer. */
export const POCKET_LAYER_ID = 0x7c01;
