//! `npc`: the NPC module of doc 20 (Plan/20 §7-§8).
//!
//! | module | content | WP |
//! |---|---|---|
//! | [`checker`] | `NpcChecker`: the VR-012 / VR-013 proximity index | NPC-19 |
//! | [`engine`] | `on_script_dlg`: throttle, range, session validation, the `DialogOps` response, the error policy | NPC-06 |
//! | [`session`] | `DialogSession`, `Offer`, the link parser (VR-060) | NPC-06 |
//! | [`host`] | `DialogHost`: `ScriptHost` over the game state, `run_dialog` with the predefined quest keys | NPC-05/06/07 |
//! | `helpers`, `quest_ops`, [`social`] | the helper groups of §6.6, §6.5, §6.7 | NPC-07 |
//! | [`idle`] | `#auto` idle speech, the slot item `npc_idle_speech` | NPC-08 |
//! | [`services`] | `Teleporter`, `NpcBuff`, `Tele16` handlers | NPC-08, NPC-18 |
//! | [`menus`] | the click rule (0 menus -> dialog, 1 -> direct, 2+ -> popup) and the registry of the menu owners | NPC-06 |
//!
//! The NPC scripts run in `flyff-lua`; the game thread holds the [`NpcService`] state in `Services.npc` and a
//! [`PlayerNpc`] in every player (`PlayerExt.npc`).

pub mod checker;
pub mod engine;
mod helpers;
pub mod host;
pub mod idle;
pub mod menus;
mod quest_ops;

pub mod services;
pub mod session;
pub mod social;

pub use checker::{DEFAULT_NPC_MENU_RANGE, MAX_MOVER_MENU, MoverView, NpcChecker};
pub use engine::{DialogStats, end_dialog, on_script_dlg};
pub use host::DialogHost;
pub use idle::IdleSpeech;
pub use menus::{MenuAction, MenuHandler, MenuOutcome, MenuOwners, plan_for};
pub use quest_ops::quest_msg;
pub use services::{EatPetHook, NpcBuffHook, Tele16Hook, on_npc_buff, on_tele16, on_teleporter};
pub use session::{DialogSession, Offer, OfferKind, PlayerNpc, parse_links};
pub use social::{GuildFacts, NoSocial, PartyFacts, SocialView};

use crate::quest::{NoQuestData, QuestPredicates};
use flyff_lua::Helper;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The NPC dialog state of the game (`Services.npc`).
#[derive(Debug)]
pub struct NpcService {
    /// The quest rules the flows ask: `Game::new` installs the real `QuestRules` over the loaded quest tables (also
    /// on empty data); [`NoQuestData`] is the default of a bare service (unit tests).
    pub predicates: Arc<dyn QuestPredicates>,
    /// The party and guild binding of the helpers #76-#84 (default: no party, no guild).
    pub social: Arc<dyn SocialView>,
    /// `guild.create_requires_quest`: with `false` (default) the helpers `IsSetQuest` / `GetQuestState` report
    /// `QUEST_CREGUILD` as completed (PL-149, D20-13).
    pub guild_create_requires_quest: bool,
    /// The request counters (`flyff_dialog_requests_total{result}`).
    pub stats: DialogStats,
    /// The `#auto` timers.
    pub idle: IdleSpeech,
    /// The owners of the NPC menu services by `MMI_*` id (see [`menus`]).
    pub menus: MenuOwners,
    /// Doc 17's `buff::npc::on_npc_buff` (WP-SKILL-03).
    pub on_npc_buff: NpcBuffHook,
    /// Doc 15's `world::tele16::handle` (WP-WORLD-19).
    pub on_tele16: Tele16Hook,
    /// Doc 19's eat-pet deactivation, called by `world::tele16::handle` before anything else when an eat pet
    /// is out (15 §10.7 step 4). The default removes the pet mover and clears the handle; the eat-pet module
    /// (WP-ITEM-18) installs the full version (effects, awakening and vis stats).
    pub deactivate_eat_pet: EatPetHook,
    pub(crate) refused_logged: BTreeSet<(String, String)>,
    pub(crate) no_player_logged: BTreeSet<String>,
    pub(crate) stub_logged: BTreeSet<Helper>,
}

impl Default for NpcService {
    fn default() -> Self {
        NpcService {
            predicates: Arc::new(NoQuestData),
            social: Arc::new(NoSocial),
            guild_create_requires_quest: false,
            stats: DialogStats::default(),
            idle: IdleSpeech::default(),
            menus: MenuOwners::default(),
            on_npc_buff: crate::buff::npc::on_npc_buff,
            on_tele16: crate::world::tele16::handle,
            deactivate_eat_pet: services::dismiss_eat_pet,
            refused_logged: BTreeSet::new(),
            no_player_logged: BTreeSet::new(),
            stub_logged: BTreeSet::new(),
        }
    }
}
