// The bow and wand charge gauge (Plan/43 §7.7; `WndWorld.cpp:85-86, 937-1003, 6657-6672`; PL-121; WP-INPUT-11).
//
//   power(now) = MulDiv(min(now - powerTick, 1800), 4, 1800)       0..4, the level changes at 225 / 675 / 1125 / 1575 ms
//   -1 while `powerTick` lies in the future (the 1800 ms lock after an auto-fired shot)
//
// `MulDiv` is the Windows rounding (round half away from zero), so the levels change at the ROUNDED thresholds, not at the
// truncated 450 / 900 / 1350 / 1800 that ref/21 §8.6 and 17 §11.3 quote (43 §16 Q7, KEEP). The auto-fire is decided in the 60 Hz
// tick (the original did it in `OnEraseBkgnd`, PL-121); the release on L-up is `worldClicks.ts`.

/** `MAX_POWER_GAUGE_MS`: the gauge is full after this long. */
export const MAX_POWER_GAUGE_MS = 1800;
/** `POWER_ATK_DELAY_MS`: the gauge stays locked this long after an auto-fired shot. */
export const POWER_ATK_DELAY_MS = 1800;
/** `MAX_CHARGE_LEVEL`. */
export const MAX_CHARGE_LEVEL = 4;
/** A wand with this little MP (< 5) fires itself at power 3 (the server charges MP at 3 only, KQ-05). */
export const WAND_MP_FLOOR = 5;
export const WAND_STARVED_POWER = 3;

/** Windows `MulDiv(a, b, c)`: `a * b / c` rounded half away from zero (`c > 0`). */
export function mulDiv(a: number, b: number, c: number): number {
  const n = a * b;
  const half = Math.trunc(c / 2);
  return n >= 0 ? Math.trunc((n + half) / c) : -Math.trunc((-n + half) / c);
}

/** What the gauge draws (doc 41 `RenderGauge(0..4)`), `null` for no gauge. */
export type GaugeDisplay = number | null;

export class ChargeGauge {
  /** `m_dwPowerTick`: the moment the charge started (ms); in the future after an auto-fired shot. */
  powerTick = 0;

  /** `power(now)`: -1 during the lock, else 0..4. */
  power(now: number): number {
    const n = now - this.powerTick;
    if (n < 0) return -1;
    return mulDiv(Math.min(n, MAX_POWER_GAUGE_MS), MAX_CHARGE_LEVEL, MAX_POWER_GAUGE_MS);
  }

  /** The charge starts now (an L-down on a mover, a released shot). */
  restart(now: number): void {
    this.powerTick = now;
  }

  /** An auto-fired shot locks the gauge for {@link POWER_ATK_DELAY_MS}. */
  lock(now: number): void {
    this.powerTick = now + POWER_ATK_DELAY_MS;
  }

  /** `RenderGauge` value: nothing without a living focus, 4 during the lock, else the level while charging, else 0. */
  display(now: number, hasTarget: boolean, lDown: boolean, selectTarget: boolean): GaugeDisplay {
    if (!hasTarget) return null;
    const p = this.power(now);
    if (p === -1) return MAX_CHARGE_LEVEL;
    return !lDown || !selectTarget ? 0 : p;
  }
}

/** Which shot the tick decides to fire, if any (`shoot` of §7.7). */
export type AutoShot = 'bow' | 'wand' | null;

/**
 * The auto-fire rule of §7.7 for a held click on a focused mover: a bow fires at power >= 4; a wand at power >= 4, or at
 * power >= 3 when its MP is below 5.
 */
export function autoShot(
  kind: 'bow' | 'wand' | 'other',
  power: number,
  mp: number,
  lDown: boolean,
  selectTarget: boolean,
): AutoShot {
  if (!lDown || !selectTarget) return null;
  if (kind === 'bow') return power >= MAX_CHARGE_LEVEL ? 'bow' : null;
  if (kind === 'wand') {
    const need = mp >= WAND_MP_FLOOR ? MAX_CHARGE_LEVEL : WAND_STARVED_POWER;
    return power >= need ? 'wand' : null;
  }
  return null;
}
