//! Join placement, the join sequence driver and the re-attach view (Plan/15 §9, WP-WORLD-09; 11 §2.5;
//! ref/02 §8.2, ref/08 §6.1-§6.2).
//!
//! * **Placement** ([`place_join`], §9.1): the saved position is corrected like `CDPDatabaseClient::OnJoin` did;
//!   an out-of-world position goes to Madrigal (6971.984, 100, 3336.884), keeping the saved layer; a world that
//!   is not loaded, a missing layer, an instance dungeon, the quiz world 211 or a refused guild house enter
//!   Madrigal layer 0 **and** queue a same-world teleport to y 99.8 (**KEEP-QUIRK**: the original queues it and
//!   it only sends a `SetPos` 0.2 m lower). Post-placement teleports (GC1to1 worlds, secret room, the secret
//!   dungeon 151, instance exits) are requested in the same tick and run at `_replace`.
//! * **The join batch** (§9.2): the [`JoinRegistry`] holds one [`JoinContributor`] per step of 11 §2.5, written in
//!   step order into the new session's queue in phase 1; steps whose feature does not exist yet have no
//!   contributor and are skipped (11 §2.5 last paragraph). The spawns of everything visible are the
//!   `add_to_view` of the same tick's `_add`, so the whole sequence leaves in one `ServerBatch`.
//! * **Re-attach** (§9.3): [`replay_for_reattach`] writes the same steps from memory, with `resend_view` instead
//!   of `add_to_view`, and marks the session urgent. Pushes made while the session had no sink were dropped and
//!   are not replayed.

use super::Worlds;
use super::catalog::WorldStatic;
use super::continent::{CONT_DARKON12, CONT_FLARIS, Continents};
use super::environment::{environment_msg, season_msg};
use super::revival::MADRIGAL_START;
use super::spawn::{SpawnCacheStats, SpawnWriter, spawn_self};
use super::visibility::resend_view;
use crate::config::GameConfig;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::live::LiveConfig;
use crate::outbox::Outbox;
use crate::tick::TickClock;
use flyff_common::config::EventId;
use flyff_common::defs::world::{
    WI_DUNGEON_SECRET_0, WI_WORLD_GUILDWAR1TO1_0, WI_WORLD_MADRIGAL, WI_WORLD_QUIZ,
};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, ObjId, SessionId, WorldId};
use flyff_data::GameData;
use flyff_proto::pb;

/// The steps of the join sequence, in wire order (11 §2.5, 15 §9.2). The derived order is the write order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum JoinStep {
    /// 1 `EnvironmentAll{season}`.
    EnvironmentAll,
    /// 2 `WorldReadInfo{world_id, pos}`: the client starts loading the world on it.
    WorldReadInfo,
    /// 3 `SpawnSelf` (always the first mover; the client checks `id_player`).
    SpawnSelf,
    /// 4 `GameTimer`.
    GameTimer,
    /// 5 `TaskBar` (doc 42, `crate::taskbar::TaskBarStep`).
    TaskBar,
    /// 6 `Environment{effect, title}`.
    Environment,
    /// 7 `QueryPlayerDataNtf` x N (doc 21).
    QueryPlayerData,
    /// 8 `AddFriendGameJoin` (doc 21).
    AddFriendGameJoin,
    /// 9 `AddPartyName` (doc 21).
    AddPartyName,
    /// 10 `AddGameJoin` (doc 16).
    AddGameJoin,
    /// 11 `AllGuilds` (doc 21).
    AllGuilds,
    /// 12 `Guild` (doc 21).
    Guild,
    /// 13 `War` (doc 21).
    War,
    /// 14 `FlyffEvent` (doc 22).
    FlyffEvent,
    /// 15 `Lord` (doc 22).
    Lord,
    /// 16 `CoupleNtf` (doc 21).
    Couple,
    /// `EventLuaDesc` x N (doc 22).
    EventLuaDesc,
    /// `Tag` (offline notes, doc 21).
    Tag,
    /// `PartyMember` (doc 21).
    PartyMember,
    /// `SmModeAll` (doc 16).
    SmModeAll,
    /// `GameRate` x 6 (+7 for GMs) (doc 22).
    GameRate,
    /// `GuildCombat` x 5-7 (doc 21).
    GuildCombat,
    /// `Gc1to1NowState` (doc 21).
    Gc1to1NowState,
    /// Secret-room info (doc 22).
    SecretRoomInfo,
    /// `TaxAllInfo` (doc 22).
    TaxAllInfo,
    /// Rainbow-race state (doc 22).
    RainbowRaceState,
    /// `GuildHouseAllInfo` (doc 22).
    GuildHouseAllInfo,
    /// `HousingAllInfo` (doc 22, TQ-11-14): only for a room with an entry or a visitor.
    HousingAllInfo,
    /// `CampusUpdate` (doc 21).
    CampusUpdate,
    /// `ChatSettingsNtf` (doc 23): the privacy toggles and the ignore list, the last step of the batch.
    ChatSettings,
}

/// What a contributor reads: everything except the outbox it writes into.
pub struct JoinCtx<'a> {
    /// The services (presence, social: the contributors of docs 21/22 read them).
    pub svc: &'a Services,
    /// The worlds with the freshly placed player.
    pub worlds: &'a Worlds,
    /// The loaded game data.
    pub data: &'a GameData,
    /// The tick clock (`game_ms` for the `GameTimer`, `local` for the season).
    pub clock: &'a TickClock,
    /// The live configuration.
    pub live: &'a LiveConfig,
    /// The startup configuration.
    pub cfg: &'a GameConfig,
    /// The avatar.
    pub player: ObjId,
    /// The session the batch goes to.
    pub session: SessionId,
    /// `true` for the re-attach replay (15 §9.3), `false` for the join.
    pub reattach: bool,
}

/// One step of the join sequence, owned by the doc that owns the message (15 §9.2). `write` pushes with
/// `out.to_session(cx.session, obj_id, msg)`.
pub trait JoinContributor: Send {
    /// The step this contributor fills.
    fn step(&self) -> JoinStep;
    /// Writes the step's messages (none is fine: e.g. `Guild` for a player without a guild).
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox);
}

/// The registered contributors, written in [`JoinStep`] order (registration order within a step).
pub struct JoinRegistry {
    list: Vec<Box<dyn JoinContributor>>,
}

impl std::fmt::Debug for JoinRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoinRegistry")
            .field(
                "steps",
                &self.list.iter().map(|c| c.step()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl JoinRegistry {
    /// An empty registry.
    pub fn empty() -> JoinRegistry {
        JoinRegistry { list: Vec::new() }
    }

    /// The contributors of this doc, of doc 16 and of doc 42 for the messages the protocol slice defines today:
    /// `EnvironmentAll`, `WorldReadInfo`, `SpawnSelf`, `GameTimer`, `TaskBar`, `Environment` and `AddGameJoin`.
    pub fn standard() -> JoinRegistry {
        let mut r = JoinRegistry::empty();
        r.register(Box::new(EnvironmentAllStep));
        r.register(Box::new(WorldReadInfoStep));
        r.register(Box::new(SpawnSelfStep));
        r.register(Box::new(GameTimerStep));
        r.register(Box::new(crate::taskbar::TaskBarStep));
        r.register(Box::new(EnvironmentStep));
        r.register(Box::new(AddGameJoinStep));
        // doc 21 (WP-SOCIAL-19, -08): the player data of the friends and guild mates, then the friend list
        r.register(Box::new(
            crate::social::messenger::join::QueryPlayerDataStep,
        ));
        r.register(Box::new(
            crate::social::messenger::join::AddFriendGameJoinStep,
        ));
        // doc 21 (WP-SOCIAL-04, -03): the stored party name and the party record of the joiner
        r.register(Box::new(crate::social::party::join::AddPartyNameStep));
        r.register(Box::new(crate::social::party::join::PartyMemberStep));
        // doc 21 (WP-SOCIAL-09): every guild (always, an empty list included) and the joiner's own guild
        r.register(Box::new(crate::social::guild::join::AllGuildsStep));
        r.register(Box::new(crate::social::guild::join::GuildStep));
        // doc 21 (WP-SOCIAL-15): the running war of the joiner's guild
        r.register(Box::new(crate::social::guild::war::join::WarStep));
        // doc 21 (WP-SOCIAL-21, -22): the couple record and the campus of the joiner (nothing for the others)
        r.register(Box::new(crate::social::couple::join::CoupleStep));
        r.register(Box::new(crate::social::campus::join::CampusUpdateStep));
        // doc 21 (WP-SOCIAL-20): the undelivered offline notes of the joiner; doc 23 (WP-GM-22): the privacy toggles and
        // the ignore list, the last step of the batch
        r.register(Box::new(crate::social::messenger::notes::TagStep));
        r.register(Box::new(crate::chat::privacy::ChatSettingsStep));
        // doc 16 (WP-MOVER-16): the running premium timers, after `PartyMember`
        r.register(Box::new(crate::stats::sm::SmModeAllStep));
        // doc 22 (WP-EVENT-02): the event flags, position 14
        r.register(Box::new(crate::event::join::FlyffEventStep));
        // doc 22 (WP-EVENT-10): the lord system (the lord, the election, the lord events, the skill cooldowns), position 15
        r.register(Box::new(crate::event::lord::join::LordStep));
        // doc 22 (WP-EVENT-04): the descriptions of the running Event.lua events
        r.register(Box::new(crate::event::desc::EventLuaDescStep));
        // doc 22 (WP-EVENT-05): the game rates, after `SmModeAll`
        r.register(Box::new(crate::event::game_rate::GameRateStep));
        // doc 21 (WP-SOCIAL-16): the five siege messages of the login (§7.8), after `GameRate`
        r.register(Box::new(crate::social::siege::login::GuildCombatStep));
        // doc 21 (WP-SOCIAL-17): the guild board and the fighter list while a siege runs, behind the five
        r.register(Box::new(crate::social::siege::fight::WarLoginStep));
        // doc 21 §8 (WP-SOCIAL-26): the 1:1 guild combat state of a member of a tendering guild (flag on only)
        r.register(Box::new(crate::social::gc1to1::login::Gc1to1LoginStep));
        // doc 22 §14 (WP-EVENT-17): the state of a racer while the race runs (nothing for any other joiner)
        r.register(Box::new(crate::event::rainbow::join::RainbowRaceStateStep));
        // doc 22 §12 (WP-EVENT-14): `GuildHouseAllInfo`, only for a member of a guild that owns a house
        r.register(Box::new(crate::event::guild_house::GuildHouseAllInfoStep));
        // doc 22 §13 (WP-EVENT-16): the furniture list and the visitors of a non-empty mini room, behind `GuildHouseAllInfo`
        r.register(Box::new(crate::event::housing::HousingAllInfoStep));
        // doc 22 (WP-EVENT-12): `TaxAllInfo` of every joiner, then the rate window for the master of the next owner
        r.register(Box::new(crate::event::tax::TaxAllInfoStep));
        // doc 22 §11 (WP-EVENT-13, task secret-room): `SecretRoomMngState{1, 0}` of every joiner while the manager is
        // closed, the state of a master or a line-up member otherwise (before `TaxAllInfo` by its `JoinStep`)
        r.register(Box::new(
            crate::event::secret_room::join::SecretRoomInfoStep,
        ));
        r
    }

    /// Adds a contributor (a later one of the same step writes after the earlier one).
    pub fn register(&mut self, c: Box<dyn JoinContributor>) {
        self.list.push(c);
        self.list.sort_by_key(|c| c.step()); // stable: registration order within a step
    }

    /// The registered steps in write order (tests).
    pub fn steps(&self) -> Vec<JoinStep> {
        self.list.iter().map(|c| c.step()).collect()
    }

    /// Writes every step for `cx.session`.
    pub fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        for c in &self.list {
            c.write(cx, out);
        }
    }
}

struct EnvironmentAllStep;
impl JoinContributor for EnvironmentAllStep {
    fn step(&self) -> JoinStep {
        JoinStep::EnvironmentAll
    }
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        out.to_session(
            cx.session,
            0,
            season_msg(&cx.worlds.seasons, &cx.clock.local),
        );
    }
}

struct WorldReadInfoStep;
impl JoinContributor for WorldReadInfoStep {
    fn step(&self) -> JoinStep {
        JoinStep::WorldReadInfo
    }
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        if let Some(c) = cx.worlds.core(cx.player) {
            out.to_session(
                cx.session,
                0,
                pb::WorldReadInfo {
                    world_id: c.world.0,
                    pos: Some(crate::movement::intents::pb_vec(c.pos)),
                },
            );
        }
    }
}

struct SpawnSelfStep;
impl JoinContributor for SpawnSelfStep {
    fn step(&self) -> JoinStep {
        JoinStep::SpawnSelf
    }
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        if let Some(m) = spawn_self(cx.worlds.store(), cx.player, cx.clock.game_ms) {
            out.to_session(cx.session, cx.player.0, m);
        }
    }
}

struct GameTimerStep;
impl JoinContributor for GameTimerStep {
    fn step(&self) -> JoinStep {
        JoinStep::GameTimer
    }
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        out.to_session(
            cx.session,
            0,
            super::clock::game_timer_msg(cx.clock.game_ms),
        );
    }
}

struct EnvironmentStep;
impl JoinContributor for EnvironmentStep {
    fn step(&self) -> JoinStep {
        JoinStep::Environment
    }
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        out.to_session(cx.session, 0, environment_msg(cx.worlds));
    }
}

struct AddGameJoinStep;
impl JoinContributor for AddGameJoinStep {
    fn step(&self) -> JoinStep {
        JoinStep::AddGameJoin
    }
    fn write(&self, cx: &JoinCtx<'_>, out: &mut Outbox) {
        let Some(mut m) = super::spawn::join_add_game_join(cx.worlds.store(), cx.player) else {
            return;
        };
        if cx.cfg.rules.features.welcome_notice && !cx.reattach {
            m.welcome_messages = cx.cfg.rules.gameplay.welcome_messages.clone();
        }
        out.to_session(cx.session, 0, m);
    }
}

// ---- placement ----------------------------------------------------------------------------------------------

/// The saved location of a character (`characters.world_id`, `layer`, `pos`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedLocation {
    /// The saved world id.
    pub world: WorldId,
    /// The saved layer.
    pub layer: LayerId,
    /// The saved position.
    pub pos: Vec3,
}

/// The predicates of doc 22 that the placement consults, with the "feature absent" defaults (15 §9.1 step 3-4).
pub trait JoinPolicy: Send {
    /// `guild_house_login_ok(player, world, layer)`: `true` for every world that is not a guild house.
    fn guild_house_login_ok(&self, _world: WorldId, _layer: LayerId, _c: CharacterId) -> bool {
        true
    }
    /// `CInstanceDungeonHelper::IsInstanceDungeon(world)`.
    fn is_instance_world(&self, _world: WorldId) -> bool {
        false
    }
    /// The character takes part in a secret room.
    fn secret_room_participant(&self, _c: CharacterId) -> bool {
        false
    }
    /// `instance_go_out(player)` for a character saved inside an instance dungeon (called after the spawn).
    fn instance_go_out(&mut self, _svc: &mut Services, _ctx: &mut GameCtx<'_>, _p: PlayerRef) {}
    // ---- guild house (doc 22 §12.2, task guild-house) ----
    /// Doc 22 computes the admission facts that need the services before [`place_join`] (which has none): called by
    /// `WorldRules::join` right before the placement with the character's saved location. The default does nothing.
    fn prepare(
        &mut self,
        _svc: &mut Services,
        _ctx: &mut GameCtx<'_>,
        _c: CharacterId,
        _saved: SavedLocation,
    ) {
    }
}

/// The policy without doc 22: no guild houses, no instances, no secret room.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultJoinPolicy;

impl JoinPolicy for DefaultJoinPolicy {}

/// A teleport requested right after the placement (15 §9.1 step 4), executed at `_replace` of the same tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostTeleport {
    /// The target world.
    pub world: WorldId,
    /// The target position.
    pub pos: Vec3,
}

/// Where a joining character enters the world and what follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The world the `_add` goes to.
    pub world: WorldId,
    /// The layer.
    pub layer: LayerId,
    /// The position.
    pub pos: Vec3,
    /// The teleports queued in this order (the fallback `SetPos` y 99.8 first, then the post-placement ones).
    pub teleports: Vec<PostTeleport>,
    /// The saved world is an instance dungeon: doc 22's `instance_go_out` runs after the spawn.
    pub instance_go_out: bool,
}

/// 15 §9.1 steps 2-4 for a saved location.
pub fn place_join(
    worlds: &Worlds,
    saved: SavedLocation,
    character: CharacterId,
    gc1to1_enabled: bool,
    policy: &dyn JoinPolicy,
) -> Placement {
    let madrigal = WorldId(WI_WORLD_MADRIGAL);
    let mut world = saved.world;
    let mut pos = saved.pos;
    // 2: a saved position outside a loaded world goes to Madrigal; the saved layer is kept
    if let Some(w) = worlds.world(world)
        && !w.statics.in_world(pos.x, pos.z)
    {
        world = madrigal;
        pos = MADRIGAL_START;
    }
    // 3: admission
    let admitted = policy.guild_house_login_ok(world, saved.layer, character)
        && worlds.world(world).is_some()
        && worlds.layer(world, saved.layer).is_some()
        && !policy.is_instance_world(world)
        && world.0 != WI_WORLD_QUIZ;
    let mut teleports = Vec::new();
    let (world, layer, pos) = if admitted {
        (world, saved.layer, pos)
    } else {
        // KEEP-QUIRK: the same-world teleport 0.2 m lower
        teleports.push(PostTeleport {
            world: madrigal,
            pos: Vec3::new(MADRIGAL_START.x, 99.8, MADRIGAL_START.z),
        });
        (madrigal, LayerId(0), MADRIGAL_START)
    };
    // 4: post-placement teleports
    if gc1to1_enabled && (WI_WORLD_GUILDWAR1TO1_0..=WI_WORLD_GUILDWAR1TO1_0 + 3).contains(&world.0)
    {
        teleports.push(PostTeleport {
            world: madrigal,
            pos: super::revival::MADRIGAL_DRIVE_OUT,
        });
    }
    // 22 §11 (task secret-room): the legacy `IsInTheSecretRoom` at join is the world alone (152..=167)
    if policy.secret_room_participant(character) || crate::event::secret_room::is_room_world(world)
    {
        teleports.push(PostTeleport {
            world: madrigal,
            pos: Continents::revival_pos(CONT_FLARIS),
        });
    }
    if world.0 == WI_DUNGEON_SECRET_0 {
        teleports.push(PostTeleport {
            world: madrigal,
            pos: Continents::revival_pos(CONT_DARKON12),
        });
    }
    Placement {
        world,
        layer,
        pos,
        teleports,
        instance_go_out: policy.is_instance_world(saved.world),
    }
}

/// `true` when `GC1to1` is on (`events.guild_combat_1to1`).
pub fn gc1to1_enabled(live: &LiveConfig) -> bool {
    live.event_state(EventId::GuildCombat1to1) != 0
}

/// The statics of a placement's world (for callers that validate positions).
pub fn statics_of(worlds: &Worlds, world: WorldId) -> Option<&std::sync::Arc<WorldStatic>> {
    worlds.world(world).map(|w| &w.statics)
}

// ---- the sequence ---------------------------------------------------------------------------------------------

/// Writes the join batch of `player` into `session` (15 §9.2, phase 1).
pub fn write_join_batch(
    reg: &JoinRegistry,
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    player: ObjId,
    session: SessionId,
    reattach: bool,
) {
    let cx = JoinCtx {
        svc,
        worlds: &*ctx.worlds,
        data: ctx.data,
        clock: ctx.clock,
        live: ctx.live,
        cfg: ctx.cfg,
        player,
        session,
        reattach,
    };
    reg.write(&cx, &mut *ctx.outbox);
}

/// `join::replay_for_reattach` (15 §9.3): the steps of the join sequence from memory, then `resend_view` (the
/// spawn of every object in the player's sets, in the order of the sets), and the session is marked urgent. The
/// server-side sets did not change, so no `add_to_view` runs.
pub fn replay_for_reattach(
    reg: &JoinRegistry,
    svc: &Services,
    ctx: &mut GameCtx<'_>,
    player: ObjId,
    session: SessionId,
) {
    write_join_batch(reg, svc, ctx, player, session, true);
    let mut stats = SpawnCacheStats::default();
    let frame = ctx.clock.tick.frames_completed_after();
    let mut writer = SpawnWriter::new(
        &mut *ctx.outbox,
        &*ctx.sessions,
        &mut stats,
        ctx.clock.game_ms,
        frame,
        ctx.cfg.max_batch_bytes,
    );
    resend_view(ctx.worlds.store(), &mut writer, player);
    ctx.outbox.mark_urgent(session);
}
