// The generic hit effects of a melee attack (Plan/41 section 17.3, ActionMoverDmg.cpp:91-100): at the attacker's hit frame the
// weapon's `dwSfxObj3` appears at the victim - one unit in front of him along the attacker's facing and one unit up
// (`AngleToVectorXZ(attackerAngle, 1.0)`: `(sin a, 0, -cos a)`), a yo-yo at the victim + 1 up - and `dwSfxObj5` at the attacker's
// feet. The point sprites of the melee sparks are `render/sfx/ParticleMng` (WP-REND-21).

export type V3 = readonly [number, number, number];

/** One effect to create. */
export interface HitSpark {
  /** `XI_*` effect id. */
  readonly xi: number;
  readonly pos: V3;
}

/** `v > 0` and not the `NULL_ID` encodings. */
export function validEffect(v: number | undefined): v is number {
  return v !== undefined && v > 0 && v < 0x7fff_ffff;
}

/**
 * The effects of a melee hit: `sfxObj3` on the victim, `sfxObj5` on the attacker. `yoyo` is the attacker's active hand item kind
 * (`IK3_YOYO`).
 */
export function hitSparks(
  sfxObj3: number | undefined,
  sfxObj5: number | undefined,
  victim: V3,
  attacker: V3,
  attackerAngleDeg: number,
  yoyo: boolean,
): HitSpark[] {
  const out: HitSpark[] = [];
  if (validEffect(sfxObj3)) {
    const a = (attackerAngleDeg * Math.PI) / 180;
    const pos: V3 = yoyo
      ? [victim[0], victim[1] + 1, victim[2]]
      : [victim[0] + Math.sin(a), victim[1] + 1, victim[2] - Math.cos(a)];
    out.push({ xi: sfxObj3, pos });
  }
  if (validEffect(sfxObj5)) out.push({ xi: sfxObj5, pos: attacker });
  return out;
}
