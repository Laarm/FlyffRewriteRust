//! Snapshot helpers (Plan/16 §3.6, WP-MOVER-03): the columns and documents a [`Mover`] owns.
//!
//! They take `&Mover` (13's `PlayerState`, 16 §3.3: the mover with `MoverKind::Player`) and are pure: no
//! clocks, no I/O. [`fill_row`] writes every snapshot-owned column of `characters` that the mover knows (the
//! round trip with `Mover::new_player` preserves each of them); [`docs`] builds the nine documents. The
//! session-dependent values (`play_time_s` of the stay, `last_logout_at`, the banks, the equipment preview of
//! the character list) are WP-DB-07's.

use crate::mover::Mover;
use crate::session::SessionEntry;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, LayerId, SaveGen, UnixMs, UnixTime, WorldId};
use flyff_db::{
    BuffsDoc, CharacterDocs, CharacterRow, CharacterSnapshot, ContainerRecord, SaveReason,
    SkillsDoc, SmTimersDoc, TaskbarDoc,
};
use flyff_rules::consts::{FRESH_MODE, MAX_HUMAN_PARTS};

/// `characters.mode_flags`: `FRESH_MODE` is cleared in the persisted value (13 §17.3, KEEP: the in-memory
/// flag stays until the next login).
pub fn persisted_mode(mode: u32) -> u32 {
    mode & !FRESH_MODE
}

/// The `skills` document (`NULL_ID` slots become `None`, 16 §3.6).
pub fn skills_doc(m: &Mover) -> Option<SkillsDoc> {
    Some(m.player()?.progression.skills_doc())
}

/// The `sm_timers` document (resist bytes stored as `i32`, `0xFF` = none, 16 §3.6).
pub fn sm_timers_doc(m: &Mover) -> Option<SmTimersDoc> {
    Some(m.player()?.sm.to_doc())
}

/// The `buffs` document: every buff except `BUFF_EQUIP` and the housing buffs (never stored, 13 §17.3 rule 5)
/// with the remaining time at `now`.
pub fn buffs_doc(m: &Mover, now: GameMs) -> BuffsDoc {
    crate::buff::persist::snapshot(m, now)
}

/// The nine documents of a player: the owned ones are rebuilt from the runtime state (skills, SM timers,
/// buffs, taskbar with the action points, honor), the item and quest documents are the stand-ins' carried
/// documents. `None` for a non-player.
pub fn docs(m: &Mover, now: GameMs) -> Option<CharacterDocs> {
    let p = m.player()?;
    let taskbar = TaskbarDoc {
        action_points: p.skills_rt.action_points,
        ..p.taskbar.clone()
    };
    Some(CharacterDocs {
        inventory: p.items.inventory_doc(),
        pockets: p.items.pockets_doc(),
        skills: p.progression.skills_doc(),
        quests: p.quests.doc.clone(),
        taskbar,
        buffs: buffs_doc(m, now),
        sm_timers: p.sm.to_doc(),
        honor: p.social.honor_doc.clone(),
        settings: p.misc.settings.clone(),
    })
}

/// Writes every snapshot-owned `characters` column the mover owns into `row` (16 §3.6): look, scale, world,
/// layer and position, return and marking positions, action state, pools (raw, not clamped), job, level,
/// `max(max_level, level)`, EXP, base stats, points, gold, flight EXP, tutorial state, fuel, mode (without
/// `FRESH_MODE`), fame, PK fields, death and angel values, mute, attendance, coupons, honor title, summoned
/// pet and the equipment preview. Returns `false` (and leaves `row` alone) for a non-player.
///
/// `play_time_s` is the value loaded at join; the stay's seconds and `last_logout_at` are added by the
/// session layer (12 §12.6).
pub fn fill_row(m: &Mover, row: &mut CharacterRow) -> bool {
    let Some(p) = m.player() else {
        return false;
    };
    row.sex = p.appearance.sex;
    row.mover_index = m.core.index;
    row.skin_set = p.appearance.skin_set;
    row.hair_mesh = p.appearance.hair_mesh;
    row.hair_color = p.appearance.hair_color;
    row.head_mesh = p.appearance.head_mesh;
    row.hide_coat_mask = p.appearance.hide_coat_mask();
    row.scale = m.core.scale;
    row.world_id = m.core.world.0;
    row.layer = m.core.layer.0;
    row.pos = [m.core.pos.x, m.core.pos.y, m.core.pos.z];
    row.return_world_id = p.misc.return_world.0;
    row.return_pos = [
        p.misc.return_pos.x,
        p.misc.return_pos.y,
        p.misc.return_pos.z,
    ];
    row.marking_world_id = p.misc.marking_world.0;
    row.marking_pos = [
        p.misc.marking_pos.x,
        p.misc.marking_pos.y,
        p.misc.marking_pos.z,
    ];
    row.motion = m.sim.motion.id;
    row.act_state = m.sim.act.state.bits();
    row.act_state_flags = m.sim.act.flags.bits();
    row.hp = m.mv.hp;
    row.mp = m.mv.mp;
    row.fp = m.mv.fp;
    row.job = u8::try_from(m.mv.job.max(0)).unwrap_or(0);
    let level = u16::try_from(m.mv.level.clamp(1, i32::from(u16::MAX))).unwrap_or(1);
    row.level = level;
    row.max_level = p.progression.max_level.max(level);
    row.exp = p.progression.exp;
    row.stat_str = m.mv.base.str;
    row.stat_sta = m.mv.base.sta;
    row.stat_dex = m.mv.base.dex;
    row.stat_int = m.mv.base.int;
    row.stat_points = p.progression.stat_points;
    row.skill_points = p.progression.skill_points;
    row.skill_level_total = p.progression.skill_level_total;
    row.gold = i64::from(p.items.gold.max(0));
    row.flight_exp = p.progression.flight_exp;
    row.tutorial_state = p.progression.tutorial_state;
    row.fuel = m.sim.fuel;
    // `tm_acc_fuel` is never negative (31 §9.2)
    row.fuel_time_acc = u32::try_from(m.sim.tm_acc_fuel.max(0)).unwrap_or(0);
    row.mode_flags = persisted_mode(m.mv.mode);
    row.fame = p.progression.fame;
    row.pk_value = p.pk.value;
    row.pk_propensity = p.pk.propensity;
    row.pk_exp = p.pk.pk_exp;
    row.death_exp = p.progression.death_exp;
    row.death_level = p.progression.death_level;
    row.angel_exp = p.progression.angel_exp;
    row.angel_level = p.progression.angel_level;
    row.mute_remaining_s = p.misc.mute_remaining_s;
    row.messenger_state = p.misc.messenger_state;
    row.play_time_s = p.misc.play_time_s;
    row.attendance_bits = p.misc.attendance_bits;
    row.attendance_minutes = p.misc.attendance_minutes;
    row.attendance_elapsed = p.misc.attendance_elapsed;
    row.coupon_count = p.misc.coupon_count;
    row.honor_title_id = p.social.honor.and_then(|h| u16::try_from(h).ok());
    row.summoned_pet_obj_id = p.items.pet_obj_id;
    row.rest_points = p.misc.rest_points;
    row.last_logout_at = p.misc.last_logout_at;
    for (dst, e) in row
        .equipment_preview
        .iter_mut()
        .zip(m.mv.equip_info.iter())
        .take(MAX_HUMAN_PARTS)
    {
        *dst = if e.is_empty() { 0 } else { e.item_id };
    }
    true
}

/// A fresh [`CharacterRow`] of the player (`CharacterRow::default()` + [`fill_row`]).
pub fn row(m: &Mover) -> Option<CharacterRow> {
    let mut r = CharacterRow::default();
    fill_row(m, &mut r).then_some(r)
}

/// Where the character is saved when it is not where the world says (the logout location of
/// `world::destroy_player`, 15 §18.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Location {
    /// The world.
    pub world: WorldId,
    /// The position.
    pub pos: Vec3,
    /// The layer.
    pub layer: LayerId,
}

/// What [`build`] needs besides the mover (13 §17.2).
#[derive(Clone, Copy, Debug)]
pub struct SnapshotCtx<'a> {
    /// The session record (play time base and join instant).
    pub entry: &'a SessionEntry,
    /// The fresh generation.
    pub generation: SaveGen,
    /// Why the snapshot is taken.
    pub reason: SaveReason,
    /// The tick time (buff remaining time).
    pub now: GameMs,
    /// The wall clock of the tick (play time, `taken_at`, `last_logout_at`).
    pub unix_ms: UnixMs,
    /// `gameplay.max_pack_item`: the upper bound of a stack count.
    pub max_pack: u16,
    /// The location to store instead of the mover's own (`None` = where it stands).
    pub location: Option<Location>,
}

/// `play_time_s` of a snapshot (12 §12.6, PL-038): `play_time_base_s + floor((tick wall ms - joined_at_unix) /
/// 1000)`. The time of character select is never counted; a re-attach keeps `joined_at_unix`.
pub fn play_time_s(entry: &SessionEntry, unix_ms: UnixMs) -> i64 {
    let stay_ms = unix_ms.0.saturating_sub(entry.joined_at_unix.0).max(0);
    entry.play_time_base_s.saturating_add(stay_ms / 1000)
}

/// Clamps the stack counts of a container to `1..=max_pack` (13 §16.6: a snapshot write never fails because of
/// game values; the clamp is logged). Returns the number of items changed.
fn clamp_counts(c: &mut ContainerRecord, max_pack: u16) -> usize {
    let mut changed = 0;
    for it in &mut c.items {
        let v = it.count.clamp(1, max_pack.max(1));
        if v != it.count {
            tracing::warn!(
                item = it.item_id,
                count = it.count,
                to = v,
                "snapshot: stack count clamped"
            );
            it.count = v;
            changed += 1;
        }
    }
    changed
}

/// `snapshot::build` (13 §17.2-§17.3): the row of [`fill_row`] with the session values (the stay's play time,
/// `last_logout_at` on a logout snapshot, the logout location), the nine documents, and every bank loaded at
/// enter-world; gold and stack counts are clamped. `None` for a non-player.
///
/// Rules (13 §17.3): `FRESH_MODE` is cleared in the stored mode, `max_level = max(max_level, level)`, buffs carry
/// their remaining time at `now` (`BUFF_EQUIP` excluded), the equipment preview follows the worn items.
pub fn build(m: &Mover, cx: &SnapshotCtx<'_>) -> Option<CharacterSnapshot> {
    let p = m.player()?;
    let mut row = CharacterRow::default();
    fill_row(m, &mut row);
    if let Some(loc) = cx.location {
        row.world_id = loc.world.0;
        row.layer = loc.layer.0;
        row.pos = [loc.pos.x, loc.pos.y, loc.pos.z];
    }
    row.gold = row.gold.clamp(0, i64::from(i32::MAX));
    row.play_time_s = play_time_s(cx.entry, cx.unix_ms);
    let taken_at: UnixTime = cx.unix_ms.to_unix_time();
    if matches!(
        cx.reason,
        SaveReason::Logout | SaveReason::ToCharSelect | SaveReason::Shutdown
    ) {
        row.last_logout_at = taken_at;
    }
    let mut docs = docs(m, cx.now)?;
    clamp_counts(&mut docs.inventory.container, cx.max_pack);
    for pocket in &mut docs.pockets.pockets {
        clamp_counts(&mut pocket.container, cx.max_pack);
    }
    let banks = p
        .items
        .bank_snapshots()
        .into_iter()
        .map(|mut b| {
            clamp_counts(&mut b.doc.container, cx.max_pack);
            b.gold = b.gold.clamp(0, i64::from(i32::MAX));
            b
        })
        .collect();
    Some(CharacterSnapshot {
        account_id: p.ident.account_id,
        character_id: p.ident.character_id,
        r#gen: cx.generation,
        reason: cx.reason,
        taken_at,
        row,
        docs,
        banks,
    })
}
