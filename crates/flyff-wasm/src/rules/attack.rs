//! The client attackability export of `RulesContext` (Plan/16 §18.4 M5 row, Plan/43 §7.6, WP-MOVER-23).
//!
//! `client_attackable(self_view, target_view, ctx) -> u32` is a thin wrapper over
//! `flyff_rules::combat::attackable::client_attackable` (the client branch of `CMover::IsAttackAble`): `0` the
//! target cannot be attacked, `1` it can, `2` it is a player the PK rules allow (the caller remembers it as the
//! last target). The views are plain camelCase objects; every field is optional and a missing one is the neutral
//! value (a level-1 mover on the ground, no duel, no war, no guild, no region class):
//!
//! * `selfView`: `{ id, flying, chaotic, duelState, duelOther, duelParty, partyId, guildId, warId,
//!   guildCombatState, level, regionAttr, x, z, textDisguise, baseMotion, forceAttack, altKey }`
//!   (`duelOther` 4294967295 = none; `regionAttr` is the `RA_*` class of the region the character stands in);
//! * `targetView`: the same flat shape with `kind` `'other'` (a ground item or a ctrl), `'player'` (the default; also
//!   `vendorOpen`, `pking`) or `'npc'` (`dead`, `killable`, `peaceful`, `guard`, `alignment` = `nChaotic`, `flyingNpc`);
//! * `ctx`: `{ eve18, eventsPk, guildWar, schoolBattle, guildCombat, guildCombat1to1, worldIsGuildWar,
//!   worldIsArena, gc1to1Target, pkLimitLevel }` (the event flags and the kind of world; `eve18` defaults to
//!   true, `pkLimitLevel` to 20).
//!
//! The Rust core ([`RulesCore::client_attackable`]) has no `JsValue` and is tested natively.

use super::{RulesContext, RulesCore, from_js};
use flyff_rules::combat::attackable::{
    AttackCtx, NpcView, PlayerView, SelfView, TargetView, client_attackable,
};
use serde::Deserialize;
use wasm_bindgen::prelude::{JsError, JsValue, wasm_bindgen};

/// One mover as the rule sees it (camelCase): the same flat shape serves the own character and the clicked
/// object, so the object needs no tagged union (which `serde-wasm-bindgen` would have to buffer). A missing field
/// is the neutral value.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AttackViewJs {
    /// `'player'` (the default), `'npc'` or `'other'` (a ground item or a ctrl). Only the target's matters.
    pub kind: String,
    /// The object id (`GetId()`).
    pub id: u32,
    /// `IsFly()`.
    pub flying: bool,
    /// `IsChaotic()`.
    pub chaotic: bool,
    /// `m_nDuelState`.
    pub duel_state: i32,
    /// `m_idDuelOther` (`0xFFFFFFFF` = none).
    pub duel_other: u32,
    /// `m_idDuelParty`.
    pub duel_party: i32,
    /// `m_idparty`.
    pub party_id: i32,
    /// `m_idGuild`.
    pub guild_id: u32,
    /// `m_idWar`.
    pub war_id: i32,
    /// `m_nGuildCombatState`.
    pub guild_combat_state: i32,
    /// `GetLevel()`.
    pub level: i32,
    /// `GetPKPVPRegionAttr()`: the `RA_*` class of the region.
    pub region_attr: u32,
    /// `GetPos().x`.
    pub x: f32,
    /// `GetPos().z`.
    pub z: f32,
    // ---- the own character only ----
    /// `HasBuffByIk3(IK3_TEXT_DISGUISE)`.
    pub text_disguise: bool,
    /// `IsStateMode(STATE_BASEMOTION_MODE)`.
    pub base_motion: bool,
    /// `IsForceAttack()`.
    pub force_attack: bool,
    /// The alt key.
    pub alt_key: bool,
    // ---- a player target only ----
    /// `m_vtInfo.IsVendorOpen()`.
    pub vendor_open: bool,
    /// `g_Neuz.m_NeuzEnemy.IsPKing(id)`.
    pub pking: bool,
    // ---- an NPC target only ----
    /// `IsDie()`.
    pub dead: bool,
    /// `bKillable`.
    pub killable: bool,
    /// `IsPeaceful()`.
    pub peaceful: bool,
    /// `dwClass == RANK_GUARD`.
    pub guard: bool,
    /// `nChaotic` of the prop (guards only).
    pub alignment: i32,
    /// `dwFlying == 1`.
    pub flying_npc: bool,
}

impl Default for AttackViewJs {
    fn default() -> Self {
        let p = PlayerView::default();
        let n = NpcView::default();
        AttackViewJs {
            kind: "player".to_owned(),
            id: p.id,
            flying: p.flying,
            chaotic: p.chaotic,
            duel_state: p.duel_state,
            duel_other: p.duel_other,
            duel_party: p.duel_party,
            party_id: p.party_id,
            guild_id: p.guild_id,
            war_id: p.war_id,
            guild_combat_state: p.guild_combat_state,
            level: p.level,
            region_attr: p.region_attr,
            x: p.x,
            z: p.z,
            text_disguise: false,
            base_motion: false,
            force_attack: false,
            alt_key: false,
            vendor_open: false,
            pking: false,
            dead: n.dead,
            killable: n.killable,
            peaceful: n.peaceful,
            guard: n.guard,
            alignment: n.chaotic,
            flying_npc: n.flying,
        }
    }
}

impl AttackViewJs {
    fn player(&self) -> PlayerView {
        PlayerView {
            id: self.id,
            flying: self.flying,
            chaotic: self.chaotic,
            duel_state: self.duel_state,
            duel_other: self.duel_other,
            duel_party: self.duel_party,
            party_id: self.party_id,
            guild_id: self.guild_id,
            war_id: self.war_id,
            guild_combat_state: self.guild_combat_state,
            level: self.level,
            region_attr: self.region_attr,
            x: self.x,
            z: self.z,
        }
    }

    fn as_self(&self) -> SelfView {
        SelfView {
            player: self.player(),
            text_disguise: self.text_disguise,
            base_motion: self.base_motion,
            force_attack: self.force_attack,
            alt_key: self.alt_key,
        }
    }

    fn as_target(&self) -> TargetView {
        match self.kind.as_str() {
            "other" => TargetView::Other,
            "npc" => TargetView::Npc(NpcView {
                dead: self.dead,
                killable: self.killable,
                peaceful: self.peaceful,
                guard: self.guard,
                chaotic: self.alignment,
                flying: self.flying_npc,
            }),
            _ => TargetView::Player {
                view: self.player(),
                vendor_open: self.vendor_open,
                pking: self.pking,
            },
        }
    }
}

/// The world state (camelCase).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AttackCtxJs {
    /// `EVE_18`.
    pub eve18: bool,
    /// `EVE_PK`.
    pub events_pk: bool,
    /// `EVE_GUILDWAR`.
    pub guild_war: bool,
    /// `SBS_START2`.
    pub school_battle: bool,
    /// `EVE_GUILDCOMBAT`.
    pub guild_combat: bool,
    /// `EVE_GUILDCOMBAT1TO1`.
    pub guild_combat_1to1: bool,
    /// The world is the guild-war map.
    pub world_is_guild_war: bool,
    /// The world is an arena.
    pub world_is_arena: bool,
    /// The target takes part in the 1:1 guild combat.
    pub gc1to1_target: bool,
    /// `PKSetting.LIMITE_LEVEL`.
    pub pk_limit_level: i32,
}

impl Default for AttackCtxJs {
    fn default() -> Self {
        let d = AttackCtx::default();
        AttackCtxJs {
            eve18: d.eve_18,
            events_pk: d.events_pk,
            guild_war: d.guild_war,
            school_battle: d.school_battle,
            guild_combat: d.guild_combat,
            guild_combat_1to1: d.guild_combat_1to1,
            world_is_guild_war: d.world_is_guild_war,
            world_is_arena: d.world_is_arena,
            gc1to1_target: d.gc1to1_target,
            pk_limit_level: d.pk_limit_level,
        }
    }
}

impl From<AttackCtxJs> for AttackCtx {
    fn from(v: AttackCtxJs) -> AttackCtx {
        AttackCtx {
            eve_18: v.eve18,
            events_pk: v.events_pk,
            guild_war: v.guild_war,
            school_battle: v.school_battle,
            guild_combat: v.guild_combat,
            guild_combat_1to1: v.guild_combat_1to1,
            world_is_guild_war: v.world_is_guild_war,
            world_is_arena: v.world_is_arena,
            gc1to1_target: v.gc1to1_target,
            pk_limit_level: v.pk_limit_level,
        }
    }
}

impl RulesCore {
    /// The client branch of `IsAttackAble` (see the module notes): `0` no, `1` yes, `2` a PK target.
    pub fn client_attackable(
        &self,
        me: &AttackViewJs,
        target: &AttackViewJs,
        ctx: &AttackCtxJs,
    ) -> u32 {
        client_attackable(&me.as_self(), &target.as_target(), &(*ctx).into())
    }
}

#[wasm_bindgen]
impl RulesContext {
    /// M5 (WP-MOVER-23): may the own character attack the clicked object? `selfView`, `targetView` and `ctx` are
    /// the camelCase objects of the module notes (every field optional); the answer is `0` no, `1` yes, `2` a PK
    /// target. A malformed view throws and answers nothing.
    pub fn client_attackable(
        &self,
        self_view: JsValue,
        target_view: JsValue,
        ctx: JsValue,
    ) -> Result<u32, JsError> {
        let me: AttackViewJs = view_or_default(self_view, "client_attackable selfView")?;
        let target: AttackViewJs = from_js(target_view, "client_attackable targetView")?;
        let ctx: AttackCtxJs = view_or_default(ctx, "client_attackable ctx")?;
        Ok(self.core.client_attackable(&me, &target, &ctx))
    }
}

/// `undefined` / `null` is the neutral view.
fn view_or_default<T: serde::de::DeserializeOwned + Default>(
    v: JsValue,
    what: &str,
) -> Result<T, JsError> {
    if v.is_undefined() || v.is_null() {
        Ok(T::default())
    } else {
        from_js(v, what)
    }
}
