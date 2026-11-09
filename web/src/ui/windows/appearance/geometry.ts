// The numbers of the face and sex-change windows that others read (the tests, the gallery): kept out of the shells so that reading them does not
// pull the shells' chunks in (`ui/wnd/lazy.ts`).

import type { Point } from '../../layout/geometry';
import { CHANGE_FACE_COUNT } from './ports';

/** The first face column (`point.x -= 10`), the column pitch and the top / height of the picture area of `APP_CHANGEFACE`. */
export const FACE_LEFT = 10;
export const FACE_PITCH = 86;
export const FACE_TOP = 5;
export const FACE_HEIGHT = 170;
/** `point.y < 180` of `OnLButtonUp`. */
const FACE_HIT_HEIGHT = 180;

/** `OnLButtonUp`: the face under a client point, `-1` for none. */
export function faceAt(pt: Point): number {
  // `|| 0`: the truncation of a small negative number is -0
  const col = Math.trunc((pt.x - FACE_LEFT) / FACE_PITCH) || 0;
  return col >= 0 && col < CHANGE_FACE_COUNT && pt.y >= 0 && pt.y < FACE_HIT_HEIGHT ? col : -1;
}

/** `viewport.X -= 6`: the sex-change preview starts 6 px left of the placeholder. */
export const PREVIEW_SHIFT = 6;
