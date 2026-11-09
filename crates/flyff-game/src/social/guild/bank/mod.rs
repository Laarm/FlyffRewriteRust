//! The guild bank, its log and the guild cloak (Plan/21 §5.8, §5.9; WP-SOCIAL-13; ref/07a §15.1-§15.6, ref/07c §15.1).
//!
//! | module | content |
//! |---|---|
//! | this file | [`GuildBank`] (the 42-slot container the guild keeps), the constants and the helpers the handlers share |
//! | `handlers` | the window, the put, the take (penya and item) and the move |
//! | [`log`] | the log view of the master and the kingpins |
//! | [`cloak`] | `CreateGuildCloak` and the cloak builder the admin console uses |
//! | [`removal`] | the P13 poll (GM confiscation), `removal_matches`, [`removal::remove_stack`] |
//!
//! **State.** The bank is [`GuildBank`] (`ItemContainer::new(ContainerKind::GuildBank, 42, 0)`), the penya is
//! `Guild.gold`. Every change writes the WHOLE document (`SetGuildBank`) like the original; every log row that is
//! written is also pushed on the in-memory `GuildBankLogTail` (the kinds `A`, `D`, `I` and `O` are listed by the
//! log view, `C`, `P` and `R` go to the table only).
//!
//! **Messages (what the client does with them: ref/09a §5.5).** `PutItemGuildBankNtf{mode 1}` to the actor (the
//! client replays `Add` on its mirror of the bank, the item carries its destination slot), `{mode 3}` to the online
//! guild members with the window open inside the ACTOR's observer set. `GetItemGuildBankNtf`: `penya_mine` (the
//! client ADDS the penya itself, so the server adds it with `send = false`), `penya_all` to every other online
//! member, `item_mine` (the client adds the item itself: the server's inventory add is silent), `item_all` to the
//! window-open members in view. A move sends nothing (KEEP-QUIRK KQ-39 e).
//!
//! **One commit block.** A handler validates on the immutable state and on container clones (including
//! `can_snapshot`); the commit block then installs the clones, moves the penya, writes the log, queues the ONE
//! `AtomicBatch` (the snapshot of the member is taken after its inventory and purse changed) and sends the messages.
//! A `Failed` batch is a bug: it is logged at error and the memory is kept (21 §2.4).

pub mod cloak;
mod handlers;
pub mod log;
pub mod removal;

pub use cloak::on_create_guild_cloak;
pub use handlers::{
    on_get_item_guild_bank, on_guild_bank_move_item, on_guild_bank_wnd_close,
    on_guild_bank_wnd_req, on_put_item_guild_bank,
};
pub use log::on_guild_log_view;
pub use removal::{removal_poll, remove_stack};

use super::commit::{self, Origin};
use crate::chat::route::broadcast_filtered;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::{ContainerKind, ItemContainer};
use crate::item::model::Item;
use crate::outbox::Audience;
use crate::world::Worlds;
use flyff_common::config::EventId;
use flyff_common::defs::neuz::MMI_GUILDBANKING;
use flyff_common::{CharacterId, GuildId, ItemSerial, ObjId, SessionId};
use flyff_db::audit::social as audit;
use flyff_db::{GuildBankDoc, GuildBankLogKind, GuildBankLogRecord};
use flyff_proto::pb;

/// `MAX_GUILDBANK`: the slots of the guild bank.
pub const BANK_SLOTS: u16 = 42;

/// The penya the guild cloak costs (`CreateGuildCloak`, ref/07a §15.6).
pub const CLOAK_COST: u32 = 10_000;

/// The 42-slot guild bank (`CGuild::m_GuildBank`): an item container whose storage ids are what the packets
/// call the bank object id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildBank {
    /// The 42 slots.
    pub items: ItemContainer,
}

impl Default for GuildBank {
    fn default() -> GuildBank {
        GuildBank::empty()
    }
}

impl GuildBank {
    /// An empty bank.
    pub fn empty() -> GuildBank {
        GuildBank {
            items: ItemContainer::new(ContainerKind::GuildBank, BANK_SLOTS, 0),
        }
    }

    /// The bank of a loaded document. The database repairs a document before the game sees it, so an item that
    /// finds no slot here is logged at error (it is never dropped silently).
    pub fn from_doc(doc: GuildBankDoc) -> GuildBank {
        let (items, orphans) =
            ItemContainer::from_record(&doc.container, ContainerKind::GuildBank, BANK_SLOTS, 0);
        for o in orphans {
            tracing::error!(
                item = o.item_id,
                count = o.count,
                serial = o.serial,
                "an item of a guild bank found no slot and is not in memory"
            );
        }
        GuildBank { items }
    }

    /// The document to store (`SetGuildBank`): the whole bank.
    pub fn to_doc(&self) -> GuildBankDoc {
        GuildBankDoc {
            container: self.items.to_record(),
            ..GuildBankDoc::default()
        }
    }

    /// How many slots hold an item.
    pub fn count(&self) -> usize {
        self.items.count()
    }

    /// `GuildBankWndNtf.items`: the container as the client reads it.
    pub fn to_pb(&self) -> pb::ItemContainer {
        self.items.to_pb()
    }
}

// ---- the gate and the checks the handlers share ------------------------------------------------------------------

/// The member behind a request that passed the common gate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Member {
    /// The character.
    pub c: CharacterId,
    /// Its guild.
    pub guild: GuildId,
}

/// The common gate of the seven handlers: `events.guild_bank` is on and the actor belongs to a guild.
pub(crate) fn member(svc: &Services, ctx: &GameCtx<'_>, who: PlayerRef) -> Option<Member> {
    if ctx.live.event_state(EventId::GuildInventory) == 0 {
        return None;
    }
    let c = super::character_of(ctx, who)?;
    let guild = svc.social.guild.guild_of(c)?;
    Some(Member { c, guild })
}

/// `true` while the actor has the guild bank window open (VR-016).
pub(crate) fn window_open(ctx: &GameCtx<'_>, who: PlayerRef) -> bool {
    ctx.player(who)
        .and_then(|m| m.player())
        .is_some_and(|p| p.social.guild_bank_open)
}

/// The avatar stands in a guild house world, 212, 213 or 214 (legacy `IsGuildHouse`, `DPSrvr.cpp:3575-3581, 3664-3670`:
/// the guild bank works there without a banker NPC; doc 22 §12, task guild-house).
pub(crate) fn in_guild_house(ctx: &GameCtx<'_>, who: PlayerRef) -> bool {
    ctx.player(who)
        .is_some_and(|m| crate::event::guild_house::is_guild_house_world(m.core.world))
}

/// A guild-bank NPC within `gameplay.npc_menu_range` (VR-012, `NpcChecker::close_npc(MMI_GUILDBANKING, ..)`), or
/// the guild house. Put and take re-check it on every request; the window and the move do not (KEEP).
pub(crate) fn near_banker(ctx: &GameCtx<'_>, who: PlayerRef) -> bool {
    if in_guild_house(ctx, who) {
        return true;
    }
    ctx.player(who).is_some_and(|m| {
        ctx.worlds
            .close_npc(MMI_GUILDBANKING, m.core.world, m.core.layer, m.core.pos)
            .is_some()
    })
}

/// `NumberClamp(n, 1, count)`.
pub(crate) fn clamp_num(n: u32, stack: u16) -> u16 {
    u16::try_from(n.clamp(1, u32::from(stack.max(1)))).unwrap_or(1)
}

/// The part of `item` that moves: `n` pieces, no reservation. A PARTIAL part has no serial yet (serial 0): the
/// container hands it a fresh one, so no two stacks ever share a serial (19 §2.7; the original copied the serial).
pub(crate) fn moved_part(item: &Item, n: u16) -> Item {
    let mut it = item.clone();
    it.count = n;
    it.reserved = 0;
    it.vendor_price = 0;
    if n < item.count {
        it.serial = ItemSerial(0);
    }
    it
}

/// `true` when `o` is an online member of `guild` with the bank window open.
fn viewer(worlds: &Worlds, o: ObjId, guild: GuildId) -> bool {
    worlds
        .player(o)
        .and_then(|m| m.player())
        .is_some_and(|p| p.social.guild == Some(guild) && p.social.guild_bank_open)
}

/// `msg` to the online members of `guild` with the window open that stand inside the observer set of `actor`
/// (`Audience::NearExclSelf`, encoded once): the `mode 3` / `item_all` fan-out of a put and a take (KQ-39 e).
pub(crate) fn tell_viewers(
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    actor: PlayerRef,
    guild: GuildId,
    msg: impl Into<pb::server_msg::Msg>,
) {
    broadcast_filtered(
        svc,
        ctx,
        actor.0,
        Audience::NearExclSelf,
        msg,
        |_: SessionId, o: ObjId, worlds: &Worlds| viewer(worlds, o, guild),
    );
}

/// The wall-clock seconds of this tick.
pub(crate) fn now(ctx: &GameCtx<'_>) -> flyff_common::UnixTime {
    commit::now(ctx)
}

/// Keeps `row` in the in-memory log tail of `guild` (a kind without a view is not kept).
pub(crate) fn remember(svc: &mut Services, guild: GuildId, row: &GuildBankLogRecord) {
    if let Some(g) = svc.social.guild.get_mut(guild) {
        g.log_tail.push(row.clone());
    }
}

/// The `guild.bank` audit line of one change (`item` is the moved item with its moved count, `gold` the penya).
pub(crate) fn emit_bank(
    ctx: &GameCtx<'_>,
    origin: Origin,
    guild: GuildId,
    member: CharacterId,
    kind: GuildBankLogKind,
    item: Option<&Item>,
    gold: i64,
) {
    let record = item.map(|i| i.to_record(0));
    commit::emit(
        ctx,
        origin,
        audit::guild_bank(guild, member, kind, record.as_ref(), gold),
    );
}

/// The `guild.bank` audit line of a move (`kind: "move"`: the legacy type 4 collided with the cloak and the swap has
/// no log row, 21 §5.9).
pub(crate) fn emit_move(ctx: &GameCtx<'_>, origin: Origin, guild: GuildId, member: CharacterId) {
    commit::emit(
        ctx,
        origin,
        flyff_db::AuditEvent::new(
            flyff_db::audit::ev::GUILD_BANK,
            serde_json::json!({
                "guild": guild.0, "member": member.0, "kind": "move", "item": serde_json::Value::Null, "gold": 0,
            }),
        ),
    );
}
