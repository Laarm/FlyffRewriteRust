// The ids of the digit buttons of the number pad (`ButItemNum0-9.BMP`), shared by `APP_VENDOREX_SELL` and `APP_VENDOREX_BUY`.

import {
  WIDC_NUM0,
  WIDC_NUM1,
  WIDC_NUM2,
  WIDC_NUM3,
  WIDC_NUM4,
  WIDC_NUM5,
  WIDC_NUM6,
  WIDC_NUM7,
  WIDC_NUM8,
  WIDC_NUM9,
} from '../../../gen/defs/resdata';

/** Button id -> the digit character it types (`OnChar( '0' )` ... `OnChar( '9' )`). */
export const PAD_NUMBER_IDS: ReadonlyMap<number, string> = new Map([
  [WIDC_NUM0, '0'],
  [WIDC_NUM1, '1'],
  [WIDC_NUM2, '2'],
  [WIDC_NUM3, '3'],
  [WIDC_NUM4, '4'],
  [WIDC_NUM5, '5'],
  [WIDC_NUM6, '6'],
  [WIDC_NUM7, '7'],
  [WIDC_NUM8, '8'],
  [WIDC_NUM9, '9'],
]);
