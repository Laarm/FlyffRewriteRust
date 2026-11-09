//! Wire mapping of a mover (Plan/16 §3.7, ref/08 §7.5): the field builders of the spawn messages.
//!
//! `mover_common`, `player_public`, `player_self`, `player_other` and `npc_part` produce the message parts
//! whose fields this document owns; doc 15's `world::spawn` assembles `SpawnSelf` / `SpawnPlayer` /
//! `SpawnNpc` from them. Fields whose owners land later (vendor title, cheer, murderer, duel, casting item,
//! vis-pet effect) are written with their documented defaults (15 §7.2).
//!
//! The functions read the mover; nothing here mutates game state.

use super::{Mover, MoverKind};
use crate::item::wire as item_wire;
use crate::player::PlayerExt;
use flyff_common::GameMs;
use flyff_common::math::Vec3;
use flyff_db::ContainerRecord;
use flyff_proto::pb;
use flyff_rules::consts::{MAX_ADJPARAMARY, MAX_HUMAN_PARTS, NULL_CHGPARAM, NULL_ID, SM_MAX};

/// Banks of the account carried by `PlayerSelf` (`m_Bank[3]`).
const BANK_SLOTS: usize = 3;
/// Slots of a bank container.
const BANK_CAPACITY: u16 = 42;
/// Entries of `honorTitle[]` (`MAX_HONOR_TITLE`).
const HONOR_TITLES: usize = 150;

fn vec3(v: Vec3) -> pb::Vec3 {
    pb::Vec3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

/// `CObj::Serialize` (ref/08 §7.2): the entry header and common prefix of every object. `scale` and `angle`
/// are truncated like the legacy casts (`(u16)(scale * 100)`, `(i16)(angle * 10)`).
pub fn obj_common(core: &crate::world::ObjCore) -> pb::ObjCommon {
    pb::ObjCommon {
        obj_type: u32::from(core.kind.ot()),
        index: core.index,
        scale_x100: (core.scale * 100.0) as u16 as u32,
        pos: Some(vec3(core.pos)),
        angle_x10: i32::from((core.angle * 10.0) as i16),
    }
}

/// The DST snapshot of a mover (16 §4.7): every stored dst with `adj != 0` or `chg != NULL_CHGPARAM`, in
/// ascending dst order. Pools are never stored (16 §4.6).
pub fn dst_snapshot(m: &Mover) -> Vec<pb::DstValue> {
    let t = &m.mv.dst.table;
    (0..MAX_ADJPARAMARY as u32)
        .filter_map(|d| {
            let (adj, chg) = (t.adj(d), t.chg(d));
            (adj != 0 || chg != NULL_CHGPARAM).then_some(pb::DstValue { dst: d, adj, chg })
        })
        .collect()
}

/// `MoverCommon` (ref/08 §7.5.1): motion, player flag, raw HP, state words, belligerence, the vis-pet effect
/// (doc 19, 0 here) and the DST snapshot.
pub fn mover_common(m: &Mover) -> pb::MoverCommon {
    let belligerence = match &m.kind {
        MoverKind::Monster(x) => x.belligerence,
        MoverKind::Npc(x) => x.belligerence,
        MoverKind::Player(_) => 0,
    };
    pb::MoverCommon {
        // `(u_short)m_dwMotion`; a mover whose motion was never selected (`-1`) shows `MTI_STAND`, which the
        // legacy `CreateObj` selected at creation
        motion: if m.sim.motion.id == u32::MAX {
            flyff_sim::types::MTI_STAND
        } else {
            u32::from(m.sim.motion.id as u16)
        },
        is_player: m.is_player(),
        hp: m.mv.hp,
        state: m.sim.act.state.bits(),
        state_flag: m.sim.act.flags.bits(),
        belligerence: belligerence & 0xFF,
        // the effect of a vis pet (`m_dwMoverSfxId`, ITEM-18)
        mover_sfx_id: crate::ai::pet::sfx_of(m),
        dst_params: dst_snapshot(m),
    }
}

/// `PlayerPublic` (ref/08 §7.5.2): name, look, ids, base stats (not `GetStr()`), guild / party ids, modes, PK
/// data, fame, honor, the 31 equip options, the 26 SM times and the hide flags. `now` is the game clock for
/// the remaining pink time. `None` for a non-player.
pub fn player_public(m: &Mover, now: GameMs) -> Option<pb::PlayerPublic> {
    let p = m.player()?;
    let pink_ms = m.combat.pk.pink_until.map_or(0, |t| {
        u32::try_from(t.0.saturating_sub(now.0)).unwrap_or(u32::MAX)
    });
    Some(pb::PlayerPublic {
        name: p.ident.name.clone(),
        sex: u32::from(p.appearance.sex),
        skin_set: u32::from(p.appearance.skin_set),
        hair_mesh: u32::from(p.appearance.hair_mesh),
        hair_color: p.appearance.hair_color,
        head_mesh: u32::from(p.appearance.head_mesh),
        id_player: p.ident.character_id.0,
        job: m.mv.job.clamp(0, 255) as u32,
        str: m.mv.base.str.clamp(0, 0xFFFF) as u32,
        sta: m.mv.base.sta.clamp(0, 0xFFFF) as u32,
        dex: m.mv.base.dex.clamp(0, 0xFFFF) as u32,
        int: m.mv.base.int.clamp(0, 0xFFFF) as u32,
        level: m.mv.level.clamp(0, 0xFFFF) as u32,
        fuel: m.sim.fuel,
        acc_fuel: m.sim.tm_acc_fuel,
        id_guild: p.social.guild.map_or(0, |g| g.0),
        // the legacy sends the war id only with a guild
        id_war: if p.social.guild.is_some() {
            p.social.war
        } else {
            0
        },
        id_guild_cloak: p.social.guild_cloak,
        id_party: p.social.party.map_or(0, |x| x.0),
        id_duel_party: if p.social.party.is_some() {
            p.social.duel_party
        } else {
            0
        },
        // `(char)m_dwAuthorization`
        authorization: i32::from(p.ident.authority.0 as i8),
        mode: m.mv.mode,
        state_mode: m.mv.state_mode,
        use_item_prop_id: 0,
        pk_time_ms: pink_ms,
        pk_value: p.pk.value,
        pk_propensity: p.pk.propensity,
        pk_exp: p.pk.pk_exp,
        fame: p.progression.fame,
        duel: u32::from(m.combat.duel.duel),
        honor: p.social.honor.unwrap_or(-1),
        equip_option: m.mv.equip_info.iter().map(|e| e.option).collect(),
        guild_combat_state: p.social.guild_combat_state,
        // the stored values are already the normalised ones of 11 §5.8 (absolute kinds hold unix seconds)
        sm_time: p
            .sm
            .timers
            .iter()
            .take(SM_MAX)
            .map(|&t| i64::from(t))
            .collect(),
        hide_coat: p.appearance.hide_coat.to_vec(),
    })
}

/// `PlayerOther` (ref/08 §7.5.4): the occupied equipment parts and the summoned pet, for every other
/// player, and the title of the open personal shop (doc 19 §9, empty without one).
pub fn player_other(m: &Mover) -> Option<pb::PlayerOther> {
    let p = m.player()?;
    let (pet_id, pet_name) = active_pet(p);
    Some(pb::PlayerOther {
        vendor_title: p.econ.vendor.title.clone().unwrap_or_default(),
        equip: equip_visuals(m, true),
        pet_id,
        pet_name,
    })
}

/// The occupied parts of `equip_info` as `{part, item_id, flag}` (players) or `{part, item_id}` (NPCs: the
/// flag byte of the NPC variant is not on the wire, so it is 0).
fn equip_visuals(m: &Mover, with_flag: bool) -> Vec<pb::EquipVisual> {
    m.mv.equip_info
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.is_empty())
        .map(|(part, e)| pb::EquipVisual {
            part: part as u32,
            item_id: e.item_id,
            flag: if with_flag { u32::from(e.flag) } else { 0 },
        })
        .collect()
}

/// `MAKELONG(kind, level)` and the name of the summoned pet item, `NULL_ID` and "" without one.
fn active_pet(p: &PlayerExt) -> (u32, String) {
    let pet = p
        .items
        .pet_obj_id
        .and_then(|id| p.items.inventory.get_at_id(id))
        .and_then(|it| it.pet.as_deref());
    match pet {
        Some(pet) => (
            u32::from(pet.kind) | ((pet.level as u32) << 16),
            pet.name.clone(),
        ),
        None => (NULL_ID, String::new()),
    }
}

fn bank_container(p: &PlayerExt, slot: usize) -> pb::ItemContainer {
    match p.items.bank(slot) {
        Some(b) => b.items.to_pb(),
        None => item_wire::container(&ContainerRecord::empty(BANK_CAPACITY, BANK_CAPACITY)),
    }
}

/// `PlayerSelf` (ref/08 §7.5.3): the receiver's own private state. MP and FP are the raw stored pools
/// widened to 32 bit (11 §5.3). Doc 19/20/21 fields use their stand-ins' values (inventory, banks, pockets,
/// quests, honor); the murderer id is 0 until its owner lands. The cheer fields are the points and the rest to
/// the next one (`tick_cheer - now`, 0 when the timer is not running or overdue; `social::cheer::on_join` starts
/// it before the join batch). `None` for a non-player.
pub fn player_self(m: &Mover, now: GameMs) -> Option<pb::PlayerSelf> {
    let p = m.player()?;
    let honor_progress: Vec<i32> = (0..HONOR_TITLES as u16)
        .map(|id| p.social.honor_doc.get(id))
        .collect();
    let quests = &p.quests.doc;
    let pet_slot = p.items.pet_obj_id.map_or(NULL_ID, u32::from);
    Some(pb::PlayerSelf {
        mp: m.mv.mp,
        fp: m.mv.fp,
        tutorial_state: p.progression.tutorial_state,
        fxp: p.progression.flight_exp,
        gold: u32::try_from(p.items.gold).unwrap_or(0),
        exp: p.progression.exp,
        skill_level: p.progression.skill_level_total,
        skill_point: p.progression.skill_points,
        death_exp: p.progression.death_exp,
        death_level: p.progression.death_level,
        marking_world: p.misc.marking_world.0,
        marking_pos: Some(vec3(p.misc.marking_pos)),
        // one builder for the bit-15 rule (the timer is frozen): `SetQuest` uses it too
        quests: quests
            .active
            .iter()
            .map(crate::quest::msg::quest_msg)
            .collect(),
        complete_quests: quests.completed.iter().map(|&q| u32::from(q)).collect(),
        checked_quests: quests.checked.iter().map(|&q| u32::from(q)).collect(),
        id_murderer: 0,
        remain_gp: p.progression.stat_points,
        equip_item_ids: m.mv.equip_info.iter().map(|e| e.item_id).collect(),
        job_skills: p
            .progression
            .job_skills
            .iter()
            .map(|s| pb::Skill {
                skill_id: s.skill_id,
                level: s.level,
            })
            .collect(),
        cheer_point: u32::try_from(p.social.cheer_points).unwrap_or(0),
        cheer_rest_ms: if p.social.tick_cheer.0 == 0 {
            0
        } else {
            u32::try_from(p.social.tick_cheer.0.saturating_sub(now.0)).unwrap_or(u32::MAX)
        },
        slot: u32::from(p.ident.slot),
        gold_bank: (0..BANK_SLOTS)
            .map(|s| {
                p.items
                    .bank(s)
                    .map_or(0, |b| u32::try_from(b.gold.max(0)).unwrap_or(u32::MAX))
            })
            .collect(),
        id_player_bank: (0..BANK_SLOTS)
            .map(|s| p.items.bank(s).map_or(0, |b| b.character_id.0))
            .collect(),
        plus_max_hp: p.sm.plus_max_hp,
        attack_resist_left: u32::from(p.sm.attack_resist_left),
        attack_resist_right: u32::from(p.sm.attack_resist_right),
        defense_resist: u32::from(p.sm.defense_resist),
        angel_exp: p.progression.angel_exp,
        angel_level: p.progression.angel_level,
        inventory: Some(p.items.inventory.to_pb()),
        banks: (0..BANK_SLOTS).map(|s| bank_container(p, s)).collect(),
        pet_id: pet_slot,
        pockets: Some(item_wire::pocket_set_runtime(&p.items)),
        mute: p.misc.mute_remaining_s,
        honor_progress,
        id_campus: p.social.campus,
        campus_point: p.social.campus_point,
    })
}

/// `NpcPart` (ref/08 §7.5.5): the figure, the character key, the worn parts of humanoid NPCs, the aggressive
/// flag and the move pattern state. `None` for a player.
pub fn npc_part(m: &Mover) -> Option<pb::NpcPart> {
    let (hair_mesh, hair_color, head_mesh, key) = match &m.kind {
        MoverKind::Npc(n) => (
            n.hair_mesh,
            n.hair_color,
            n.head_mesh,
            n.character_key.clone(),
        ),
        MoverKind::Monster(_) => (0, 0, 0, String::new()),
        MoverKind::Player(_) => return None,
    };
    let active_attack = m.monster().is_some_and(|x| x.active_attack);
    Some(pb::NpcPart {
        hair_mesh,
        hair_color,
        head_mesh,
        character_key: key,
        equip: equip_visuals(m, false),
        active_attack,
        move_pattern: (m.sim.pattern.pattern & 0xFF) as u32,
        move_event: (m.sim.pattern.event & 0xFF) as u32,
        move_event_cnt: m.sim.pattern.cnt,
        speed_factor: m.sim.params.speed_factor,
    })
}

/// `CBuffMgr::Serialize` (ref/08 §7.5.6): the live buffs with their remaining time at `now`; `BUFF_ITEM2`
/// carries its absolute expiry instead of a level (11 §7.3).
pub fn spawn_buffs(m: &Mover, now: GameMs) -> Vec<pb::Buff> {
    m.buffs
        .iter()
        .filter(|(_, e)| !e.removed)
        .map(|(key, e)| {
            let item2 = key.kind == crate::buff::BuffKind::Item2;
            pb::Buff {
                r#type: u32::from(key.kind.as_u8()),
                id: u32::from(key.id),
                level: if item2 { 0 } else { e.level },
                remain_ms: e.snapshot_ms(now),
                expire_at: if item2 { i64::from(e.level) } else { 0 },
            }
        })
        .collect()
}

/// `AddGameJoin{saved_play_time_s}` (16 §3.7): the loaded play time of the character; doc 15 appends it at
/// position 10 of the join batch. The welcome lines (`gameplay.welcome_messages`, doc 23) are added by the
/// join driver.
pub fn add_game_join(p: &PlayerExt) -> pb::AddGameJoin {
    pb::AddGameJoin {
        saved_play_time_s: u32::try_from(p.misc.play_time_s.max(0)).unwrap_or(u32::MAX),
        welcome_messages: Vec::new(),
    }
}

const _: () = {
    // the fixed array sizes of the wire contract
    assert!(MAX_HUMAN_PARTS == 31);
    assert!(SM_MAX == 26);
};
