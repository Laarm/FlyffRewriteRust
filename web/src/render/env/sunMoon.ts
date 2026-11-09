// The sun angle and the light direction (Plan/41 section 8.3, SkyBox.cpp:584-700). KEEP-QUIRK: `m_fSunAngle` is
// evaluated before `SetLight`; the SAME number is used as RADIANS for the sun, moon and flare quads (REND-28) and as
// DEGREES for the light direction (section 8.2 case 3), so during sun and moon hours the light points almost
// straight down and it swings towards the horizon between 18:00 and 19:59.

export type Vec3 = readonly [number, number, number];

/**
 * `m_fSunAngle` for the game time `hour` (1..24), `min`, `sec` (outdoor worlds only):
 *
 *     t = hour*60 + min - 720 + sec/60;  a = -t^2/2500 + 45                          default (hours 18, 19)
 *     hour >= 20 or 1 <= hour <= 6:  nS = hour <= 6 ? 24 + hour : hour;  a = (nS*3600 + min*60 + sec)*0.00015 - 90
 *     7 <= hour <= 17:               a = (hour*3600 + min*60 + sec)*0.00015 - 90
 */
export function sunAngle(hour: number, min: number, sec: number): number {
  const t = hour * 60 + min - 720 + sec / 60;
  let a = -(t * t) / 2500 + 45;
  if (hour >= 20 || (hour >= 1 && hour <= 6)) {
    const nS = hour <= 6 ? 24 + hour : hour;
    a = (nS * 3600 + min * 60 + sec) * 0.00015 - 90;
  } else if (hour >= 7 && hour <= 17) {
    a = (hour * 3600 + min * 60 + sec) * 0.00015 - 90;
  }
  return a;
}

/**
 * The light direction (the direction the light TRAVELS, D3D semantics) of the outdoor case in LH space:
 * `RotationX((sunAngle + 180) deg) * (0, 0, 1)` in row-vector D3DX form = `(0, -sin t, cos t)`.
 */
export function lightDirLH(sunAngleValue: number): Vec3 {
  const t = ((sunAngleValue + 180) * Math.PI) / 180;
  return [0, -Math.sin(t), Math.cos(t)];
}

/** `SetNight(hour >= 21 || hour <= 6)` (Neuz.cpp FrameMove): the flag that shows the light objects. */
export function isNight(hour: number): boolean {
  return hour >= 21 || hour <= 6;
}
