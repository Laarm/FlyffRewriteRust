//! The TELE16 teleporter rules library (Plan/15 §10.7, WP-WORLD-19; F-341, VR-063, PL-017). A port of
//! `CDPSrvr::OnTeleport` (src:WORLDSERVER/DPSrvr.cpp:13220-13265) plus the added rule VR-063.
//!
//! **A pure library:** no message handler, no message sends, no side effects. The `Tele16` handler of doc 20
//! (WP-NPC-08, [`crate::npc::services::on_tele16`]) calls [`validate`] and performs the effects in the order of the
//! handler contract (15 §10.7):
//!
//! 1. `check.deactivate_eat_pet` -> doc 19's eat-pet deactivation (before anything else),
//! 2. `Err(Text(t))` -> send `Text{kind: 1, t}` (the repack's `AddText`), `Err(WorldMissing)` -> log an error,
//!    `Err(Ignore)` -> nothing,
//! 3. `Ok(target)` -> `Worlds::replace(player, target.world, target.pos, layer 0, Normal)` and, on success,
//!    [`Cooldowns::record`]`(character, target.index, now)`.
//!
//! [`validate`] reads the player and the configuration from a [`GameCtx`]; [`evaluate`] is the same decision over
//! plain data ([`Tele16Subject`]) for tests and for callers that already hold the facts.
//!
//! Decisions where the plan is silent (documented in `NOTES.md`): the level range applies when `min_level > 0`
//! (the client prints "Level Cap: N/A" otherwise), the `class` column is enforced whenever it is non-zero,
//! `HasActivatedSystemPet() || HasPet()` is "a system pet item is summoned" (`items.pet_obj_id`), a player who
//! browses the personal shop of another player counts as `IsVendorOpen()`.

use super::ReplaceKind;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use flyff_common::defs::job::{JTYPE_HERO, JTYPE_MASTER, MAX_LEGEND_LEVEL};
use flyff_common::defs::world::WI_WORLD_GUILDWAR;
use flyff_common::det::DetMap;
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, GameMs, LayerId, WorldId};
use flyff_data::GameData;
use flyff_sim::types::OBJSTA_COLLECT;

/// The text of the active-pet refusal (step 3).
pub const TEXT_PET: &str = "Can't teleport with an active Pet or Egg";
/// The text of the fight refusal (step 5).
pub const TEXT_FIGHTING: &str = "Can't teleport while fighting.";
/// The text of the level refusal (VR-063).
pub const TEXT_LEVEL: &str = "Your level does not meet the requirement.";
/// The text of the class refusal (VR-063).
pub const TEXT_CLASS: &str = "Your class does not meet the requirement.";
/// The text of the cool-down refusal (VR-063).
pub const TEXT_COOLDOWN: &str = "Teleport is on cooldown.";

/// Why a request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tele16Refusal {
    /// Silent: the feature is off, the index does not exist, or the player cannot teleport right now.
    Ignore,
    /// The player gets this text (`AddText`, `Text{kind: 1}`).
    Text(&'static str),
    /// The target world is not loaded: an error log, nothing for the player.
    WorldMissing,
}

/// An accepted request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tele16Target {
    /// The `teleport16.inc` row.
    pub index: u32,
    /// The destination world.
    pub world: WorldId,
    /// The destination position.
    pub pos: Vec3,
}

/// The result of [`validate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tele16Check {
    /// Step 4: the handler deactivates the active eat pet (doc 19) before anything else. Set only when steps
    /// 0-3 passed and regardless of steps 5-7, exactly as the original deactivated it before the fight test.
    pub deactivate_eat_pet: bool,
    /// The decision.
    pub result: Result<Tele16Target, Tele16Refusal>,
}

/// The last successful teleport of each `(character, row)`: in memory, never persisted; lives in
/// [`Worlds::tele16_cooldowns`](super::Worlds) (VR-063).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cooldowns {
    last: DetMap<(CharacterId, u32), GameMs>,
}

impl Cooldowns {
    /// No teleport yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` when the player never teleported to `index`, or at least `cooldown_s` seconds have passed since.
    pub fn ready(&self, ch: CharacterId, index: u32, cooldown_s: u32, now: GameMs) -> bool {
        match self.last.get(&(ch, index)) {
            None => true,
            Some(at) => now.0.saturating_sub(at.0) >= u64::from(cooldown_s) * 1000,
        }
    }

    /// Remembers a successful teleport.
    pub fn record(&mut self, ch: CharacterId, index: u32, now: GameMs) {
        self.last.insert((ch, index), now);
    }

    /// Forgets the entries of a character (logout).
    pub fn forget(&mut self, ch: CharacterId) {
        self.last.retain(|(c, _), _| *c != ch);
    }

    /// The number of remembered teleports.
    pub fn len(&self) -> usize {
        self.last.len()
    }

    /// `true` when nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.last.is_empty()
    }
}

/// The row of `teleport16.inc` as `(world, position)`, no checks (`prj.TPData[index]` on the world server).
pub fn resolve_destination(data: &GameData, index: u32) -> Option<(WorldId, Vec3)> {
    let row = data.teleports16.get(usize::try_from(index).ok()?)?;
    Some((
        WorldId(row.world),
        Vec3::new(row.pos[0], row.pos[1], row.pos[2]),
    ))
}

/// The switches of the rules (`[features]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tele16Rules {
    /// `features.teleportation_system`.
    pub enabled: bool,
    /// `features.reactivate_eatpet`.
    pub reactivate_eatpet: bool,
}

impl Default for Tele16Rules {
    fn default() -> Self {
        Self {
            enabled: true,
            reactivate_eatpet: true,
        }
    }
}

/// What the rules need to know about the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tele16Subject {
    /// The character (the cool-down key).
    pub character: CharacterId,
    /// The player is in a world (`GetWorld()` is not null).
    pub in_world: bool,
    /// The world the player is in.
    pub world: WorldId,
    /// `IsDie()`.
    pub dead: bool,
    /// `IsCollecting()`.
    pub collecting: bool,
    /// `VendorIsVendor()`: a personal shop is open.
    pub vending: bool,
    /// `IsVendorOpen()`: the player browses a personal shop.
    pub vendor_open: bool,
    /// `HasActivatedSystemPet() || HasPet()`.
    pub pet: bool,
    /// `HasActivatedEatPet()`.
    pub eat_pet: bool,
    /// `IsAuthHigher( AUTH_GAMEMASTER )`.
    pub gm: bool,
    /// `IsAttackMode()`.
    pub attack_mode: bool,
    /// `m_nLevel`.
    pub level: i32,
    /// The `dwJobType` of the job (`propJob.inc`); `u32::MAX` for none.
    pub job_type: u32,
}

/// The decision of 15 §10.7 over plain data, side-effect free. `world_loaded` answers "does the server run this
/// world" for step 7.
pub fn evaluate(
    data: &GameData,
    rules: Tele16Rules,
    s: &Tele16Subject,
    index: u32,
    now: GameMs,
    cd: &Cooldowns,
    world_loaded: &dyn Fn(WorldId) -> bool,
) -> Tele16Check {
    let refuse = |r: Tele16Refusal, deactivate_eat_pet: bool| Tele16Check {
        deactivate_eat_pet,
        result: Err(r),
    };
    // 0. the feature
    if !rules.enabled {
        return refuse(Tele16Refusal::Ignore, false);
    }
    // 1. the row (PL-017: 17 rows, the phantom 18th does not exist)
    let Some(row) = usize::try_from(index)
        .ok()
        .and_then(|i| data.teleports16.get(i))
    else {
        return refuse(Tele16Refusal::Ignore, false);
    };
    // 2. silent states
    if !s.in_world || s.dead || s.collecting || s.vending || s.vendor_open {
        return refuse(Tele16Refusal::Ignore, false);
    }
    // 3. an active pet or egg
    if s.pet {
        return refuse(Tele16Refusal::Text(TEXT_PET), false);
    }
    // 4. the eat pet is deactivated by the handler, whatever follows
    let deactivate = rules.reactivate_eatpet && s.eat_pet;
    // 5. fighting (game masters are exempt)
    if !s.gm && (s.attack_mode || s.world.0 == WI_WORLD_GUILDWAR) {
        return refuse(Tele16Refusal::Text(TEXT_FIGHTING), deactivate);
    }
    // 6. VR-063 (game masters are exempt)
    if !s.gm {
        if row.min_level > 0 {
            // the client rule: `max < min` is the legend cap
            let max = if row.max_level < row.min_level {
                MAX_LEGEND_LEVEL
            } else {
                row.max_level
            };
            let level = i64::from(s.level);
            if level < i64::from(row.min_level) || level > i64::from(max) {
                return refuse(Tele16Refusal::Text(TEXT_LEVEL), deactivate);
            }
        }
        let class_ok = match row.class {
            0 => true,
            1 => s.job_type == JTYPE_MASTER || s.job_type == JTYPE_HERO,
            2 => s.job_type == JTYPE_HERO,
            _ => true,
        };
        if !class_ok {
            return refuse(Tele16Refusal::Text(TEXT_CLASS), deactivate);
        }
        if row.cooldown > 0 && !cd.ready(s.character, index, row.cooldown, now) {
            return refuse(Tele16Refusal::Text(TEXT_COOLDOWN), deactivate);
        }
    }
    // 7. the target world
    let world = WorldId(row.world);
    if !world_loaded(world) {
        return refuse(Tele16Refusal::WorldMissing, deactivate);
    }
    // 8.
    Tele16Check {
        deactivate_eat_pet: deactivate,
        result: Ok(Tele16Target {
            index,
            world,
            pos: Vec3::new(row.pos[0], row.pos[1], row.pos[2]),
        }),
    }
}

/// The facts [`evaluate`] needs about a live player; `None` when `who` is not a live player with a session.
pub fn subject_of(ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Tele16Subject> {
    let m: &Mover = ctx.worlds.player(who.0)?;
    let p = m.player()?;
    let entry = ctx.sessions.get(who.0);
    Some(Tele16Subject {
        character: entry.map_or(CharacterId(0), |e| e.character),
        in_world: m.core.world.0 != 0,
        world: m.core.world,
        dead: crate::combat::state::is_die(m),
        collecting: m.sim.act.state.bits() & OBJSTA_COLLECT != 0,
        vending: p.econ.vending,
        vendor_open: matches!(
            p.econ.partner,
            crate::econ::TradePartner::VendorBrowse { .. }
        ),
        pet: p.items.pet_obj_id.is_some(),
        eat_pet: p.items.summoned_eat_pet.is_some(),
        gm: entry.is_some_and(|e| e.authority.is_gm()),
        attack_mode: crate::stats::tick::is_attack_mode(m),
        level: m.mv.level,
        job_type: u32::try_from(m.mv.job)
            .ok()
            .and_then(|j| ctx.data.jobs.info(j))
            .map_or(u32::MAX, |j| j.job_type),
    })
}

/// The rules switches of the running configuration.
pub fn rules_of(ctx: &GameCtx<'_>) -> Tele16Rules {
    let f = &ctx.cfg.rules.features;
    Tele16Rules {
        enabled: f.teleportation_system,
        reactivate_eatpet: f.reactivate_eatpet,
    }
}

/// `validate( g, player, index, now, cd )` of 15 §10.7 over a [`GameCtx`]: side-effect free. A player that is not
/// live gets `Ignore`.
pub fn validate(
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    index: u32,
    now: GameMs,
    cd: &Cooldowns,
) -> Tele16Check {
    let rules = rules_of(ctx);
    let Some(subject) = subject_of(ctx, who) else {
        return Tele16Check {
            deactivate_eat_pet: false,
            result: Err(Tele16Refusal::Ignore),
        };
    };
    evaluate(ctx.data, rules, &subject, index, now, cd, &|w| {
        ctx.worlds.world(w).is_some()
    })
}

/// [`validate`] with the cool-downs of [`Worlds`](super::Worlds) and the clock of the tick.
pub fn validate_now(ctx: &GameCtx<'_>, who: PlayerRef, index: u32) -> Tele16Check {
    validate(
        ctx,
        who,
        index,
        ctx.clock.game_ms,
        &ctx.worlds.tele16_cooldowns,
    )
}

/// `Tele16{index}` (15 §10.7, `CDPSrvr::OnTeleport`): the effects of [`validate_now`] in the order of the handler
/// contract. The doc-20 handler (`npc::services::on_tele16`) forwards the decoded message here unchanged and
/// sends nothing itself.
///
/// 1. `check.deactivate_eat_pet`: the eat pet is dismissed first (doc 19; the hook is
///    [`NpcService::deactivate_eat_pet`](crate::npc::NpcService), the default removes the pet mover and clears
///    the handle), whatever the rest says;
/// 2. `Text(t)`: `Text{kind: 1, t}` to the player; `WorldMissing`: an error log; `Ignore`: nothing;
/// 3. `Ok(t)`: `Worlds::replace(player, t.world, t.pos, layer 0, Normal)` and, when the replace was accepted,
///    the cool-down of the row starts ([`Cooldowns::record`]).
pub fn handle(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef, index: u32) {
    let check = validate_now(ctx, who, index);
    if check.deactivate_eat_pet {
        let dismiss = svc.npc.deactivate_eat_pet;
        dismiss(svc, ctx, who);
    }
    match check.result {
        Err(Tele16Refusal::Ignore) => {}
        Err(Tele16Refusal::Text(t)) => notice::text(ctx, who, t),
        Err(Tele16Refusal::WorldMissing) => {
            tracing::error!(index, "Tele16: the target world of the row is not loaded");
        }
        Ok(t) => {
            let character = ctx.sessions.get(who.0).map(|e| e.character);
            match ctx
                .worlds
                .replace(who.0, t.world, t.pos, LayerId(0), ReplaceKind::Normal)
            {
                Ok(()) => {
                    if let Some(ch) = character {
                        let now = ctx.clock.game_ms;
                        ctx.worlds.tele16_cooldowns.record(ch, t.index, now);
                    }
                }
                Err(e) => {
                    tracing::debug!(player = who.0.0, index, error = %e, "Tele16: the replace was refused");
                }
            }
        }
    }
}
