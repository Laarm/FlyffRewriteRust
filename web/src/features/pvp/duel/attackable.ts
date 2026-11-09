// The player branch of the attack gate (Plan/43 §7.6, 16 §18.4 `client_attackable`; `CMover::IsAttackAble`, `src:_Common/Mover.cpp:6577-6734`;
// WP-CFEAT-36): the views of the own character and of a clicked player that `rules.clientAttackable` decides on. A duel partner (and a
// member of the duelled party) becomes attackable once the fight began (`duelState == 1` on either side); a player of the same declared guild war
// in the other guild (`IsWarTarget`, event `guild_war` on) and a combatant of the other guild in the siege (`IsGuildCombatTarget`, world
// `WI_WORLD_GUILDWAR`, event `guild_combat` on) are attackable too (M6: the war and the siege are decided by blows); every other player stays
// unattackable while `events.pk` is off, which is the shipped configuration.

import { RA_FIGHT, RA_PENALTY_PK, RA_PK, RA_SAFETY } from '../../../gen/defs/define';
import { WI_WORLD_ARENA, WI_WORLD_GUILDWAR } from '../../../gen/defs/world';
import type { AttackContext, AttackView } from '../../../rules';
import type { DuelBook } from './book';

/** `EVE_GUILDWAR` and `EVE_GUILDCOMBAT`: the ids of the event flags (`events.guild_war`, `events.guild_combat`). */
export const EVE_GUILDWAR = 3;
export const EVE_GUILDCOMBAT = 15;
/** `EVE_GUILDCOMBAT1TO1`: the event flag of the 1:1 guild combat (ui-minigames). */
export const EVE_GUILDCOMBAT1TO1 = 22;

/** `GetPKPVPRegionAttr()`: `RA_FIGHT` before `RA_PENALTY_PK` before `RA_PK`, else `RA_SAFETY` (Plan/17 §8.2). */
export function regionClassOf(attribute: number): number {
  if ((attribute & RA_FIGHT) === RA_FIGHT) return RA_FIGHT;
  if ((attribute & RA_PENALTY_PK) === RA_PENALTY_PK) return RA_PENALTY_PK;
  if ((attribute & RA_PK) === RA_PK) return RA_PK;
  return RA_SAFETY;
}

/** What the views are built from (the stores and the entity table, a fake in the tests). */
export interface AttackFacts {
  readonly self: {
    readonly objId: number;
    readonly flying: boolean;
    /** `pk_propensity > 0`. */
    readonly chaotic: boolean;
    readonly partyId: number;
    readonly guildId: number;
    readonly level: number;
    /** The region attribute word under the player (`worldStore.region.attribute`). */
    readonly regionAttribute: number;
    readonly x: number;
    readonly z: number;
    readonly worldId: number;
    /** `m_idWar` (the declared guild war the own character stands in, 0 or missing = none). */
    readonly warId?: number;
    /** `m_nGuildCombatState` (1 = a combatant of the siege). */
    readonly guildCombatState?: number;
  };
  readonly target: {
    readonly objId: number;
    readonly chaotic: boolean;
    /** `m_idparty` of the target. */
    readonly partyId: number;
    readonly guildId: number;
    readonly level: number;
    /** The region attribute word under the target, `null` when it is not known. */
    readonly regionAttribute: number | null;
    readonly x: number;
    readonly z: number;
    /** A personal shop is open. */
    readonly vendorOpen: boolean;
    /** `m_idWar` of the target (0 or missing = none). */
    readonly warId?: number;
    /** `m_nGuildCombatState` of the target. */
    readonly guildCombatState?: number;
  };
  /**
   * `g_eLocal.GetState(EVE_GUILDWAR)`, `(EVE_GUILDCOMBAT)` and `(EVE_GUILDCOMBAT1TO1)`: `IsWarTarget`, the siege combatants and the 1:1 combatants need the events on.
   * `guildCombat1to1` is ui-minigames' (WP-CFEAT-45, `Mover.cpp:6647-6652`).
   */
  readonly events?: {
    readonly guildWar: boolean;
    readonly guildCombat: boolean;
    readonly guildCombat1to1?: boolean;
  };
  /** `g_GuildCombat1to1Mng.IsPossibleMover( target )`: the target takes part in the 1:1 combat (`inputProvidersOf( ctx ).gc1to1.possible( target )`). */
  readonly gc1to1Target?: boolean;
}

export interface PlayerAttackViews {
  readonly self: AttackView;
  readonly target: AttackView;
  readonly ctx: AttackContext;
}

/**
 * The three arguments of `rules.clientAttackable` for the own character and a clicked player. `selfParty` is the own party (the
 * opposing members of a party duel carry it in the book); a member of the opposing party has `m_idparty` = the party the own player
 * duels (`OnDuelPartyStart`).
 */
export function playerAttackViews(book: DuelBook, f: AttackFacts): PlayerAttackViews {
  const own = book.get(f.self.objId);
  const other = book.get(f.target.objId);
  // a known opposing member of a party duel belongs to the party the own player duels
  const targetParty =
    own.duel === 2 && other.duel === 2 && other.party === f.self.partyId
      ? own.party
      : f.target.partyId;
  return {
    self: {
      kind: 'player',
      id: f.self.objId,
      flying: f.self.flying,
      chaotic: f.self.chaotic,
      duelState: own.state,
      duelOther: own.other,
      duelParty: own.party,
      partyId: f.self.partyId,
      guildId: f.self.guildId,
      ...(f.self.warId !== undefined && { warId: f.self.warId }),
      ...(f.self.guildCombatState !== undefined && { guildCombatState: f.self.guildCombatState }),
      level: f.self.level,
      regionAttr: regionClassOf(f.self.regionAttribute),
      x: f.self.x,
      z: f.self.z,
    },
    target: {
      kind: 'player',
      id: f.target.objId,
      chaotic: f.target.chaotic,
      duelState: other.state,
      duelOther: other.other,
      duelParty: other.party,
      partyId: targetParty,
      guildId: f.target.guildId,
      ...(f.target.warId !== undefined && { warId: f.target.warId }),
      ...(f.target.guildCombatState !== undefined && {
        guildCombatState: f.target.guildCombatState,
      }),
      level: f.target.level,
      regionAttr: regionClassOf(f.target.regionAttribute ?? f.self.regionAttribute),
      x: f.target.x,
      z: f.target.z,
      vendorOpen: f.target.vendorOpen,
    },
    ctx: {
      worldIsGuildWar: f.self.worldId === WI_WORLD_GUILDWAR,
      worldIsArena: f.self.worldId === WI_WORLD_ARENA,
      ...(f.events && { guildWar: f.events.guildWar, guildCombat: f.events.guildCombat }),
      ...(f.events?.guildCombat1to1 !== undefined && { guildCombat1to1: f.events.guildCombat1to1 }),
      ...(f.gc1to1Target !== undefined && { gc1to1Target: f.gc1to1Target }),
    },
  };
}
