//! The quest condition predicates (Plan/20 §9.4-§9.5, WP-NPC-13): pure functions over a [`QuestProp`] and a
//! [`QuestSubject`]. Each row is the `if` chain of `__IsBeginQuestCondition`, `__IsNextLevelQuest` and
//! `__IsEndQuestCondition` (`Mover.cpp:9550-10678`) ported literally, so an out-of-range mode, comparison, type
//! or operator fails its row just as the original's count fell short. The original counted satisfied checks
//! against `24 + 2·64` (begin and next level) and `21 + 2·64` (end); every row contributes exactly one "pass"
//! (the item rows one per slot), so the explicit AND is equivalent (`oracle.rs` transcribes the counting and the
//! tests compare both).
//!
//! `fixes` is `fidelity.quest_condition_fixes` (PL-159, default `true`): it switches B0a (message), B2, B5, B6, B8,
//! B10, B14, E13, E14 and E15 to the behaviour of the "Fix" column; with `false` the original's behaviour applies,
//! bugs included. The freeze side effect of a true end predicate is the caller's
//! ([`super::QuestBook::freeze_timer`]).

use super::QS_END;
use super::flow::PredicateMode;
use super::subject::QuestSubject;
use flyff_data::quest::{MAX_QUESTCONDITEM, QuestProp, QuestPropItem};
use flyff_data::types::SkillId;
use flyff_db::model::QuestRecord;

/// The result of the begin predicate.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct BeginCheck {
    /// The quest may begin.
    pub pass: bool,
    /// B0a failed and `TID_GAME_LACKSPACE` is due (both modes with the switch off, only `Accept` with it on).
    pub lack_space: bool,
}

/// How the level row reads (B4 or its next-level variant).
#[derive(Copy, Clone, PartialEq, Eq)]
enum LevelRule {
    /// `min == 0 || min <= level <= max`.
    Begin,
    /// `level < min && level + 5 >= min`.
    NextLevel,
}

/// The size test of the party and guild rows: `num == 0` passes, else `==` (comp 0), `>=` (1) or `<=` (-1);
/// any other comparison fails.
fn size_test(size: i32, comp: i8, num: i32) -> bool {
    if num == 0 {
        return true;
    }
    match comp {
        0 => size == num,
        1 => size >= num,
        -1 => size <= num,
        _ => false,
    }
}

/// B11 / E9: `0` no condition, `-1` must not be disguised, else the mover index must equal the value.
fn disguise_ok(idx: i32, s: &dyn QuestSubject) -> bool {
    if idx == 0 || (idx == -1 && !s.is_disguised()) {
        return true;
    }
    idx as u32 == s.mover_index()
}

/// B12 / E8: the pet experience percent and level.
fn pet_ok(exp: i32, level: i32, s: &dyn QuestSubject) -> bool {
    let pet = s.pet();
    let exp_ok = exp == 0 || pet.is_some_and(|p| p.exp_percent >= exp);
    let level_ok = level == -1 || pet.is_some_and(|p| p.level == level);
    exp_ok && level_ok
}

/// The sex test of an item entry: `-1` or equal.
fn sex_matches(e: &QuestPropItem, s: &dyn QuestSubject) -> bool {
    e.sex == -1 || i32::from(e.sex) == s.sex()
}

/// Whether an entry applies to the subject, given a matching sex: `Some(true)` it applies, `Some(false)` it does
/// not (and counts as a pass), `None` for a type other than 0 and 1 (a failure).
fn applies(e: &QuestPropItem, s: &dyn QuestSubject) -> Option<bool> {
    match e.kind {
        0 => Some(e.job_or_item == -1 || e.job_or_item == s.job()),
        1 => Some(e.job_or_item == -1 || s.item_num(e.job_or_item as u32) != 0),
        _ => None,
    }
}

/// One entry of B10 (`enough == false`: fewer than `num`), B14 and E15 (`enough == true`: at least `num`).
/// A sex mismatch fails in the original and is "not applicable" with the fix.
fn item_entry_ok(e: &QuestPropItem, s: &dyn QuestSubject, fixes: bool, enough: bool) -> bool {
    if !sex_matches(e, s) {
        return fixes;
    }
    match applies(e, s) {
        None => false,
        Some(false) => true,
        Some(true) => {
            e.item == 0
                || if enough {
                    s.item_num(e.item) >= e.num
                } else {
                    s.item_num(e.item) < e.num
                }
        }
    }
}

/// B10 / B14 / E15: every entry passes (at most 64 entries exist).
fn items_ok(list: &[QuestPropItem], s: &dyn QuestSubject, fixes: bool, enough: bool) -> bool {
    list.iter()
        .take(MAX_QUESTCONDITEM)
        .all(|e| item_entry_ok(e, s, fixes, enough))
}

/// E14 (any-of): a sex-matching applicable entry with `item_num >= num` passes the row; otherwise it passes only
/// when every entry is not applicable (a sex mismatch counts as not applicable with the fix).
fn one_item_ok(list: &[QuestPropItem], s: &dyn QuestSubject, fixes: bool) -> bool {
    let mut all_not_applicable = true;
    for e in list.iter().take(MAX_QUESTCONDITEM) {
        if !sex_matches(e, s) {
            all_not_applicable &= fixes;
            continue;
        }
        match applies(e, s) {
            None => all_not_applicable = false,
            Some(false) => {}
            Some(true) => {
                if s.item_num(e.item) >= e.num {
                    return true;
                }
                all_not_applicable = false;
            }
        }
    }
    all_not_applicable
}

/// The leader rule of the begin rows: the original compared with a never-computed `false` (passes for -1 and 0);
/// the fix is -1 any, 0 not the leader, 1 the leader.
fn begin_leader_ok(leader: i8, is_leader: bool, fixes: bool) -> bool {
    if fixes {
        leader == -1 || i32::from(leader) == i32::from(is_leader)
    } else {
        leader == -1 || leader == 0
    }
}

/// B5.
fn begin_party_ok(p: &QuestProp, s: &dyn QuestSubject, fixes: bool) -> bool {
    match p.begin_cond_party {
        0 => true,
        1 => s.party().is_none(),
        2 => s.party().is_some_and(|v| {
            begin_leader_ok(p.begin_cond_party_leader, v.is_leader, fixes)
                && size_test(
                    v.size,
                    p.begin_cond_party_num_comp,
                    i32::from(p.begin_cond_party_num),
                )
        }),
        _ => false,
    }
}

/// B6: the original never looked the guild up (mode 1 always passes, mode 2 never); the fix reads the real guild.
fn begin_guild_ok(p: &QuestProp, s: &dyn QuestSubject, fixes: bool) -> bool {
    match p.begin_cond_guild {
        0 => true,
        1 => !fixes || s.guild().is_none(),
        2 => {
            fixes
                && s.guild().is_some_and(|g| {
                    begin_leader_ok(p.begin_cond_guild_leader, g.is_master, true)
                        && size_test(
                            g.size,
                            p.begin_cond_guild_num_comp,
                            i32::from(p.begin_cond_guild_num),
                        )
                })
        }
        _ => false,
    }
}

/// B8 / the next-level variant: `GetSkill`, `CheckSkill(level)` (the original passed the LEVEL as a skill id; the
/// fix passes the skill) and the level compared unsigned (`(DWORD)lv`) in the begin row, signed in next level.
fn begin_skill_ok(p: &QuestProp, s: &dyn QuestSubject, fixes: bool, signed: bool) -> bool {
    let idx: SkillId = p.begin_cond_skill_idx;
    if idx == 0 {
        return true;
    }
    let Some(have) = s.skill_level(idx) else {
        return false;
    };
    let lvl = i32::from(p.begin_cond_skill_lvl);
    let check = if fixes { idx } else { lvl as u32 };
    if !s.check_skill(check) {
        return false;
    }
    if signed {
        have as i32 >= lvl
    } else {
        have >= lvl as u32
    }
}

/// B1..B3 and B5..B15: the rows both begin predicates share (`level` picks B4).
fn begin_rows_ok(p: &QuestProp, s: &dyn QuestSubject, fixes: bool, level: LevelRule) -> bool {
    let book = s.quests();
    // B1 previous quests: 0 active or completed, 1 completed and not active, 2 active and not completed
    for q in p.begin_cond_previous_quest {
        if q == 0 {
            continue;
        }
        let (a, c) = (book.is_active(u32::from(q)), book.is_complete(u32::from(q)));
        let ok = match p.begin_cond_previous_quest_type {
            0 => a || c,
            1 => !a && c,
            2 => a && !c,
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    // B2 exclusive quests: not active, and not completed (the original read the PREVIOUS quest of the slot)
    for (i, q) in p.begin_cond_exclusive_quest.iter().copied().enumerate() {
        if q == 0 {
            continue;
        }
        let done = if fixes {
            book.is_complete(u32::from(q))
        } else {
            book.is_complete(u32::from(
                p.begin_cond_previous_quest.get(i).copied().unwrap_or(0),
            ))
        };
        if book.is_active(u32::from(q)) || done {
            return false;
        }
    }
    // B3 jobs
    if !p.begin_cond_job.is_empty() && !p.begin_cond_job.iter().any(|j| i32::from(*j) == s.job()) {
        return false;
    }
    // B4 level
    let (min, max) = (
        i32::from(p.begin_cond_level_min),
        i32::from(p.begin_cond_level_max),
    );
    let lv = s.level();
    let level_ok = match level {
        LevelRule::Begin => min == 0 || (lv >= min && lv <= max),
        LevelRule::NextLevel => lv < min && lv + 5 >= min,
    };
    if !level_ok {
        return false;
    }
    // B5 party, B6 guild
    if !begin_party_ok(p, s, fixes) || !begin_guild_ok(p, s, fixes) {
        return false;
    }
    // B7 sex
    if !(p.begin_cond_sex == -1 || i32::from(p.begin_cond_sex) == s.sex()) {
        return false;
    }
    // B8 skill (the next-level variant compares signed)
    if !begin_skill_ok(p, s, fixes, level == LevelRule::NextLevel) {
        return false;
    }
    // B9 PK value
    if !(p.begin_cond_pk_value == 0 || p.begin_cond_pk_value <= s.pk_value()) {
        return false;
    }
    // B10 items that must not be owned
    if !items_ok(&p.begin_cond_not_item, s, fixes, false) {
        return false;
    }
    // B11 disguise, B12 pet, B13 tutorial
    if !disguise_ok(p.begin_cond_disguise_mover_index, s)
        || !pet_ok(p.begin_cond_pet_exp, p.begin_cond_pet_level, s)
        || !(p.begin_cond_tutorial_state == -1 || s.tutorial_state() >= p.begin_cond_tutorial_state)
    {
        return false;
    }
    // B14 items that must be owned
    if !items_ok(&p.begin_cond_item, s, fixes, true) {
        return false;
    }
    // B15 campus points
    p.begin_cond_tsp == 0 || s.campus_points() >= p.begin_cond_tsp
}

/// B0: the quest is neither active nor completed (a repeatable quest is removed from both lists to run again).
fn b0_ok(p: &QuestProp, s: &dyn QuestSubject) -> bool {
    let book = s.quests();
    !book.is_active(p.id) && !book.is_complete(p.id)
}

/// `__IsBeginQuestCondition` (B0, B0a, B1-B15). B0a: fewer free inventory slots than used `SetBeginSetAddItem`
/// slots fails the check and asks for `TID_GAME_LACKSPACE`; the original sent it also while the lists were built.
pub fn can_begin(
    p: &QuestProp,
    s: &dyn QuestSubject,
    mode: PredicateMode,
    fixes: bool,
) -> BeginCheck {
    if !b0_ok(p, s) {
        return BeginCheck::default();
    }
    let used = p.begin_set_add_items.iter().filter(|a| a.item != 0).count();
    if usize::try_from(s.empty_slots()).unwrap_or(0) < used {
        return BeginCheck {
            pass: false,
            lack_space: !fixes || mode == PredicateMode::Accept,
        };
    }
    BeginCheck {
        pass: begin_rows_ok(p, s, fixes, LevelRule::Begin),
        lack_space: false,
    }
}

/// `__IsNextLevelQuest`: B0 without B0a, B1-B3, B5-B15, and B4 replaced by `level < min && level + 5 >= min`
/// (the quest is displayed red and never accepted).
pub fn is_next_level(p: &QuestProp, s: &dyn QuestSubject, fixes: bool) -> bool {
    b0_ok(p, s) && begin_rows_ok(p, s, fixes, LevelRule::NextLevel)
}

/// E13 / E12 leader rule: 0 any, 1 not the leader, 2 the leader.
fn end_leader_ok(leader: i8, is_leader: bool) -> bool {
    leader == 0 || i32::from(leader) - 1 == i32::from(is_leader)
}

/// E10.
fn end_state_ok(p: &QuestProp, r: &QuestRecord) -> bool {
    if p.end_cond_state == 0 {
        p.last_defined_state() == r.state
    } else {
        p.end_cond_state == r.state
    }
}

/// E11.
fn complete_quests_ok(p: &QuestProp, s: &dyn QuestSubject) -> bool {
    let book = s.quests();
    let c = &p.end_cond_complete_quest;
    match p.end_cond_complete_quest_oper {
        0 => true,
        // `IsCompleteQuest` on every slot, literally: a 0 slot counts only if 0 is in the completed list
        1 => c.iter().any(|q| book.is_complete(u32::from(*q))),
        2 => c.iter().all(|q| *q == 0 || book.is_complete(u32::from(*q))),
        _ => false,
    }
}

/// E12.
fn end_party_ok(p: &QuestProp, s: &dyn QuestSubject) -> bool {
    match p.end_cond_party {
        0 => true,
        1 => s.party().is_none(),
        2 => s.party().is_some_and(|v| {
            end_leader_ok(p.end_cond_party_leader, v.is_leader)
                && size_test(
                    v.size,
                    p.end_cond_party_num_comp,
                    i32::from(p.end_cond_party_num),
                )
        }),
        _ => false,
    }
}

/// E13: the original looked the guild up by the PLAYER id (no guild: mode 1 passes, mode 2 never); the fix reads
/// the real guild.
fn end_guild_ok(p: &QuestProp, s: &dyn QuestSubject, fixes: bool) -> bool {
    match p.end_cond_guild {
        0 => true,
        1 => !fixes || s.guild().is_none(),
        2 => {
            fixes
                && s.guild().is_some_and(|g| {
                    end_leader_ok(p.end_cond_guild_leader, g.is_master)
                        && size_test(
                            g.size,
                            p.end_cond_guild_num_comp,
                            i32::from(p.end_cond_guild_num),
                        )
                })
        }
        _ => false,
    }
}

/// `__IsEndQuestCondition` (E1-E16) for an active record that is not at state 14. Pure: the freeze of a running
/// timer (KEEP, also while a list is built and also when the completion then fails) is the caller's.
pub fn can_end(p: &QuestProp, s: &dyn QuestSubject, fixes: bool) -> bool {
    let Some(r) = s.quests().find(p.id) else {
        return false;
    };
    if r.state == QS_END {
        return false;
    }
    // E1 time limit: the stored word (the frozen bit counts as non-zero)
    if !(p.end_cond_limit_time == 0 || r.time_left_min != 0 || r.timer_frozen) {
        return false;
    }
    // E2 patrol
    if !(p.end_cond_patrol.is_none_or(|z| z.world == 0) || r.patrol_done) {
        return false;
    }
    // E3 kills
    for (k, have) in p.end_cond_kill_npc.iter().zip(r.kills) {
        if !(k.mover == 0 || i32::from(have) >= k.num) {
            return false;
        }
    }
    // E4 skill
    if p.end_cond_skill_idx != 0 {
        let idx = p.end_cond_skill_idx;
        let ok = s.skill_level(idx).is_some_and(|have| {
            s.check_skill(idx) && have >= i32::from(p.end_cond_skill_lvl) as u32
        });
        if !ok {
            return false;
        }
    }
    // E5 gold (not removed here)
    if !(p.end_cond_gold == 0 || s.gold() >= i64::from(p.end_cond_gold)) {
        return false;
    }
    // E6 level, E7 experience percent
    let lv = s.level();
    if !(p.end_cond_level_min == 0
        || (lv >= i32::from(p.end_cond_level_min) && lv <= i32::from(p.end_cond_level_max)))
    {
        return false;
    }
    let pct = s.exp_percent();
    if !(p.end_cond_exp_percent_min == 0
        || (pct >= p.end_cond_exp_percent_min && pct <= p.end_cond_exp_percent_max))
    {
        return false;
    }
    // E8 pet, E9 disguise, E10 state, E11 completed quests
    if !pet_ok(p.end_cond_pet_exp, p.end_cond_pet_level, s)
        || !disguise_ok(p.end_cond_disguise_mover_index, s)
        || !end_state_ok(p, r)
        || !complete_quests_ok(p, s)
    {
        return false;
    }
    // E12 party, E13 guild
    if !end_party_ok(p, s) || !end_guild_ok(p, s, fixes) {
        return false;
    }
    // E14 any of the items, E15 all of the items, E16 campus points
    one_item_ok(&p.end_cond_one_item, s, fixes)
        && items_ok(&p.end_cond_item, s, fixes, true)
        && (p.end_cond_tsp == 0 || s.campus_points() >= p.end_cond_tsp)
}
