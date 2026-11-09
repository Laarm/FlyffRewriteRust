//! Character watches and container snapshots (Plan/25 §3.15 `CharacterWatch`, §3.16 `CharacterDetail` and
//! `ContainerSnapshot`, §4.3-§4.4; types and hooks: WP-ADMIN-06, the watch itself: **WP-ADMIN-16**).
//!
//! # What a watch does
//!
//! `ViewSpec::Character(id)` (the `CharacterWatch` message) follows one character:
//!
//! * [`CharacterDetail`] every 2 s (30 ticks) while the character is online, and at once when the avatar appears or
//!   changes; while it is offline one detail with `online = false` (the directory's name, level and job) and a poll
//!   every second for the login, after which the watch goes live without a new subscription.
//! * [`ContainerSnapshot`]s of the character's containers: equipment, inventory, the three pockets and the banks of
//!   its account (banks are account-scoped, ADM-D8: they are live through any online character of the account, so
//!   the snapshot of a bank carries `live_via_character_id` = the watched character). A snapshot is pushed once when
//!   the watch starts and again whenever the container changed, at most 5 Hz per container (a change within 200 ms of
//!   the previous push is pushed as soon as the interval is over, so the last state always arrives). A purchase in
//!   tick `n` is therefore on the wire in tick `n` (the watch runs in the `post` phase, after the flush).
//! * The watch finds changes by comparing the container with the copy it pushed last (`ItemContainer: Eq`, no
//!   allocation while nothing changed); the item services need no change hook and the watch costs nothing for a
//!   character nobody watches. Every comparison is charged to the visit budget.
//!
//! # How a cell is addressed
//!
//! The admin protocol has no `index` array, so **`ItemElem.obj_id` of an admin snapshot is the display slot** of the
//! cell: the bag grid position of the inventory, the pocket / bank slot, and for the equipment container the
//! equipment *part* (`PARTS_*`, 0..31). `RemoveItem.slot` uses the same number. The capacity of a snapshot is the grid size
//! (bag slots for the inventory, 31 for the equipment, the pocket size, 42 for a bank); an unavailable pocket has
//! capacity 0. Penya: the inventory carries the wallet, a bank its balance.

use super::events::{AdminEvent, SubError, SubErrorCode};
use super::types::ContainerLoc;
use super::views::VisitBudget;
use crate::item::container::{ContainerKind as ItemContainerKind, ItemContainer};
use crate::item::pockets::pocket::is_available;
use crate::mover::Mover;
use crate::player::PlayerExt;
use crate::tick::Game;
use flyff_common::{CharacterId, ObjId};
use flyff_db::model::ContainerRecord;
use flyff_proto::pb;
use flyff_rules::consts::MAX_HUMAN_PARTS;
use flyff_rules::item::buffitem::pocket_available;

/// Ticks between two [`CharacterDetail`] messages of a watched online character (2 s at 15 Hz, 25 §3.16 "Rates").
pub const DETAIL_EVERY_TICKS: u64 = 30;
/// The shortest interval between two pushes of one container (5 Hz, 25 §3.16 "Rates").
pub const CONTAINER_MIN_TICKS: u64 = 3;
/// How often a watch of an offline character looks for its login.
pub const OFFLINE_POLL_TICKS: u64 = 15;

/// One skill of a watched character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillLevel {
    /// Skill id.
    pub skill: u32,
    /// Level.
    pub level: u32,
}

/// One buff of a watched character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuffState {
    /// The legacy buff key `(kind << 16) | id` (`BuffKey::legacy`: kind 0 item, 1 skill, 2 pet, 3 absolute-expiry
    /// item, 4 equipment), so an item buff and a skill buff of the same number stay apart.
    pub buff: u32,
    /// Level.
    pub level: u32,
    /// Remaining time in milliseconds.
    pub remaining_ms: u32,
}

/// One quest of a watched character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuestState {
    /// Quest id.
    pub quest: u32,
    /// State: 0 `QS_BEGIN` .. 13, 14 `QS_END` (a finished quest).
    pub state: u32,
}

/// What `CharacterWatch` pushes (`CharacterDetail`).
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterDetail {
    /// The character.
    pub character_id: u32,
    /// Name.
    pub name: String,
    /// Level.
    pub level: u32,
    /// Job id.
    pub job: u32,
    /// EXP in permille of the level.
    pub exp_permille: u32,
    /// Strength.
    pub str: u32,
    /// Stamina.
    pub sta: u32,
    /// Dexterity.
    pub dex: u32,
    /// Intelligence.
    pub int: u32,
    /// Free stat points.
    pub stat_points: u32,
    /// Free skill points.
    pub skill_points: u32,
    /// Hit points.
    pub hp: u32,
    /// Maximum hit points.
    pub hp_max: u32,
    /// Mana points.
    pub mp: u32,
    /// Maximum mana points.
    pub mp_max: u32,
    /// Fatigue points.
    pub fp: u32,
    /// Maximum fatigue points.
    pub fp_max: u32,
    /// The skills.
    pub skills: Vec<SkillLevel>,
    /// The buffs.
    pub buffs: Vec<BuffState>,
    /// The quests: the active ones with their state, the finished ones with state 14.
    pub quests: Vec<QuestState>,
    /// `m_dwMode`.
    pub mode_flags: u32,
    /// Unix seconds the mute ends; `0` = not muted.
    pub mute_until: u64,
    /// World id.
    pub world: u32,
    /// Layer id.
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position z.
    pub z: f32,
    /// Guild id, `0` = none.
    pub guild_id: u32,
    /// Party id, `0` = none.
    pub party_id: u32,
    /// `false`: the values are the saved ones of an offline character.
    pub online: bool,
}

/// Which container (`ContainerKind` of the protocol).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ContainerKind {
    /// Worn items.
    Equipment = 1,
    /// The bag.
    Inventory = 2,
    /// A pocket.
    Pocket = 3,
    /// A bank slot.
    Bank = 4,
    /// A guild bank.
    GuildBank = 5,
    /// The mailbox.
    Mailbox = 6,
}

/// The identity of a container in a snapshot (`ContainerRef`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContainerRef {
    /// Which kind.
    pub kind: ContainerKind,
    /// Pocket `0..3`, bank slot `0..3`.
    pub index: u32,
    /// The character the container belongs to (the guild bank: the guild id).
    pub owner_character_id: u32,
}

impl ContainerRef {
    /// The key of the latest-wins slot of this container inside its subscription.
    pub fn slot_key(&self) -> u64 {
        (u64::from(self.kind as u8) << 56)
            | ((u64::from(self.index) & 0xFF_FFFF) << 32)
            | u64::from(self.owner_character_id)
    }
}

/// What a container watch pushes (`ContainerSnapshot`).
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerSnapshot {
    /// Which container.
    pub container: ContainerRef,
    /// Revision: grows with every change.
    pub rev: u32,
    /// Slots.
    pub capacity: u32,
    /// The items (the wire item type; `obj_id` is the display slot, see the module documentation).
    pub items: Vec<pb::ItemElem>,
    /// The penya of the container (bank, inventory).
    pub gold: u64,
    /// `true`: live from the game; `false`: the saved state of an offline character.
    pub live: bool,
    /// Unix seconds of the save an offline snapshot shows.
    pub saved_at: u64,
    /// The online sibling through which an account-scoped bank is live; `0` = none.
    pub live_via_character_id: u32,
}

// ---- the state of a watch ----------------------------------------------------------------------------------------

/// The number of containers of one watch: equipment, inventory, 3 pockets, 3 banks.
const SLOTS: usize = 8;

/// What the watch pushed last for one container.
#[derive(Clone, Debug, Default)]
struct Sent {
    /// A snapshot went out at least once since the avatar appeared.
    sent: bool,
    items: Vec<pb::ItemElem>,
    gold: u64,
    capacity: u32,
    owner: u32,
    rev: u32,
    tick: u64,
    /// The container changed since the last push (the push may wait for the 5 Hz interval).
    dirty: bool,
}

/// The state of one character watch.
#[derive(Clone, Debug)]
pub struct CharacterSub {
    /// The watched character.
    pub character: CharacterId,
    /// The tick the next refresh is due.
    pub due_tick: u64,
    /// The avatar the state below belongs to; a different one (a relog) restarts the watch.
    avatar: Option<ObjId>,
    /// The tick the next [`CharacterDetail`] is due.
    detail_due: u64,
    /// The offline detail went out.
    offline_sent: bool,
    /// Copies of the containers as of the last comparison.
    inv: Option<(ItemContainer, i32)>,
    pockets: [Option<ItemContainer>; 3],
    banks: [Option<(ItemContainer, i64)>; 3],
    /// What the client has, by container index (equipment, inventory, pockets, banks).
    sent: [Sent; SLOTS],
}

impl CharacterSub {
    fn new(character: CharacterId, tick: u64) -> CharacterSub {
        CharacterSub {
            character,
            due_tick: tick,
            avatar: None,
            detail_due: tick,
            offline_sent: false,
            inv: None,
            pockets: [None, None, None],
            banks: [None, None, None],
            sent: Default::default(),
        }
    }

    /// Forgets what the client has: the next collect pushes everything again.
    fn restart(&mut self, tick: u64) {
        let character = self.character;
        *self = CharacterSub::new(character, tick);
    }
}

// ---- helpers shared with the queries -----------------------------------------------------------------------------

/// The avatar of `character` when it is in the world (a session entry with a mover).
pub fn online_avatar(g: &Game, character: CharacterId, hint: Option<ObjId>) -> Option<ObjId> {
    if let Some(o) = hint
        && g.sessions.get(o).is_some_and(|e| e.character == character)
        && g.worlds.player(o).is_some()
    {
        return Some(o);
    }
    g.sessions
        .iter()
        .find(|e| e.character == character && g.worlds.player(e.player).is_some())
        .map(|e| e.player)
}

/// `true` when the game knows the character (the directory has it, or it is in the world).
fn known(g: &Game, character: CharacterId) -> bool {
    g.services.presence.get(character).is_some() || online_avatar(g, character, None).is_some()
}

fn clamp_u32(v: i32) -> u32 {
    u32::try_from(v.max(0)).unwrap_or(0)
}

/// The items of the cells of one region of a container as the console shows them: `obj_id` is the display slot
/// (for the equipment region the part).
pub(crate) fn region_items(c: &ItemContainer, region: Region) -> Vec<pb::ItemElem> {
    let (from, to) = match region {
        Region::Bag => (0, c.index_num()),
        Region::Equipment => (c.index_num(), c.item_max()),
    };
    (from..to)
        .filter_map(|slot| {
            let storage = c.obj_at_slot(slot)?;
            let item = c.get_at_id(storage)?;
            Some(item.to_pb(slot - from))
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Region {
    Bag,
    Equipment,
}

pub(crate) fn capacity_of(c: &ItemContainer, region: Region) -> u32 {
    match region {
        Region::Bag => u32::from(c.index_num()),
        Region::Equipment => u32::from(c.item_max().saturating_sub(c.index_num())),
    }
}

/// The index of a container in a watch (and the order of the first push).
fn slot_of(loc: ContainerLoc) -> Option<usize> {
    Some(match loc {
        ContainerLoc::Equipment => 0,
        ContainerLoc::Inventory => 1,
        ContainerLoc::Pocket(i) if i < 3 => 2 + usize::from(i),
        ContainerLoc::Bank { slot } if slot < 3 => 5 + usize::from(slot),
        _ => return None,
    })
}

fn loc_of(slot: usize) -> ContainerLoc {
    match slot {
        0 => ContainerLoc::Equipment,
        1 => ContainerLoc::Inventory,
        2..=4 => ContainerLoc::Pocket(u8::try_from(slot - 2).unwrap_or(0)),
        _ => ContainerLoc::Bank {
            slot: u8::try_from(slot - 5).unwrap_or(0),
        },
    }
}

/// One live container of a player as a snapshot (`rev` 0). `via` is the character that serves an account-scoped
/// bank (the watched / queried character). `None` for a container the player does not have (a bank slot without a
/// character, a guild bank, the mailbox).
pub fn live_snapshot(
    g: &Game,
    p: &PlayerExt,
    loc: ContainerLoc,
    via: CharacterId,
) -> Option<ContainerSnapshot> {
    let own = p.ident.character_id.0;
    let items = &p.items;
    let (container, items_pb, capacity, gold, live_via) = match loc {
        ContainerLoc::Equipment => (
            ContainerRef {
                kind: ContainerKind::Equipment,
                index: 0,
                owner_character_id: own,
            },
            region_items(&items.inventory, Region::Equipment),
            capacity_of(&items.inventory, Region::Equipment),
            0,
            0,
        ),
        ContainerLoc::Inventory => (
            ContainerRef {
                kind: ContainerKind::Inventory,
                index: 0,
                owner_character_id: own,
            },
            region_items(&items.inventory, Region::Bag),
            capacity_of(&items.inventory, Region::Bag),
            u64::try_from(items.gold.max(0)).unwrap_or(0),
            0,
        ),
        ContainerLoc::Pocket(i) => {
            let pk = items.pockets.get(usize::from(i))?;
            let cfg = g.services.items.handle().cfg;
            let capacity = if is_available(&cfg, usize::from(i), pk) {
                capacity_of(&pk.items, Region::Bag)
            } else {
                0
            };
            let shown = if capacity == 0 {
                Vec::new()
            } else {
                region_items(&pk.items, Region::Bag)
            };
            (
                ContainerRef {
                    kind: ContainerKind::Pocket,
                    index: u32::from(i),
                    owner_character_id: own,
                },
                shown,
                capacity,
                0,
                0,
            )
        }
        ContainerLoc::Bank { slot } => {
            let b = items.bank(usize::from(slot))?;
            (
                ContainerRef {
                    kind: ContainerKind::Bank,
                    index: u32::from(slot),
                    owner_character_id: b.character_id.0,
                },
                region_items(&b.items, Region::Bag),
                capacity_of(&b.items, Region::Bag),
                u64::try_from(b.gold.max(0)).unwrap_or(0),
                via.0,
            )
        }
        ContainerLoc::GuildBank(_) | ContainerLoc::Mailbox => return None,
    };
    Some(ContainerSnapshot {
        container,
        rev: 0,
        capacity,
        items: items_pb,
        gold,
        live: true,
        saved_at: 0,
        live_via_character_id: live_via,
    })
}

/// The live state of an online character as the console's detail message.
pub fn detail_of(g: &Game, m: &Mover) -> Option<CharacterDetail> {
    let p = m.player()?;
    let stats = g.services.stats.core();
    let now = g.clock.game_ms;
    let unix = g.clock.unix_ms.to_unix_time().0;
    let level = clamp_u32(m.mv.level);
    let exp_permille = u32::try_from(
        flyff_rules::progression::exp_percent_x100(&stats.rules, m.mv.level, p.progression.exp)
            / 10,
    )
    .unwrap_or(0);
    let skills = p
        .progression
        .job_skills
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| SkillLevel {
            skill: s.skill_id,
            level: s.level,
        })
        .collect();
    let buffs = m
        .buffs
        .iter()
        .filter(|(_, e)| !e.removed)
        .map(|(k, e)| BuffState {
            buff: k.legacy(),
            level: e.level,
            remaining_ms: e.remain_ms(now),
        })
        .collect();
    let mut quests: Vec<QuestState> = p
        .quests
        .doc
        .active
        .iter()
        .map(|q| QuestState {
            quest: u32::from(q.quest_id),
            state: u32::from(q.state),
        })
        .collect();
    quests.extend(p.quests.doc.completed.iter().map(|q| QuestState {
        quest: u32::from(*q),
        state: u32::from(crate::quest::QS_END),
    }));
    let mute = i64::from(p.misc.mute_remaining_s);
    Some(CharacterDetail {
        character_id: p.ident.character_id.0,
        name: p.ident.name.clone(),
        level,
        job: clamp_u32(m.mv.job),
        exp_permille,
        str: clamp_u32(m.mv.base.str),
        sta: clamp_u32(m.mv.base.sta),
        dex: clamp_u32(m.mv.base.dex),
        int: clamp_u32(m.mv.base.int),
        stat_points: clamp_u32(p.progression.stat_points),
        skill_points: clamp_u32(p.progression.skill_points),
        hp: clamp_u32(m.mv.hp),
        hp_max: clamp_u32(crate::stats::env::max_hp(m, stats)),
        mp: clamp_u32(m.mv.mp),
        mp_max: clamp_u32(crate::stats::env::max_mp(m, stats)),
        fp: clamp_u32(m.mv.fp),
        fp_max: clamp_u32(crate::stats::env::max_fp(m, stats)),
        skills,
        buffs,
        quests,
        mode_flags: m.mv.mode,
        mute_until: if mute > 0 {
            u64::try_from(unix.saturating_add(mute)).unwrap_or(0)
        } else {
            0
        },
        world: m.core.world.0,
        layer: m.core.layer.0,
        x: m.core.pos.x,
        z: m.core.pos.z,
        guild_id: p.social.guild.map_or(0, |gid| gid.0),
        party_id: p.social.party.map_or(0, |pid| pid.0),
        online: true,
    })
}

/// What the game knows of a character that is not in the world (the directory): name, level and job, `online = false`.
pub fn offline_detail(g: &Game, character: CharacterId) -> Option<CharacterDetail> {
    let e = g.services.presence.get(character)?;
    Some(CharacterDetail {
        character_id: character.0,
        name: e.name.clone(),
        level: u32::from(e.level),
        job: u32::from(e.job),
        exp_permille: 0,
        str: 0,
        sta: 0,
        dex: 0,
        int: 0,
        stat_points: 0,
        skill_points: 0,
        hp: 0,
        hp_max: 0,
        mp: 0,
        mp_max: 0,
        fp: 0,
        fp_max: 0,
        skills: Vec::new(),
        buffs: Vec::new(),
        quests: Vec::new(),
        mode_flags: 0,
        mute_until: 0,
        world: 0,
        layer: 0,
        x: 0.0,
        z: 0.0,
        guild_id: 0,
        party_id: 0,
        online: false,
    })
}

// ---- saved containers (offline characters) -------------------------------------------------------------------------

/// The equipment and inventory snapshots of a saved inventory document (`live = false`). `bag_slots` follows the
/// record's capacity (`capacity - 31`).
pub fn saved_inventory(
    owner: CharacterId,
    record: &ContainerRecord,
    gold: i64,
    saved_at: u64,
) -> [ContainerSnapshot; 2] {
    let extra = u16::try_from(MAX_HUMAN_PARTS).unwrap_or(31);
    let index_num = record.capacity.saturating_sub(extra);
    let (c, _) = ItemContainer::from_record(record, ItemContainerKind::Inventory, index_num, extra);
    let mk = |kind, region| ContainerSnapshot {
        container: ContainerRef {
            kind,
            index: 0,
            owner_character_id: owner.0,
        },
        rev: 0,
        capacity: capacity_of(&c, region),
        items: region_items(&c, region),
        gold: if region == Region::Bag {
            u64::try_from(gold.max(0)).unwrap_or(0)
        } else {
            0
        },
        live: false,
        saved_at,
        live_via_character_id: 0,
    };
    [
        mk(ContainerKind::Equipment, Region::Equipment),
        mk(ContainerKind::Inventory, Region::Bag),
    ]
}

/// The snapshot of a saved pocket. `available` is the pocket's availability under the configuration
/// (`features.perm_extrabags`); an unavailable pocket shows capacity 0.
pub fn saved_pocket(
    owner: CharacterId,
    index: u8,
    record: &ContainerRecord,
    available: bool,
    saved_at: u64,
) -> ContainerSnapshot {
    let (c, _) =
        ItemContainer::from_record(record, ItemContainerKind::Pocket(index), record.capacity, 0);
    ContainerSnapshot {
        container: ContainerRef {
            kind: ContainerKind::Pocket,
            index: u32::from(index),
            owner_character_id: owner.0,
        },
        rev: 0,
        capacity: if available {
            capacity_of(&c, Region::Bag)
        } else {
            0
        },
        items: if available {
            region_items(&c, Region::Bag)
        } else {
            Vec::new()
        },
        gold: 0,
        live: false,
        saved_at,
        live_via_character_id: 0,
    }
}

/// `true` when pocket `index` can be used: the availability rule of `features.perm_extrabags` (always) or the
/// stored flags of the pocket.
pub fn pocket_usable(perm_extrabags: bool, index: usize, available: bool, expired: bool) -> bool {
    pocket_available(perm_extrabags, index, available, expired)
}

/// The snapshot of a saved bank (`slot` is the owner's character slot).
pub fn saved_bank(
    owner: CharacterId,
    slot: u8,
    record: &ContainerRecord,
    gold: i64,
    saved_at: u64,
) -> ContainerSnapshot {
    let capacity = if record.capacity == 0 {
        crate::item::inventory::BANK_SLOTS
    } else {
        record.capacity
    };
    let (c, _) = ItemContainer::from_record(record, ItemContainerKind::Bank, capacity, 0);
    ContainerSnapshot {
        container: ContainerRef {
            kind: ContainerKind::Bank,
            index: u32::from(slot),
            owner_character_id: owner.0,
        },
        rev: 0,
        capacity: capacity_of(&c, Region::Bag),
        items: region_items(&c, Region::Bag),
        gold: u64::try_from(gold.max(0)).unwrap_or(0),
        live: false,
        saved_at,
        live_via_character_id: 0,
    }
}

// ---- the hooks of the registry -----------------------------------------------------------------------------------

/// Creates a character watch (hook of `AdminViewOp::Open`). An unknown character id is `NOT_FOUND`; a known one
/// that is offline is fine (the watch goes live at its login).
pub fn open(g: &Game, character: CharacterId, tick: u64) -> Result<CharacterSub, SubError> {
    if !known(g, character) {
        return Err(SubError::new(
            SubErrorCode::NotFound,
            format!("character {character} does not exist"),
        ));
    }
    Ok(CharacterSub::new(character, tick))
}

/// Re-targets a character watch (hook of `AdminViewOp::Update`): the client has nothing of the new character yet.
pub fn update(
    g: &Game,
    sub: &mut CharacterSub,
    character: CharacterId,
    tick: u64,
) -> Result<(), SubError> {
    if !known(g, character) {
        return Err(SubError::new(
            SubErrorCode::NotFound,
            format!("character {character} does not exist"),
        ));
    }
    *sub = CharacterSub::new(character, tick);
    Ok(())
}

/// Serves a watch (called from the `post` phase every tick; see the module documentation). Returns the events to
/// push for the subscription.
pub fn collect(
    sub: &mut CharacterSub,
    g: &Game,
    tick: u64,
    budget: &mut VisitBudget,
) -> Vec<AdminEvent> {
    sub.due_tick = tick + 1;
    budget.entities(1);
    let mut out = Vec::new();
    let Some(avatar) = online_avatar(g, sub.character, sub.avatar) else {
        // offline (or purged): one detail, then a poll for the login
        if sub.avatar.is_some() {
            sub.restart(tick);
        }
        if !sub.offline_sent {
            sub.offline_sent = true;
            match offline_detail(g, sub.character) {
                Some(d) => out.push(AdminEvent::Character(Box::new(d))),
                None => {
                    out.push(AdminEvent::Error(SubError::new(
                        SubErrorCode::NotFound,
                        format!("character {} no longer exists", sub.character),
                    )));
                }
            }
        }
        sub.due_tick = tick + OFFLINE_POLL_TICKS;
        return out;
    };
    let Some(m) = g.worlds.player(avatar) else {
        sub.due_tick = tick + OFFLINE_POLL_TICKS;
        return out;
    };
    let Some(p) = m.player() else {
        sub.due_tick = tick + OFFLINE_POLL_TICKS;
        return out;
    };
    if sub.avatar != Some(avatar) {
        // the avatar appeared (a login, a relog): the client gets everything again
        sub.restart(tick);
        sub.avatar = Some(avatar);
    }
    if tick >= sub.detail_due {
        sub.detail_due = tick + DETAIL_EVERY_TICKS;
        budget.entities(4);
        if let Some(d) = detail_of(g, m) {
            out.push(AdminEvent::Character(Box::new(d)));
        }
    }
    collect_containers(sub, g, p, tick, budget, &mut out);
    out
}

/// The containers of the watch: detect changes, push what is due.
fn collect_containers(
    sub: &mut CharacterSub,
    g: &Game,
    p: &PlayerExt,
    tick: u64,
    budget: &mut VisitBudget,
    out: &mut Vec<AdminEvent>,
) {
    let items = &p.items;
    // 1. change detection against the copies of the last comparison (equality, no allocation)
    if sub.inv.as_ref().map(|(c, gold)| (c, *gold)) != Some((&items.inventory, items.gold)) {
        sub.inv = Some((items.inventory.clone(), items.gold));
        sub.sent[0].dirty = true;
        sub.sent[1].dirty = true;
    }
    for i in 0..3 {
        let now = items.pockets.get(i).map(|pk| &pk.items);
        if sub.pockets[i].as_ref() != now {
            sub.pockets[i] = now.cloned();
            sub.sent[2 + i].dirty = true;
        }
        let bank = items.bank(i).map(|b| (&b.items, b.gold));
        if sub.banks[i].as_ref().map(|(c, gold)| (c, *gold)) != bank {
            sub.banks[i] = bank.map(|(c, gold)| (c.clone(), gold));
            sub.sent[5 + i].dirty = true;
        }
    }
    budget.entities(2);
    // 2. push the dirty ones whose interval is over (a container never sent goes out at once)
    for slot in 0..SLOTS {
        let state = &sub.sent[slot];
        let never = !state.sent;
        if !state.dirty && !never {
            continue;
        }
        if !never && tick.saturating_sub(state.tick) < CONTAINER_MIN_TICKS {
            continue;
        }
        let loc = loc_of(slot);
        let Some(mut snap) = live_snapshot(g, p, loc, p.ident.character_id) else {
            // a bank slot without a character
            sub.sent[slot].dirty = false;
            sub.sent[slot].sent = true;
            continue;
        };
        budget.entities(1);
        let state = &mut sub.sent[slot];
        state.dirty = false;
        let same = state.sent
            && state.gold == snap.gold
            && state.capacity == snap.capacity
            && state.owner == snap.container.owner_character_id
            && state.items == snap.items;
        if same {
            continue;
        }
        state.rev = state.rev.wrapping_add(1);
        snap.rev = state.rev;
        state.sent = true;
        state.tick = tick;
        state.gold = snap.gold;
        state.capacity = snap.capacity;
        state.owner = snap.container.owner_character_id;
        state.items.clone_from(&snap.items);
        out.push(AdminEvent::Container(Box::new(snap)));
    }
}

/// Used by the queries: the avatar's player extension.
pub(crate) fn player_of(g: &Game, character: CharacterId) -> Option<(ObjId, &Mover)> {
    let obj = online_avatar(g, character, None)?;
    let m = g.worlds.player(obj)?;
    m.player()?;
    Some((obj, m))
}

/// The container a query asks for, for the online character `character`, through which `via` serves the banks.
pub(crate) fn live_container(
    g: &Game,
    character: CharacterId,
    loc: ContainerLoc,
) -> Option<ContainerSnapshot> {
    let _ = slot_of(loc)?;
    let (_, m) = player_of(g, character)?;
    live_snapshot(g, m.player()?, loc, character)
}
