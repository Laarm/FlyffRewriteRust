//! The client's attackability rule (Plan/16 §18.4 `client_attackable`, Plan/43 §7.6, Plan/17 §8.1;
//! WP-MOVER-23): the client branch of `CMover::IsAttackAble(pObj)` (`src:_Common/Mover.cpp:6577-6734`) with
//! `IsAttackAbleNPC`, `IsPKAttackAble`, `IsPKInspection`, `IsPKPVPInspectionBase`, `IsPVPTarget`,
//! `IsWarTarget`, `IsArenaTarget` and `IsGuildCombatTarget`.
//!
//! The function is pure: the browser fills the three views from its stores (the own character, the clicked
//! mover, the event flags and the region) and calls it through `RulesContext.client_attackable`; the server
//! tests call the same function natively. The server's own decision is the hit type of doc 17
//! (`GetHitType`), which this rule mirrors for the cases the client can see.
//!
//! The legacy function also cancels a running base-motion item cast (`SendStateModeCancel`), clears the
//! auto-attack flag on a refusal and drops the focus of a dead NPC; those are side effects of the caller (the
//! targeting state machine, Plan/43 §7.6), not part of the answer. The held keys of the original
//! (`GetAsyncKeyState(VK_CONTROL)` for a forced attack, `VK_MENU` for the "alt" override) come in as flags.

use crate::consts::NULL_ID;
use crate::consts::defs::define::{RA_PENALTY_PK, RA_SAFETY};

/// The answer for a target that cannot be attacked.
pub const ATTACKABLE_NO: u32 = 0;
/// The answer for an attackable target.
pub const ATTACKABLE_YES: u32 = 1;
/// The answer for an attackable player the PK rules allow (`IsPKAttackAble`): the caller remembers it as the
/// last target (`CWndWorld::SetLastTarget`).
pub const ATTACKABLE_PK_TARGET: u32 = 2;

/// What the rule needs to know about a mover that is a player (the own character or the clicked player).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerView {
    /// The object id (`GetId()`).
    pub id: u32,
    /// `m_pActMover->IsFly()`.
    pub flying: bool,
    /// `IsChaotic()`: `pk_propensity > 0`.
    pub chaotic: bool,
    /// `m_nDuelState`.
    pub duel_state: i32,
    /// `m_idDuelOther` ([`NULL_ID`] = none).
    pub duel_other: u32,
    /// `m_idDuelParty` (0 = none).
    pub duel_party: i32,
    /// `m_idparty` (0 = none).
    pub party_id: i32,
    /// `m_idGuild` (0 = none).
    pub guild_id: u32,
    /// `m_idWar` (0 = none).
    pub war_id: i32,
    /// `m_nGuildCombatState` (1 = a combatant).
    pub guild_combat_state: i32,
    /// `GetLevel()`.
    pub level: i32,
    /// `GetPKPVPRegionAttr()`: the `RA_*` class of the region the player stands in.
    pub region_attr: u32,
    /// The position (`GetPos().x`); only the arena uses it.
    pub x: f32,
    /// The position (`GetPos().z`).
    pub z: f32,
}

impl Default for PlayerView {
    fn default() -> Self {
        PlayerView {
            id: NULL_ID,
            flying: false,
            chaotic: false,
            duel_state: 0,
            duel_other: NULL_ID,
            duel_party: 0,
            party_id: 0,
            guild_id: 0,
            war_id: 0,
            guild_combat_state: 0,
            level: 1,
            region_attr: 0,
            x: 0.0,
            z: 0.0,
        }
    }
}

/// The own character: its [`PlayerView`] plus what only it knows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelfView {
    /// The player facts.
    pub player: PlayerView,
    /// `HasBuffByIk3(IK3_TEXT_DISGUISE)`: a text disguise forbids every attack.
    pub text_disguise: bool,
    /// `IsStateMode(STATE_BASEMOTION_MODE)`: an item cast is running (the caller cancels it).
    pub base_motion: bool,
    /// `IsForceAttack()`: the attack key (Ctrl) is held.
    pub force_attack: bool,
    /// `GetAsyncKeyState(VK_MENU)`: the alt key is held (the PK inspection answers 9 = refused).
    pub alt_key: bool,
}

/// What the rule needs to know about a target mover that is not a player (an NPC or monster).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NpcView {
    /// `IsDie()`.
    pub dead: bool,
    /// `MoverProp.bKillable`.
    pub killable: bool,
    /// `IsPeaceful()`.
    pub peaceful: bool,
    /// `MoverProp.dwClass == RANK_GUARD`.
    pub guard: bool,
    /// `MoverProp.nChaotic` (guards only).
    pub chaotic: i32,
    /// `MoverProp.dwFlying == 1`.
    pub flying: bool,
}

impl Default for NpcView {
    fn default() -> Self {
        NpcView {
            dead: false,
            killable: true,
            peaceful: false,
            guard: false,
            chaotic: 0,
            flying: false,
        }
    }
}

/// The clicked object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetView {
    /// Not a mover (a ground item, a ctrl): always attackable (the click picks it up or uses it).
    Other,
    /// A player.
    Player {
        /// The player facts.
        view: PlayerView,
        /// `m_vtInfo.IsVendorOpen()`: the player keeps a vendor stall open.
        vendor_open: bool,
        /// `g_Neuz.m_NeuzEnemy.IsPKing(id)`: the player attacked the own character recently.
        pking: bool,
    },
    /// An NPC or a monster.
    Npc(NpcView),
}

/// The world state the rule reads (the event flags `g_eLocal` and the world kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackCtx {
    /// `g_eLocal.GetState(EVE_18)`: duels, wars, arena and PK are open (forced on outside Korea).
    pub eve_18: bool,
    /// `EVE_PK`: the player-kill system is on (`events.pk`, off in this repack).
    pub events_pk: bool,
    /// `EVE_GUILDWAR == 1`: a guild war is running (`IsWarTarget`).
    pub guild_war: bool,
    /// `EVE_SCHOOL_BATTLE == SBS_START2`: the school battle event (a dead event, `IsSchoolTarget`).
    pub school_battle: bool,
    /// `EVE_GUILDCOMBAT`: the guild siege is running.
    pub guild_combat: bool,
    /// `EVE_GUILDCOMBAT1TO1`: the 1:1 guild combat is running.
    pub guild_combat_1to1: bool,
    /// `pWorld->GetID() == WI_WORLD_GUILDWAR`.
    pub world_is_guild_war: bool,
    /// `pWorld->IsArena()`.
    pub world_is_arena: bool,
    /// `g_GuildCombat1to1Mng.IsPossibleMover(target)`: the target takes part in the 1:1 combat.
    pub gc1to1_target: bool,
    /// `prj.m_PKSetting.nLimitLevel`: players up to this level cannot be attacked (20).
    pub pk_limit_level: i32,
}

impl Default for AttackCtx {
    fn default() -> Self {
        AttackCtx {
            eve_18: true,
            events_pk: false,
            guild_war: false,
            school_battle: false,
            guild_combat: false,
            guild_combat_1to1: false,
            world_is_guild_war: false,
            world_is_arena: false,
            gc1to1_target: false,
            pk_limit_level: 20,
        }
    }
}

/// `CMover::IsPVPTarget(other)`: a solo duel partner or a member of the duel party, with the duel in the fighting
/// state (`1`) on either side.
fn is_pvp_target(a: &PlayerView, o: &PlayerView) -> bool {
    let mut hit = false;
    if a.duel_other != NULL_ID && a.duel_other == o.id {
        hit = true;
    }
    if 0 < a.duel_party && a.duel_party == o.party_id {
        hit = true;
    }
    hit && (a.duel_state == 1 || o.duel_state == 1)
}

/// `CMover::IsWarTarget(other)`: both in the same guild war, different guilds (only while the war event runs).
fn is_war_target(c: &AttackCtx, a: &PlayerView, o: &PlayerView) -> bool {
    c.guild_war && a.war_id > 0 && a.war_id == o.war_id && a.guild_id != o.guild_id
}

/// `CMover::IsArenaTarget(other)`: in an arena world, both within `sqrt(1246)` of the pit centre (490, 507),
/// compared on the truncated integer offsets like the original.
fn is_arena_target(c: &AttackCtx, a: &PlayerView, o: &PlayerView) -> bool {
    if !c.world_is_arena {
        return false;
    }
    let inside = |p: &PlayerView| {
        let dx = (p.x - 490.0) as i32;
        let dy = (p.z - 507.0) as i32;
        dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy)) <= 1246
    };
    inside(o) && inside(a)
}

/// `CMover::IsGuildCombatTarget(other)` (client branch): both are combatants of different guilds.
fn is_guild_combat_target(a: &PlayerView, o: &PlayerView) -> bool {
    a.guild_combat_state == 1 && o.guild_combat_state == 1 && a.guild_id != o.guild_id
}

/// `CMover::IsPKPVPInspectionBase(region, FALSE)` of `m` for the region class `region` of the attacker: 0 when
/// `m` may take part in PK, else the reason (1 safe zone or flying or at war, 2 other region, 5 level limit, 9
/// alt key).
fn pk_inspection_base(region: u32, m: &PlayerView, c: &AttackCtx, alt: bool) -> u32 {
    if region == RA_SAFETY {
        return 1;
    }
    if region != m.region_attr {
        return 2;
    }
    if alt {
        return 9;
    }
    if m.level <= c.pk_limit_level {
        return 5;
    }
    if m.flying || m.war_id > 0 {
        return 1;
    }
    0
}

/// `CMover::IsPKInspection(other)`: may `me` attack the player `other` under the PK rules.
fn pk_inspection(
    me: &SelfView,
    other: &PlayerView,
    vendor_open: bool,
    pking: bool,
    c: &AttackCtx,
) -> bool {
    let region = me.player.region_attr;
    let mut able = pk_inspection_base(region, &me.player, c, me.alt_key) == 0;
    if pk_inspection_base(region, other, c, me.alt_key) != 0 {
        able = false;
    }
    if able {
        if vendor_open {
            // a vendor can be attacked only by force or by a player it attacked itself
            if !me.force_attack && !pking {
                able = false;
            }
        } else if region == RA_PENALTY_PK && !me.force_attack && !other.chaotic {
            // the penalty-PK region protects everybody who is not chaotic, unless the attack is forced
            able = false;
        }
    }
    able
}

/// `CMover::IsAttackAbleNPC(npc)` for an attacker that is not text-disguised: alive, killable, not peaceful, a
/// guard only against the opposite alignment, ground meets ground and air meets air.
fn npc_attackable(me: &SelfView, n: &NpcView) -> bool {
    if n.dead || !n.killable || n.peaceful {
        return false;
    }
    if n.guard {
        // the alignments must differ: `nChaotic < 0` refuses a chaotic attacker, `nChaotic > 0` a lawful one
        if n.chaotic < 0 && me.player.chaotic {
            return false;
        }
        if n.chaotic > 0 && !me.player.chaotic {
            return false;
        }
    }
    n.flying == me.player.flying
}

/// The client branch of `CMover::IsAttackAble(target)`: [`ATTACKABLE_NO`], [`ATTACKABLE_YES`] or
/// [`ATTACKABLE_PK_TARGET`] (a player the PK rules allow).
///
/// Order, as in the source: a text disguise refuses everything; a running item cast refuses (the caller
/// cancels it); a non-mover is attackable; for a player target with `EVE_18` the duel partner (the duel must be
/// fighting and neither side flying) decides first, then the war, the school battle and the arena targets, then
/// the PK inspection; the guild-combat participants of the guild-war world and of the 1:1 combat are
/// attackable on top of that, independent of `EVE_18`; an NPC follows `IsAttackAbleNPC`.
pub fn client_attackable(me: &SelfView, target: &TargetView, c: &AttackCtx) -> u32 {
    if me.text_disguise || me.base_motion {
        return ATTACKABLE_NO;
    }
    match target {
        TargetView::Other => ATTACKABLE_YES,
        TargetView::Npc(n) => u32::from(npc_attackable(me, n)),
        TargetView::Player {
            view,
            vendor_open,
            pking,
        } => {
            let a = &me.player;
            let mut result = ATTACKABLE_NO;
            if c.eve_18 {
                if is_pvp_target(a, view) {
                    if a.duel_state == 1 && !a.flying && !view.flying {
                        result = ATTACKABLE_YES;
                    }
                } else if is_war_target(c, a, view)
                    || c.school_battle
                    || is_arena_target(c, a, view)
                {
                    result = ATTACKABLE_YES;
                } else if c.events_pk && pk_inspection(me, view, *vendor_open, *pking, c) {
                    // `IsPKAttackAble` (its arena short cut was handled above)
                    result = ATTACKABLE_PK_TARGET;
                }
            }
            if c.guild_combat && c.world_is_guild_war && is_guild_combat_target(a, view) {
                result = ATTACKABLE_YES;
            }
            if c.guild_combat_1to1 && c.gc1to1_target && is_guild_combat_target(a, view) {
                result = ATTACKABLE_YES;
            }
            result
        }
    }
}
