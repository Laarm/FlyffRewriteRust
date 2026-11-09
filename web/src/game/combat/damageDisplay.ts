// The `Damage` presentation decision tree (Plan/17 section 11.2, ref/09a section 6.5, ref/21 section 6.9; WP-COMBAT-10).
// A pure function from the message and what the client knows about the two movers to the list of effects: nothing here
// touches the renderer, the sim or a store. `handlers.ts` runs the result through `RenderFx`.
//
// The order is the one of `CDPClient::OnDamage` (DPClient.cpp:1804-1960):
//   1. the display gate: an unknown attacker always shows; the option "show all damage" shows victims within 30 m; a game
//      master also sees hits on the object it has in focus; and the final rule, victim or attacker is the own player
//   2. a shown hit cancels the own item cast when the own player is involved (`SendStateModeCancel`)
//   3. BLOCKING adds the block effect and goes on; PARRY ends with its effect (no number); else RESIST shows the number or,
//      for 0, the miss effect; else MISS shows the miss effect; else CRITICAL adds the critical effect and the number
//   4. the number style is 0 for damage taken by the own player, 1 for damage dealt

import {
  XI_HIT_CRITICAL01,
  XI_HIT_HITBLOCK01,
  XI_HIT_MISS01,
  XI_HIT_PARRY01,
} from '../../gen/defs/obj';
import { AF } from './constants';

/** `AUTH_GAMEMASTER` ('L'): the viewer authority from which a game master sees the hits on its focus. */
export const AUTH_GAMEMASTER = 76;
/** The "show all damage" radius around the own player (`IsRangeObj(pos, 30)`). */
export const SHOW_ALL_RADIUS = 30;
/** The effects of block, parry, miss and critical sit this far above the victim's feet. */
export const EFFECT_LIFT = 1.5;

export interface DamageInput {
  /** `AF_*` of the message. */
  readonly atkFlags: number;
  readonly damage: number;
  readonly victimIsSelf: boolean;
  /** The attacker object is known to the client (an unknown one makes every hit visible). */
  readonly attackerKnown: boolean;
  readonly attackerIsSelf: boolean;
  /** The option "show all damage" (`g_Option.m_bDamageRender`). */
  readonly showAll: boolean;
  /** The victim stands within `SHOW_ALL_RADIUS` of the own player. */
  readonly victimNear: boolean;
  /** The viewer's authority letter (`AUTH_*`). */
  readonly authority: number;
  /** The victim is the object the viewer has in focus. */
  readonly victimIsFocus: boolean;
}

/** One effect to create (`RenderFx.sfx`): the id, its scale and the lift above the victim. */
export interface DamageEffect {
  readonly xi: number;
  readonly scale: number;
  readonly lift: number;
}

export interface DamageDisplay {
  /** The display gate passed. */
  readonly show: boolean;
  /** The own item cast ends (`StateModeReq` cancel). */
  readonly cancelItemCast: boolean;
  readonly effects: readonly DamageEffect[];
  /** The damage number to float over the victim, `null` for none. */
  readonly number: { readonly value: number; readonly style: 0 | 1 } | null;
}

const NOTHING: DamageDisplay = { show: false, cancelItemCast: false, effects: [], number: null };

/** Whether the hit is shown at all (step 1). */
export function displayGate(d: DamageInput): boolean {
  if (!d.attackerKnown) return true;
  if (d.victimIsSelf || d.attackerIsSelf) return true;
  if (d.showAll) return d.victimNear;
  // a game master sees the hits on its focus (the original only checks this branch when "show all" is off)
  return d.authority >= AUTH_GAMEMASTER && d.victimIsFocus;
}

/** The decision tree of one `Damage` message. */
export function decideDamageDisplay(d: DamageInput): DamageDisplay {
  if (!displayGate(d)) return NOTHING;
  const effects: DamageEffect[] = [];
  const style: 0 | 1 = d.victimIsSelf ? 0 : 1;
  const involved = d.victimIsSelf || d.attackerIsSelf;
  let number: DamageDisplay['number'] = null;
  if ((d.atkFlags & AF.BLOCKING) !== 0) {
    effects.push({ xi: XI_HIT_HITBLOCK01, scale: 2, lift: EFFECT_LIFT });
  }
  if ((d.atkFlags & AF.PARRY) !== 0) {
    effects.push({ xi: XI_HIT_PARRY01, scale: 2, lift: EFFECT_LIFT });
  } else if ((d.atkFlags & AF.RESIST) !== 0) {
    if (d.damage === 0) effects.push({ xi: XI_HIT_MISS01, scale: 2, lift: EFFECT_LIFT });
    else number = { value: d.damage, style };
  } else if ((d.atkFlags & AF.MISS) !== 0) {
    effects.push({ xi: XI_HIT_MISS01, scale: 2, lift: EFFECT_LIFT });
  } else {
    if ((d.atkFlags & AF.CRITICAL) !== 0) {
      effects.push({ xi: XI_HIT_CRITICAL01, scale: 3, lift: EFFECT_LIFT });
    }
    number = { value: d.damage, style };
  }
  return { show: true, cancelItemCast: involved, effects, number };
}
