//! Follow-up actions of the mover-level functions (Plan/17 §7.4, §6.9; Plan/19 §5.7; WP-SKILL-01/02,
//! WP-ITEM-09/10/21).
//!
//! The functions of `buff`, `skill::apply` and the item rows work on one `&mut Mover` and must not touch the
//! rest of the world (the same discipline as `stats` and `item::ops`). What they cannot do themselves - hurt
//! another mover, create an item for a player, teleport, run a GM text command, tell the social services about
//! a premium mode - they queue as a [`Followup`] in the [`Fx`](crate::item::fx::Fx) they fill. The caller that
//! flushes the `Fx` through the services runs the follow-ups right after the messages
//! ([`Followup::run`]), in the order they were queued.

use crate::combat::{DamageRequest, send_damage};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::model::Item;
use crate::item::ops::{ItemOrigin, create_item};
use crate::stats::dst::DstSource;
use crate::stats::out::Out;
use crate::stats::source::apply_dst;
use crate::world::ReplaceKind;
use flyff_common::math::Vec3;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_rules::combat::{AtkFlags, DamageParam};
use flyff_rules::consts::{NULL_CHGPARAM, dst::DST_HP};

/// `PROCESS_COUNT * 30`: the over-heal window in ticks (30 s).
const OVER_HEAL_TICKS: u32 = 450;

/// One deferred action.
#[derive(Clone, Debug, PartialEq)]
pub enum Followup {
    /// `SendDamage(AF_FORCE, attacker, amount)` on `victim`: the expiry of `SI_GEN_BURN`, the drain of
    /// `DST_CHR_STEALHP_IMM`. Without a valid attacker nothing happens (the pipeline refuses the hit).
    ForceDamage {
        /// Who is hurt.
        victim: ObjId,
        /// The attacker of the hit.
        attacker: Option<ObjId>,
        /// The damage.
        amount: i32,
    },
    /// The expiry of `SI_GEN_BURN`: `xRandom(min, max)` damage (rolled on the victim's world stream when this
    /// runs) from the attacker of the buff, `AF_FORCE`.
    BurnDamage {
        /// Who is hurt.
        victim: ObjId,
        /// The caster of the buff.
        attacker: Option<ObjId>,
        /// `ability_min` of the skill.
        min: i32,
        /// `ability_max` of the skill.
        max: i32,
    },
    /// `m_nOverHeal = PROCESS_COUNT * 30` (`ApplyParam` heal): the caster heals a target above its maximum.
    OverHeal {
        /// The caster.
        mover: ObjId,
    },
    /// `SetDestParam(DST_HP, amount)` of a mover: the caster's share of the HP drains.
    AddHp {
        /// The mover that is healed.
        mover: ObjId,
        /// The amount.
        amount: i32,
    },
    /// `IK2_BUFF_TOGIFT` ran out: one `item_id` goes into the inventory, else it is posted.
    ToGift {
        /// The receiving player.
        player: ObjId,
        /// The item.
        item_id: u32,
    },
    /// The revert command of an item buff (`/nodis`, `/noud`...), run with item privilege (PL-114).
    ItemCommand {
        /// The item that ran out.
        item_id: u32,
        /// The player the command acts on.
        player: ObjId,
        /// The text from the second `/` on.
        text: String,
    },
    /// A call into the shared sim on one mover (the end of the yo-yo counter buff resets the attack state).
    Sim {
        /// The mover.
        mover: ObjId,
        /// The call.
        fx: crate::combat::SimFx,
    },
    /// A teleport through the replace pipeline of doc 15 (blink wings, return scrolls, couple rings).
    Teleport {
        /// The mover.
        mover: ObjId,
        /// The target world.
        world: WorldId,
        /// The target layer.
        layer: LayerId,
        /// The target position.
        pos: Vec3,
        /// Normal or forced.
        kind: ReplaceKind,
    },
    /// A premium mode changed: the social services react (party skill modes, 21 §4.8).
    SmChanged {
        /// The player.
        player: ObjId,
        /// The `SM_*` slot.
        sm: u8,
        /// `true` when it became active.
        active: bool,
    },
    /// `II_SYS_SYS_SCR_CUSTODY2`: the instant-bank NPC appears at the player.
    SpawnInstantBank {
        /// The player.
        player: ObjId,
    },
    /// The honor titles are re-checked (doc 21: after a restat, 16 §7.3).
    HonorRecheck {
        /// The player.
        player: ObjId,
    },
    /// A town blink wing: to the revival point of the world (the chaotic one for a chaotic player), else to
    /// the nearest, else to the same spot on the default layer (`DoUseItemBlinkWing`, `IK3_TOWNBLINKWING`).
    BlinkTown {
        /// The player.
        mover: ObjId,
        /// `true` for the escape wing (`REPLACE_FORCE`).
        force: bool,
    },
    /// A blink wing with a fixed destination: the world and spot of the prop, or, for a chaotic player, the
    /// chaotic revival point of `key` there (else the nearest chaotic one).
    BlinkFixed {
        /// The player.
        mover: ObjId,
        /// The world of the prop (`dwWeaponType`).
        world: WorldId,
        /// The spot of the prop.
        pos: Vec3,
        /// The player is chaotic.
        chaotic: bool,
        /// The revival key of the prop (`szTextFileName`), for a chaotic player.
        key: String,
    },
    /// A couple ring carved by `partner` (a character id): the teleport to the partner after the checks of
    /// `DoUseItemWarp` that need the other player.
    CoupleRing {
        /// The player that uses the ring.
        player: ObjId,
        /// The character id the ring is carved with.
        partner: u32,
    },
    /// A system-pet egg of the player hatched (`SetHonorAdd( HS_HATCHING_EGG )`, 19 §13.3).
    EggHatched {
        /// The player.
        player: ObjId,
    },
    /// `II_SYS_SYS_SCR_FRIENDSUMMON_A/B` was used (21 §4.11): the name dialog opens on the scroll in `slot`; the scroll
    /// itself is consumed only by the success of the summon.
    FriendSummon {
        /// The player.
        player: ObjId,
        /// The inventory object id of the scroll.
        slot: u16,
    },
    /// A guild-house item was used (doc 22 §12.3, task guild-house): the kinds 38, 39 and 40 are listed into the furniture
    /// list, the comeback scroll 41 enters the house; the item is consumed by the follow-up on success.
    GuildHouseItem {
        /// The player.
        player: ObjId,
        /// The inventory object id of the item.
        slot: u16,
        /// The item id (the slot must still hold it).
        item_id: u32,
    },
    /// `OnAfterUseItem`: an item was used up (`SetHonorAdd( item, HI_USE_ITEM )`, 21 §13.3).
    ItemUsed {
        /// The player.
        player: ObjId,
        /// The item that was used (the linked one for an `IK3_LINK` item).
        item: u32,
    },
    /// What the buff-like item rows defer to the services: the item buff itself, an item skill, the party level
    /// scroll, the village teleport of the return scroll (`item::buffitems`, WP-ITEM-20).
    BuffItem(crate::item::buffitems::Later),
    /// What the eat pets do with the world: summon the mover, delete it, change its effect (`item::pets::eat`,
    /// WP-ITEM-18).
    EatPet(crate::item::pets::eat::Op),
    // ---- housing (WP-EVENT-16) ----
    /// A furniture or papering item was used (`IK2_FURNITURE` / `IK2_PAPERING`): the list-up of the item at inventory
    /// slot `slot` into the mini room's list (22 §13.2; `event::housing::list_up`).
    HousingListUp {
        /// The player.
        player: ObjId,
        /// The inventory object id of the item.
        slot: u16,
    },
}

impl Followup {
    /// Replaces the mover id `from` by `to` everywhere in the action (a join builds its follow-ups before the
    /// object store has given the player its id).
    pub fn rebind(&mut self, from: ObjId, to: ObjId) {
        let fix = |id: &mut ObjId| {
            if *id == from {
                *id = to;
            }
        };
        match self {
            Followup::ForceDamage {
                victim, attacker, ..
            }
            | Followup::BurnDamage {
                victim, attacker, ..
            } => {
                fix(victim);
                if let Some(a) = attacker {
                    fix(a);
                }
            }
            Followup::OverHeal { mover }
            | Followup::AddHp { mover, .. }
            | Followup::Sim { mover, .. }
            | Followup::BlinkTown { mover, .. }
            | Followup::BlinkFixed { mover, .. }
            | Followup::Teleport { mover, .. } => fix(mover),
            Followup::ToGift { player, .. }
            | Followup::ItemCommand { player, .. }
            | Followup::SmChanged { player, .. }
            | Followup::HonorRecheck { player }
            | Followup::CoupleRing { player, .. }
            | Followup::EggHatched { player }
            | Followup::ItemUsed { player, .. }
            | Followup::FriendSummon { player, .. }
            | Followup::GuildHouseItem { player, .. }
            | Followup::HousingListUp { player, .. }
            | Followup::SpawnInstantBank { player } => fix(player),
            Followup::BuffItem(l) => l.rebind(from, to),
            Followup::EatPet(op) => op.rebind(from, to),
        }
    }

    /// Executes the action with the services.
    pub fn run(self, svc: &mut Services, ctx: &mut GameCtx<'_>) {
        match self {
            Followup::ForceDamage {
                victim,
                attacker,
                amount,
            } => {
                if let Some(attacker) = attacker {
                    send_damage(
                        svc,
                        ctx,
                        DamageRequest::new(
                            victim,
                            attacker,
                            AtkFlags::FORCE,
                            DamageParam(amount.max(0) as u32),
                        ),
                    );
                }
            }
            Followup::BurnDamage {
                victim,
                attacker,
                min,
                max,
            } => {
                let Some(world) = ctx.worlds.core(victim).map(|c| c.world) else {
                    return;
                };
                let amount = flyff_rules::combat::x_random_range(ctx.rng.world(world), min, max);
                Followup::ForceDamage {
                    victim,
                    attacker,
                    amount,
                }
                .run(svc, ctx);
            }
            Followup::OverHeal { mover } => {
                if let Some(m) = ctx.worlds.mover_mut(mover) {
                    m.combat.over_heal_ticks = OVER_HEAL_TICKS;
                }
            }
            Followup::AddHp { mover, amount } => {
                let core = svc.stats.handle();
                let mut out = Out::new();
                if let Some(m) = ctx.worlds.mover_mut(mover) {
                    apply_dst(
                        m,
                        &core,
                        DstSource::Script { tag: 0 },
                        (DST_HP, amount, NULL_CHGPARAM),
                        true,
                        &mut out,
                    );
                }
                out.flush(ctx, mover);
            }
            Followup::ToGift { player, item_id } => to_gift(svc, ctx, player, item_id),
            Followup::ItemCommand {
                item_id,
                player,
                text,
            } => crate::buff::api::run_item_command(svc, ctx, PlayerRef(player), item_id, &text),
            Followup::Sim { mover, fx } => crate::combat::apply_sim_fx_game(ctx, mover, fx),
            Followup::Teleport {
                mover,
                world,
                layer,
                pos,
                kind,
            } => {
                if let Err(e) = ctx.worlds.replace(mover, world, pos, layer, kind) {
                    tracing::debug!(mover = mover.0, "follow-up teleport refused: {e}");
                }
            }
            Followup::SmChanged { player, sm, active } => {
                crate::social::hooks::on_sm_mode_changed(svc, ctx, PlayerRef(player), sm, active);
            }
            Followup::SpawnInstantBank { player } => {
                crate::item::usage::system::spawn_instant_bank(svc, ctx, PlayerRef(player));
            }
            Followup::HonorRecheck { player } => {
                crate::social::hooks::honor_recheck(svc, ctx, PlayerRef(player), true);
            }
            Followup::BlinkTown { mover, force } => {
                crate::item::usage::blink::run_town(ctx, mover, force);
            }
            Followup::BlinkFixed {
                mover,
                world,
                pos,
                chaotic,
                key,
            } => crate::item::usage::blink::run_fixed(ctx, mover, world, pos, chaotic, &key),
            Followup::CoupleRing { player, partner } => {
                crate::item::usage::blink::run_couple_ring(svc, ctx, PlayerRef(player), partner);
            }
            Followup::EggHatched { player } => {
                crate::social::hooks::on_egg_hatched(svc, ctx, PlayerRef(player));
            }
            Followup::FriendSummon { player, slot } => {
                crate::social::messenger::summon::open(svc, ctx, PlayerRef(player), slot);
            }
            Followup::GuildHouseItem {
                player,
                slot,
                item_id,
            } => {
                crate::event::guild_house::on_item_followup(
                    svc,
                    ctx,
                    PlayerRef(player),
                    slot,
                    item_id,
                );
            }
            Followup::ItemUsed { player, item } => {
                crate::social::hooks::on_item_used(svc, ctx, PlayerRef(player), item);
            }
            Followup::BuffItem(l) => l.run(svc, ctx),
            Followup::EatPet(op) => op.run(svc, ctx),
            Followup::HousingListUp { player, slot } => {
                crate::event::housing::list_up(svc, ctx, PlayerRef(player), slot);
            }
        }
    }
}

/// `CBuffItem` release of `IK2_BUFF_TOGIFT` (`buff.cpp:94-114`): one `item_id` (charged as its prop says) is
/// created in the inventory; when the bag is full the original posted it to the player. The mail service
/// (WP-ECON-06) is not in this build: the item is put on the ground at the player's feet with the player as
/// the owner, and the event is audited, so the reward is never lost.
fn to_gift(svc: &mut Services, ctx: &mut GameCtx<'_>, player: ObjId, item_id: u32) {
    let who = PlayerRef(player);
    let Some(prop) = ctx.data.item(item_id) else {
        return;
    };
    let mut item = Item::new(item_id, 1);
    item.charged = prop.charged != 0;
    match create_item(
        svc,
        ctx,
        who,
        item.clone(),
        ItemOrigin::Other("buff_to_gift"),
    ) {
        Ok(_) => {}
        Err(crate::item::ops::ItemError::LackSpace) => {
            let Some(m) = ctx.worlds.mover(player) else {
                return;
            };
            let (world, layer, pos) = (m.core.world, m.core.layer, m.core.pos);
            let record = item.to_record(0);
            let meta = crate::item::loot::GroundItemMeta {
                owner: Some(player),
                drop_time: ctx.clock.game_ms,
                ..crate::item::loot::GroundItemMeta::default()
            };
            let drop = crate::world::ItemDrop::new(record, pos, 0.0, meta);
            if ctx.worlds.spawn_item(world, layer, drop).is_err() {
                tracing::warn!(
                    player = player.0,
                    item = item_id,
                    "BUFF_TOGIFT: the gift was lost"
                );
            }
            crate::audit::emit_for(
                ctx,
                who,
                flyff_db::audit::ev::ITEM_CREATE,
                serde_json::json!({ "item": item_id, "count": 1, "source": "buff_to_gift_post" }),
            );
        }
        Err(e) => tracing::warn!(player = player.0, item = item_id, "BUFF_TOGIFT: {e}"),
    }
}
