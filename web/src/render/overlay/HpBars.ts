// The party HP bar (Plan/41 §19.4; MoverRender.cpp:1873-1965): a background and a fill cropped to the HP fraction, both
// scaled by (0.8, 0.55), alpha 200, 80 px wide on screen.

import { even } from './anchor';
import type { AnchorPx } from './Nameplates';
import { AUTH_GAMEMASTER, type DrawItem, type OverlaySubject, type OverlayViewer } from './types';

export const HP_BG = 'theme/gaupartyhpbg.tga';
export const HP_FILL = 'theme/gaupartyhp.tga';
export const HP_FILL_DEBUFF = 'theme/gaupartyhp_dbf.tga';
export const HP_SCALE_X = 0.8;
export const HP_SCALE_Y = 0.55;
export const HP_ALPHA = 200;

/** The width of the gauge on the screen (`nGaugeWidth`). */
export const HP_GAUGE_WIDTH = 80;

export interface HpBarOptions {
  nameSelf: boolean;
  nameOthers: boolean;
}

/**
 * Whether a bar is drawn: the own player and the members of the own party, in front of the camera (`inFront`: `dot(camDir,
 * dir to member) >= 0`), at telescope 0 only, not transparent, not a text-disguised GM, not render-skipped, with the name
 * option of its category on and `viewHp`.
 */
export function hpBarShown(
  s: OverlaySubject,
  viewer: OverlayViewer,
  o: HpBarOptions,
  inFront: boolean,
): boolean {
  if (s.culled || s.transparent || !viewer.viewHp || viewer.telescope !== 0) return false;
  if (!s.isSelf && !s.partyMember) return false;
  if (s.kind !== 'player') return false;
  if (!inFront) return false;
  if (s.isSelf ? !o.nameSelf : !o.nameOthers) return false;
  if (s.authorization >= AUTH_GAMEMASTER && s.textDisguise) return false;
  if (!s.isSelf && s.renderSkip) return false;
  return s.maxHp > 0;
}

/**
 * The two items of the bar: the background at `p = (even(anchor.x - 40 - 2), anchor.y - 34 - 3 - 2)` and the fill
 * cropped to `texW * hp / maxHp` source pixels. `fillTexW` is the width of the fill texture (it is known once the
 * texture loaded; the crop is applied by the renderer from `cropW = round(fillTexW * hp / maxHp)`).
 */
export function layoutHpBar(s: OverlaySubject, anchor: AnchorPx, fillTexW: number): DrawItem[] {
  const x = even(Math.trunc(anchor.x - HP_GAUGE_WIDTH / 2) - 2);
  const y = Math.trunc(anchor.y - 34 - 3) - 2;
  const hp = Math.max(0, Math.min(s.hp, s.maxHp));
  const cropW = Math.trunc((fillTexW * hp) / s.maxHp);
  return [
    { kind: 'image', path: HP_BG, x, y, alpha: HP_ALPHA, scaleX: HP_SCALE_X, scaleY: HP_SCALE_Y },
    {
      kind: 'image',
      path: s.debuffed ? HP_FILL_DEBUFF : HP_FILL,
      x,
      y,
      alpha: HP_ALPHA,
      scaleX: HP_SCALE_X,
      scaleY: HP_SCALE_Y,
      cropW,
    },
  ];
}
