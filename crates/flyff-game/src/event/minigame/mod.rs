//! `event::minigame`: the four NPC event mini-games (Plan/22 §16, WP-EVENT-20; ref/15 §17; ref/07c §7;
//! src:_Common/MiniGame.cpp, src:WORLDSERVER/DPSrvr.cpp:9569-9716).
//!
//! * **Kawibawibo** (`kawibawibo.rs`): rock-paper-scissors against the NPC for a win-streak prize,
//! * **Reassemble** (`reassemble.rs`): nine card items for a weighted prize,
//! * **Alphabet / find-word** (`alphabet.rs`): five letter cards spell the offered word,
//! * **Five-system** (`five_system.rs`): stakes on six dice faces, the payout is the stake x `MULTIPLE`.
//!
//! Every request first needs its NPC menu (`MMI_KAWIBAWIBO 44`, `MMI_FINDWORD 45`, `MMI_FIVESYSTEM 46`,
//! `MMI_REASSEMBLE 47`) within reach (VR-012, PL-107); `FiveSystemDestroyWnd` is exempt so a player who walked away
//! still gets the stakes back. The ten client messages are routed by `handlers.rs` (`ClientCall` overrides in
//! `crate::handlers`), the answers are `MiniGame{sub}` messages. Prizes and penya go through `deliver.rs` (inventory
//! else mail; penya by mail when the wallet would overflow); `pick.rs` is the weighted sampler (PL-105), public for
//! the rainbow race. The logout refund of the five-system stakes is [`MiniGameLogout`] (`LogoutStep::CloseMiniGame`).
//!
//! The per-player state ([`MiniGamePlayer`]) is not persisted; it is kept in [`NpcMiniGameService`] by `ObjId` and
//! dropped at logout (ObjIds never repeat, so an entry that a tainted session leaves behind is harmless). With the
//! shipped worlds exactly one mini-game NPC stands: `MaFl_KAWIBAWIBO` in `WdVolcaneYellow` (world 207).

pub mod alphabet;
pub mod deliver;
pub mod five_system;
pub mod handlers;
pub mod kawibawibo;
pub mod logout;
pub mod pick;
pub mod reassemble;

pub use five_system::close_five_system;
pub use logout::MiniGameLogout;
pub use pick::weighted_pick;

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use flyff_common::ObjId;
use flyff_proto::pb;
use std::collections::{BTreeMap, BTreeSet};

/// `KAWIBAWIBO_*`: the result of a round (`MiniGame.h`), also the state the next round depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KawiResult {
    /// `KAWIBAWIBO_WIN` 0.
    Win = 0,
    /// `KAWIBAWIBO_LOST` 1.
    Lost = 1,
    /// `KAWIBAWIBO_DRAW` 2.
    Draw = 2,
    /// `KAWIBAWIBO_FAILED` 3: no coupon, no row. The state after login (User.cpp:199).
    Failed = 3,
    /// `KAWIBAWIBO_GETITEM` 4.
    GetItem = 4,
}

/// The mini-game state of one player (`m_nKawibawiboWin`, `m_nKawibawiboState`, `m_nBetFiveSystem[6]`, and the word
/// the alphabet window offered).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MiniGamePlayer {
    /// The current win streak.
    pub kawi_streak: i32,
    /// The result of the last round.
    pub kawi_last: KawiResult,
    /// The stake on each of the six dice faces.
    pub five_bets: [i32; 6],
    /// The index of the alphabet word offered by the last `AlphabetOpenWnd`.
    pub alphabet_question: Option<usize>,
}

impl Default for MiniGamePlayer {
    fn default() -> Self {
        MiniGamePlayer {
            kawi_streak: 0,
            kawi_last: KawiResult::Failed,
            five_bets: [0; 6],
            alphabet_question: None,
        }
    }
}

/// The mini-game state of all players (`Services.event.minigames`).
#[derive(Debug, Default)]
pub struct NpcMiniGameService {
    players: BTreeMap<ObjId, MiniGamePlayer>,
    warned: BTreeSet<u32>,
}

impl NpcMiniGameService {
    /// The state of `obj`, if it ever played.
    pub fn player(&self, obj: ObjId) -> Option<&MiniGamePlayer> {
        self.players.get(&obj)
    }

    /// The state of `obj`, created with the defaults on first use.
    pub(crate) fn player_mut(&mut self, obj: ObjId) -> &mut MiniGamePlayer {
        self.players.entry(obj).or_default()
    }

    /// How many players have a state.
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// `true` when nobody has a state.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// `true` the first time `key` is asked (a warning per data fault, not per request).
    pub(crate) fn warn_once(&mut self, key: u32) -> bool {
        self.warned.insert(key)
    }

    /// Drops the state of `obj` (logout).
    pub(crate) fn forget(&mut self, obj: ObjId) {
        self.players.remove(&obj);
    }
}

/// `NpcChecker::close_npc( menu, ..)` for the avatar `who` (VR-012, PL-107): an NPC that offers `menu` within reach.
pub(crate) fn at_npc(ctx: &GameCtx<'_>, who: PlayerRef, menu: u32) -> bool {
    ctx.player(who).is_some_and(|m| {
        ctx.worlds
            .close_npc(menu, m.core.world, m.core.layer, m.core.pos)
            .is_some()
    })
}

/// An own, usable inventory item by its wire object id: `(storage id, item id, count)`. A wire id beyond the
/// storage range is no item.
pub(crate) fn usable_item(
    ctx: &GameCtx<'_>,
    who: PlayerRef,
    obj_id: u32,
) -> Option<(StorageId, u32, u16)> {
    let id = StorageId::try_from(obj_id).ok()?;
    let it = ctx.player(who)?.player()?.items.inventory.get_at_id(id)?;
    it.is_usable().then_some((id, it.item_id, it.count))
}

/// The first usable inventory entry holding `item_id`.
pub(crate) fn first_usable(ctx: &GameCtx<'_>, who: PlayerRef, item_id: u32) -> Option<StorageId> {
    ctx.player(who)?
        .player()?
        .items
        .inventory
        .iter()
        .find(|(_, it)| it.item_id == item_id && it.is_usable())
        .map(|(id, _)| id)
}

/// The wallet of `who`.
pub(crate) fn wallet(ctx: &GameCtx<'_>, who: PlayerRef) -> i64 {
    ctx.player(who)
        .and_then(|m| m.player())
        .map_or(0, |p| i64::from(p.items.gold))
}

/// Queues a `MiniGame{sub}` message to the player itself.
pub(crate) fn reply(fx: &mut Fx, sub: pb::mini_game::Sub) {
    fx.out.to_self(pb::MiniGame { sub: Some(sub) });
}

/// `LogoutStep::CloseMiniGame` (User.cpp:4040): refunds the stakes of an open five-system window, then forgets the
/// player's mini-game state.
pub fn on_logout(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    close_five_system(svc, ctx, who);
    svc.event.minigames.forget(who.0);
}
