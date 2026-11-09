//! Random options (Plan/19 §12.6, F-232..F-234, F-238; ref/13 §18.7, ref/07c §17; src:_Common/randomoption.cpp,
//! DPSrvr.cpp:5489-5616, 6171-6295, 11121-11210, 12012-12260, 12702, 12891-12977, User.cpp:8963-9010):
//!
//! * [`gen_random_option`]: the awakening generator over the raw tables of `randomoption.inc` (count weights,
//!   DST weights, 1e9 value steps, the part pools of the gloves, boots and helmets, the `decrease` variant);
//! * [`on_stat_scroll`]: the random STR / DEX / INT / STA scroll (`features.improved_stat_scroll`);
//! * [`on_npc_awakening`], [`on_blessedness_cancel`], [`on_eat_pet_cancel`]: the NPC services;
//! * [`generate_on`] / [`initialize_on`]: the two helpers behind `DoUseItemTarget` ([`super::target`]), with the
//!   protected awakening (`__PROTECT_AWAKE`, PL-079): the pending word is persisted and offered again after the
//!   join ([`resend_pending`]);
//! * [`on_select_value`] answers `ItemSelectAwakeningValueReq`;
//! * [`on_retry`]: the Scroll of Augmentation (**FIX VR-023 / PL-068**: a line outside `1..=line count` is
//!   refused and nothing is consumed);
//! * [`on_remove_level_down`].

use super::refine::{has_item_buff, remove_item_buff};
use super::txn::{
    Outcome, UpgradeCx, audit, equipped, item_at, material, pay, sound, take_one, usable,
};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::model::{Item, PetLevel};
use crate::item::ops::{UiParam, core as ops};
use crate::item::usage::UseFacts;
use crate::mover::Mover;
use crate::world::fx::create_sfx_msg;
use flyff_common::defs::item::{II_SYS_SYS_SCR_AWAKESAFE, II_SYS_SYS_SCR_RANDOMOPTIONRTRY};
use flyff_common::defs::itemkind::{
    IK1_WEAPON, IK2_ARMOR, IK2_ARMORETC, IK3_EGG, IK3_RANDOM_SCROLL,
};
use flyff_common::defs::neuz::{PARTS_CAP, PARTS_FOOT, PARTS_HAND};
use flyff_common::defs::obj::XI_INT_SUCCESS;
use flyff_common::defs::text::{
    TID_GAME_AWAKE_OR_BLESSEDNESS01, TID_GAME_AWAKECANCEL_INFO, TID_GAME_AWAKENING_SUCCESS,
    TID_GAME_BLESSEDNESS_CANCEL, TID_GAME_BLESSEDNESS_CANCEL_INFO, TID_GAME_EQUIPPUT,
    TID_GAME_INVALID_TARGET_ITEM, TID_GAME_PICKUP_PET_AWAKENING_CANCEL_E001,
    TID_GAME_RANDOMSCROLL_ERROR, TID_GAME_RANDOMSCROLL_SUCCESS, TID_GAME_REGARDLESS_USE04,
    TID_GAME_SMELT_SAFETY_SUCCESS, TID_MMI_RANDOMOPTIONRETRY_ERRORTEXT05, TID_TOOLTIP_DEX,
    TID_TOOLTIP_INT, TID_TOOLTIP_STA, TID_TOOLTIP_STR,
};
use flyff_common::rng::GameRng;
use flyff_data::items_ext::random_option::{RandomOptionGroup, RandomOptionTables};
use flyff_proto::pb;
use flyff_rules::chat::TEXT_DIAG;
use flyff_rules::consts::dst::{
    DST_ADJDEF, DST_ATKPOWER, DST_ATTACKSPEED, DST_CHR_CHANCECRITICAL, DST_CRITICAL_BONUS, DST_DEX,
    DST_FP_MAX, DST_HP_MAX, DST_INT, DST_MP_MAX, DST_SPEED, DST_SPELL_RATE, DST_STA, DST_STR,
};
use flyff_rules::item::random_option::{
    RandomOptKind, RandomOptLine, RandomOptWord, random_option_kind,
};
use flyff_rules::item::refine::{
    AWAKENING_COST, STAT_SCROLL_SPACE, ScrollStat, scroll_option_id, scroll_stat, stat_scroll_value,
};

/// The block name of a kind in `randomoption.inc`.
fn group_name(kind: RandomOptKind) -> &'static str {
    match kind {
        RandomOptKind::Awakening => "Awakening",
        RandomOptKind::Blessing => "Blessing",
        RandomOptKind::SystemPet => "SystemPet",
        RandomOptKind::EatPet => "EatPet",
    }
}

/// `AwakeningExtension`: the destinations an awakening may give to a part (gloves, boots, helmets). The suit and
/// every other part use the whole table.
fn part_pool(parts: u32) -> Option<&'static [u32]> {
    match parts {
        PARTS_HAND => Some(&[
            DST_CHR_CHANCECRITICAL,
            DST_ATKPOWER,
            DST_SPELL_RATE,
            DST_ATTACKSPEED,
            DST_MP_MAX,
            DST_FP_MAX,
        ]),
        PARTS_FOOT => Some(&[
            DST_CRITICAL_BONUS,
            DST_SPEED,
            DST_ADJDEF,
            DST_HP_MAX,
            DST_MP_MAX,
            DST_FP_MAX,
        ]),
        PARTS_CAP => Some(&[DST_STR, DST_DEX, DST_INT, DST_STA, DST_MP_MAX, DST_FP_MAX]),
        _ => None,
    }
}

/// `DetermineRandomOptionSize`: the number of lines by the cumulative weights `a`, `a + b`, `a + b + c`; the
/// draw is `random_below(a + b + c)`.
fn determine_size(g: &RandomOptionGroup, rng: &mut dyn GameRng) -> usize {
    let cum = [
        i64::from(g.a),
        i64::from(g.a) + i64::from(g.b),
        i64::from(g.a) + i64::from(g.b) + i64::from(g.c),
    ];
    let upper = u32::try_from(cum[2].max(0)).unwrap_or(0);
    let r = i64::from(rng.random_below(upper));
    cum.iter().position(|&c| r < c).map_or(0, |i| i + 1)
}

/// `DetermineRandomOptionDst`: the entry chosen by `random_below(sum of the weights)` over the (part-filtered)
/// table in file order. `None` for an empty table.
fn determine_dst<'a>(
    g: &'a RandomOptionGroup,
    pool: Option<&[u32]>,
    rng: &mut dyn GameRng,
) -> Option<&'a flyff_data::items_ext::random_option::RandomOptionDst> {
    let allowed = |d: &&flyff_data::items_ext::random_option::RandomOptionDst| {
        pool.is_none_or(|p| p.contains(&d.dst))
    };
    let total: i64 = g.dsts.iter().filter(allowed).map(|d| i64::from(d.n)).sum();
    let r = i64::from(rng.random_below(u32::try_from(total.max(0)).unwrap_or(0)));
    let mut cum = 0_i64;
    for d in g.dsts.iter().filter(allowed) {
        cum += i64::from(d.n);
        if r < cum {
            return Some(d);
        }
    }
    None
}

/// `DetermineRandomOptionAdj`: `random_below(1e9)` against the cumulative value steps; with `decrease` and a
/// step other than the first, the value of the **previous** step (with the shipped data the next higher one).
fn determine_adj(
    d: &flyff_data::items_ext::random_option::RandomOptionDst,
    decrease: bool,
    rng: &mut dyn GameRng,
) -> i32 {
    let r = i64::from(rng.random_below(1_000_000_000));
    let mut cum = 0_i64;
    for (i, [adj, prob]) in d.values.iter().enumerate() {
        cum += i64::from(*prob);
        if r < cum {
            if decrease
                && i > 0
                && let Some([prev, _]) = d.values.get(i - 1)
            {
                return *prev;
            }
            return *adj;
        }
    }
    0
}

/// `CRandomOptionProperty::GenRandomOption( word, kind, parts, decrease )`. Refused (`false`) when the word has
/// lines (`GetRandomOptionSize > 0`; a word with the protected-awakening flag counts as empty). The draws are,
/// in order: the line count, the index that decreases (only with `decrease`), then per line the destination and
/// the value.
pub fn gen_random_option(
    tables: &RandomOptionTables,
    kind: RandomOptKind,
    parts: u32,
    word: &mut RandomOptWord,
    decrease: bool,
    rng: &mut dyn GameRng,
) -> bool {
    if word.line_count() > 0 {
        return false;
    }
    let Some(g) = tables.group(group_name(kind)) else {
        return true;
    };
    let n = determine_size(g, rng);
    let decrease_line = if decrease {
        Some(rng.random_below(n as u32) as usize)
    } else {
        None
    };
    let pool = if kind == RandomOptKind::Awakening {
        part_pool(parts)
    } else {
        None
    };
    for j in 0..n {
        let Some(d) = determine_dst(g, pool, rng) else {
            break;
        };
        // the original counts the lines down: the line with `remaining == decrease index` is the one
        let remaining = n - 1 - j;
        let adj = determine_adj(d, decrease_line == Some(remaining), rng);
        let (Ok(dst), Ok(adj)) = (u8::try_from(d.dst), i16::try_from(adj)) else {
            continue;
        };
        let _ = word.push_line(RandomOptLine { dst, adj });
    }
    true
}

/// `GetRandomOptionKind( item )`: which table the item uses (`None` = −1).
pub fn awakening_kind(cx: &UpgradeCx<'_>, item: &Item) -> Option<RandomOptKind> {
    let p = cx.prop(item.item_id)?;
    let egg_c = item
        .pet
        .as_ref()
        .is_some_and(|pet| pet.level >= PetLevel::C)
        && p.item_kind3 == IK3_EGG;
    random_option_kind(p.parts, p.item_kind3, egg_c, cx.env.cfg().only_armor_awakes)
}

fn stat_text(stat: ScrollStat) -> u32 {
    match stat {
        ScrollStat::Str => TID_TOOLTIP_STR,
        ScrollStat::Dex => TID_TOOLTIP_DEX,
        ScrollStat::Sta => TID_TOOLTIP_STA,
        ScrollStat::Int => TID_TOOLTIP_INT,
    }
}

/// `CString::Format` for `%s` and `%d` in order (the two conversions the item texts use).
pub fn format_text(template: &str, args: &[Arg<'_>]) -> String {
    let mut out = String::with_capacity(template.len() + 24);
    let mut args = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail.chars().next() {
            Some('s') => {
                if let Some(Arg::S(s)) = args.next() {
                    out.push_str(s);
                }
                rest = &tail[1..];
            }
            Some('d') => {
                match args.next() {
                    Some(Arg::D(n)) => out.push_str(&n.to_string()),
                    Some(Arg::S(s)) => out.push_str(s),
                    None => {}
                }
                rest = &tail[1..];
            }
            Some('%') => {
                out.push('%');
                rest = &tail[1..];
            }
            _ => {
                out.push('%');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One argument of [`format_text`].
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    /// `%s`.
    S(&'a str),
    /// `%d`.
    D(i64),
}

/// `OnRandomScroll{item, scroll}` (Plan/19 §12.6.1). Silent for a scroll of another kind and for missing or
/// reserved items; `TID_GAME_EQUIPPUT` for a worn target, `TID_GAME_RANDOMSCROLL_ERROR` for an item that is not
/// a weapon or armour. The value is rolled once (`random_below(100)`): tier 1..4. Writes the stat id into the low
/// byte (an earlier stat option is overwritten, awakening lines and level-down stay), consumes the scroll, plays
/// the success sound and effect and tells the player the new value.
pub fn on_stat_scroll(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::RandomScroll,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    let (Some(target), Some(scroll)) = (usable(m, msg.item_obj_id), usable(m, msg.scroll_obj_id))
    else {
        return Outcome::Ignored;
    };
    if target == scroll {
        return Outcome::Ignored;
    }
    if equipped(m, target) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let (Some(before), Some(scroll_item)) =
        (item_at(m, target).cloned(), item_at(m, scroll).cloned())
    else {
        return Outcome::Ignored;
    };
    let (Some(tp), Some(sp)) = (cx.prop(before.item_id), cx.prop(scroll_item.item_id)) else {
        return Outcome::Ignored;
    };
    if sp.item_kind3 != IK3_RANDOM_SCROLL {
        return Outcome::Ignored;
    }
    if !(tp.item_kind1 == IK1_WEAPON || matches!(tp.item_kind2, IK2_ARMOR | IK2_ARMORETC)) {
        fx.text(TID_GAME_RANDOMSCROLL_ERROR);
        return Outcome::Refused;
    }
    let value = stat_scroll_value(
        rng.random_below(STAT_SCROLL_SPACE),
        cx.env.cfg().improved_stat_scroll,
    );
    let stat = scroll_stat(scroll_item.item_id);
    let id = scroll_option_id(stat, value);
    fx.mark_urgent();
    ops::update_item(
        m,
        cx.env,
        target,
        UiParam::RandomOptItemId,
        u32::from(id),
        0,
        fx,
    );
    take_one(m, cx, scroll, fx);
    // the original shows the effect even to an invisible GM here
    sound(fx, true);
    fx.out
        .near(create_sfx_msg(XI_INT_SUCCESS, m.core.pos, false));
    let texts = &cx.env.data().texts;
    let stat_name = texts.get(stat_text(stat.unwrap_or(ScrollStat::Int)));
    let line = format_text(
        texts.get(TID_GAME_RANDOMSCROLL_SUCCESS),
        &[
            Arg::S(&tp.name),
            Arg::S(stat_name),
            Arg::D(i64::from(value)),
        ],
    );
    fx.out.to_self(pb::Text {
        kind: flyff_rules::chat::TEXT_GENERAL,
        text: line,
    });
    let after = item_at(m, target).cloned();
    audit(
        fx,
        "stat_scroll",
        "scroll",
        "success",
        &before,
        after.as_ref(),
        vec![material(scroll_item.item_id, 1)],
    );
    Outcome::Success
}

/// `OnAwakening{item}` at the awakening NPC (Plan/19 §12.6.3): the NPC in range (`npc_near`, VR-012), 100 000
/// penya (`TID_GAME_LACKMONEY`), a usable item of the awakening kind (`TID_GAME_INVALID_TARGET_ITEM`) without
/// lines (`TID_GAME_AWAKE_OR_BLESSEDNESS01`). The NPC route never uses the protection flow: the word is
/// initialised (the safe flag is cleared too) and filled.
pub fn on_npc_awakening(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::Awakening,
    npc_near: bool,
    facts: &UseFacts,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    if !npc_near || facts.trading || facts.vending {
        return Outcome::Ignored;
    }
    if crate::item::gold::core::gold(m) < AWAKENING_COST {
        fx.text(flyff_common::defs::text::TID_GAME_LACKMONEY);
        return Outcome::Refused;
    }
    let Some(id) = u32::try_from(msg.item_obj_id)
        .ok()
        .and_then(|o| usable(m, o))
    else {
        return Outcome::Ignored;
    };
    let Some(before) = item_at(m, id).cloned() else {
        return Outcome::Ignored;
    };
    let Some(tp) = cx.prop(before.item_id) else {
        return Outcome::Ignored;
    };
    if awakening_kind(cx, &before) != Some(RandomOptKind::Awakening) {
        fx.text(TID_GAME_INVALID_TARGET_ITEM);
        return Outcome::Refused;
    }
    if before.random_option.line_count() > 0 {
        fx.text(TID_GAME_AWAKE_OR_BLESSEDNESS01);
        return Outcome::Refused;
    }
    fx.mark_urgent();
    let mut word = before.random_option;
    word.initialize();
    gen_random_option(
        &cx.env.data().random_option,
        RandomOptKind::Awakening,
        tp.parts,
        &mut word,
        false,
        rng,
    );
    ops::update_item_ex(m, cx.env, id, word, fx);
    let _ = pay(m, cx, AWAKENING_COST, fx);
    fx.text(TID_GAME_AWAKENING_SUCCESS);
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "awaken",
        "npc",
        "success",
        &before,
        after.as_ref(),
        vec![serde_json::json!({ "gold": AWAKENING_COST })],
    );
    Outcome::Success
}

/// `OnBlessednessCancel{item}` at the blessing-cancel NPC: free. A blessed item (kind blessing with lines)
/// loses its lines and the player gets the diag text `TID_GAME_BLESSEDNESS_CANCEL_INFO`; anything else gets
/// `TID_GAME_BLESSEDNESS_CANCEL`.
pub fn on_blessedness_cancel(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::BlessednessCancel,
    npc_near: bool,
    fx: &mut Fx,
) -> Outcome {
    if !npc_near {
        return Outcome::Ignored;
    }
    let Some(id) = u32::try_from(msg.item_obj_id)
        .ok()
        .and_then(|o| usable(m, o))
    else {
        return Outcome::Ignored;
    };
    let Some(before) = item_at(m, id).cloned() else {
        return Outcome::Ignored;
    };
    if awakening_kind(cx, &before) == Some(RandomOptKind::Blessing)
        && before.random_option.line_count() > 0
    {
        fx.mark_urgent();
        let mut word = before.random_option;
        word.initialize();
        ops::update_item_ex(m, cx.env, id, word, fx);
        fx.out.to_self(pb::Text {
            kind: TEXT_DIAG,
            text: cx
                .env
                .data()
                .texts
                .get(TID_GAME_BLESSEDNESS_CANCEL_INFO)
                .to_owned(),
        });
        let after = item_at(m, id).cloned();
        audit(
            fx,
            "blessing_cancel",
            "npc",
            "success",
            &before,
            after.as_ref(),
            vec![],
        );
        Outcome::Success
    } else {
        fx.text(TID_GAME_BLESSEDNESS_CANCEL);
        Outcome::Refused
    }
}

/// `OnPickupPetAwakeningCancel{item}` at the pet-awakening-cancel NPC: free; the eat pet loses its lines
/// (`DoUseItemTarget_InitializeRandomOption`, the text is the awakening-cancel one in the original too) or the
/// player gets `TID_GAME_PICKUP_PET_AWAKENING_CANCEL_E001`. With `features.only_armor_awakes` no item has the
/// eat-pet kind, so the answer is always the error text.
pub fn on_eat_pet_cancel(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::PickupPetAwakeningCancel,
    npc_near: bool,
    hooks: &super::UpgradeHooks,
    fx: &mut Fx,
) -> Outcome {
    if !npc_near {
        return Outcome::Ignored;
    }
    let Some(id) = usable(m, msg.obj_id) else {
        return Outcome::Ignored;
    };
    let ok = initialize_on(
        m,
        cx,
        id,
        RandomOptKind::EatPet,
        TID_GAME_PICKUP_PET_AWAKENING_CANCEL_E001,
        None,
        hooks,
        fx,
    );
    if ok {
        fx.mark_urgent();
        super::txn::feedback(m, fx, true);
        Outcome::Success
    } else {
        Outcome::Refused
    }
}

/// `true` while the item is the summoned system pet or the summoned eat pet (`IsUsing`).
pub fn is_using(m: &Mover, id: StorageId) -> bool {
    m.player().is_some_and(|p| {
        p.items.pet_obj_id == Some(id) || p.items.summoned_eat_pet.is_some_and(|(i, _)| i == id)
    })
}

/// `DoUseItemTarget_GenRandomOption`: the kind and expiry checks (`mismatch` text), the "already has lines"
/// check (not for blessings and eat pets, which can be overwritten; `has_option` text), the `ok` text, then the
/// generation. An item with the protected-awakening flag keeps its old lines, gets the flag cleared and offers
/// the new word as the pending choice (`ItemSelectAwakeningValueNtf`); any other item is re-rolled in place.
/// Returns `true` when the scroll is to be consumed.
#[allow(clippy::too_many_arguments)]
pub fn generate_on(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    id: StorageId,
    kind: RandomOptKind,
    ok_text: u32,
    mismatch_text: u32,
    has_option_text: u32,
    scroll: Option<u32>,
    hooks: &super::UpgradeHooks,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> bool {
    let Some(item) = item_at(m, id).cloned() else {
        return false;
    };
    let Some(tp) = cx.prop(item.item_id) else {
        return false;
    };
    if awakening_kind(cx, &item) != Some(kind) || item.is_expired() {
        fx.text(mismatch_text);
        return false;
    }
    let overwrite = matches!(kind, RandomOptKind::Blessing | RandomOptKind::EatPet);
    if !overwrite && item.random_option.line_count() > 0 {
        fx.text(has_option_text);
        return false;
    }
    if ok_text > 0 {
        fx.text(ok_text);
    }
    let using = is_using(m, id);
    if using
        && kind == RandomOptKind::EatPet
        && let Some(f) = hooks.pet_options_reset
    {
        f(m, cx, id, fx);
    }
    let mut word = item.random_option;
    let tables = &cx.env.data().random_option;
    let mut pending = None;
    if word.safe_flag() {
        word.set_safe_flag(false);
        let mut fresh = RandomOptWord(0);
        gen_random_option(tables, kind, tp.parts, &mut fresh, true, rng);
        pending = Some(fresh);
    } else {
        word.initialize();
        gen_random_option(tables, kind, tp.parts, &mut word, false, rng);
    }
    if let Some(p) = pending {
        if let Some(it) = m
            .player_mut()
            .and_then(|pl| pl.items.inventory.get_at_id_mut(id))
        {
            it.pending_random_option = Some(p);
        }
        fx.out.to_self(pb::ItemSelectAwakeningValueNtf {
            item_obj_id: u32::from(id),
            serial: item.serial.0,
            new_random_option: p.to_i64(),
        });
    }
    ops::update_item_ex(m, cx.env, id, word, fx);
    if using
        && matches!(kind, RandomOptKind::SystemPet | RandomOptKind::EatPet)
        && let Some(f) = hooks.pet_options_set
    {
        f(m, cx, id, fx);
    }
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "awaken",
        if pending.is_some() {
            "protected"
        } else {
            "scroll"
        },
        "success",
        &item,
        after.as_ref(),
        scroll.map(|s| material(s, 1)).into_iter().collect(),
    );
    true
}

/// `DoUseItemTarget_InitializeRandomOption`: an item of `kind` with lines loses them; with an
/// `II_SYS_SYS_SCR_AWAKESAFE` buff on an awakening the lines are kept and the protected-awakening flag is set
/// instead (the buff is consumed). The player always gets `TID_GAME_AWAKECANCEL_INFO` (the original sends it
/// for every kind, whatever the caller passed as the success text). A kind mismatch or an item without lines
/// answers `error_text` and returns `false`.
#[allow(clippy::too_many_arguments)]
pub fn initialize_on(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    id: StorageId,
    kind: RandomOptKind,
    error_text: u32,
    scroll: Option<u32>,
    hooks: &super::UpgradeHooks,
    fx: &mut Fx,
) -> bool {
    let Some(item) = item_at(m, id).cloned() else {
        return false;
    };
    if awakening_kind(cx, &item) != Some(kind) || item.random_option.line_count() == 0 {
        fx.text(error_text);
        return false;
    }
    let using = is_using(m, id);
    if using
        && matches!(kind, RandomOptKind::SystemPet | RandomOptKind::EatPet)
        && let Some(f) = hooks.pet_options_reset
    {
        f(m, cx, id, fx);
    }
    let mut word = item.random_option;
    let protect = kind == RandomOptKind::Awakening && has_item_buff(m, II_SYS_SYS_SCR_AWAKESAFE);
    if protect {
        word.set_safe_flag(true);
        remove_item_buff(m, cx, II_SYS_SYS_SCR_AWAKESAFE, fx);
        // the success text the original assigns and never sends (`nOk`), then the one it does send
        fx.text(TID_GAME_REGARDLESS_USE04);
    } else {
        word.initialize();
    }
    ops::update_item_ex(m, cx.env, id, word, fx);
    fx.text(TID_GAME_AWAKECANCEL_INFO);
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "awaken_cancel",
        if protect { "protected" } else { "scroll" },
        "success",
        &item,
        after.as_ref(),
        scroll.map(|s| material(s, 1)).into_iter().collect(),
    );
    true
}

/// `OnItemSelectAwakeningValue{item, serial, flag}` (Plan/19 §12.6.4). The item must exist and be usable, the
/// serial must match and a pending word must exist; `flag 1` keeps the old lines (the pending word is
/// discarded), `flag 2` takes the new ones (`word = (word & 0xC000_0000_0000_00FF) | new`). Any other flag
/// changes nothing and keeps the pending word.
pub fn on_select_value(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::ItemSelectAwakeningValueReq,
    fx: &mut Fx,
) -> Outcome {
    let Some(id) = usable(m, msg.item_obj_id) else {
        return Outcome::Ignored;
    };
    let Some(before) = item_at(m, id).cloned() else {
        return Outcome::Ignored;
    };
    if before.serial.0 != msg.serial {
        return Outcome::Ignored;
    }
    let Some(new) = before.pending_random_option.filter(|w| w.0 != 0) else {
        return Outcome::Ignored;
    };
    let word = match msg.select_flag {
        1 => before.random_option,
        2 => RandomOptWord((before.random_option.0 & RandomOptWord::KEEP_MASK) | new.0),
        _ => return Outcome::Ignored,
    };
    fx.mark_urgent();
    if let Some(it) = m
        .player_mut()
        .and_then(|p| p.items.inventory.get_at_id_mut(id))
    {
        it.pending_random_option = None;
    }
    ops::update_item_ex(m, cx.env, id, word, fx);
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "awaken_select",
        if msg.select_flag == 2 { "new" } else { "old" },
        "success",
        &before,
        after.as_ref(),
        vec![],
    );
    Outcome::Success
}

/// The `ItemSelectAwakeningValueNtf` of every pending protected awakening of the player (PL-079): the item, its
/// serial and the offered word. Sent after the join batch and after a re-attach.
pub fn pending_messages(m: &Mover) -> Vec<pb::ItemSelectAwakeningValueNtf> {
    let Some(p) = m.player() else {
        return Vec::new();
    };
    p.items
        .inventory
        .iter()
        .filter_map(|(id, it)| {
            let w = it.pending_random_option.filter(|w| w.0 != 0)?;
            Some(pb::ItemSelectAwakeningValueNtf {
                item_obj_id: u32::from(id),
                serial: it.serial.0,
                new_random_option: w.to_i64(),
            })
        })
        .collect()
}

/// [`pending_messages`] into an effect list (the owner is bound).
pub fn resend_pending(m: &Mover, fx: &mut Fx) {
    for msg in pending_messages(m) {
        fx.out.to_self(msg);
    }
}

/// `OnRandomOptionRetry{item, scroll, line}` - the Scroll of Augmentation (Plan/19 §12.6.7,
/// `features.randomoption_retry`). Silent unless the original answered: a worn target (`TID_GAME_EQUIPPUT`), an
/// egg while a system pet is active (`TID_MMI_RANDOMOPTIONRETRY_ERRORTEXT05`). Needs an awakening item with 2 or
/// 3 lines and, **FIX VR-023**, `line` in `1..=line count` (the original added a third line for `line = 3` on
/// a two-line item and wiped every line for any other number). A fresh full awakening is generated and its
/// first line replaces line `line`; the order of the others is kept. Consumes the scroll.
pub fn on_retry(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::RandomOptionRetry,
    rng: &mut dyn GameRng,
    fx: &mut Fx,
) -> Outcome {
    if !cx.env.cfg().randomoption_retry {
        return Outcome::Ignored;
    }
    let (Some(id), Some(scroll)) = (usable(m, msg.item_obj_id), usable(m, msg.scroll_obj_id))
    else {
        return Outcome::Ignored;
    };
    if id == scroll {
        return Outcome::Ignored;
    }
    let (Some(before), Some(scroll_item)) = (item_at(m, id).cloned(), item_at(m, scroll).cloned())
    else {
        return Outcome::Ignored;
    };
    if scroll_item.item_id != II_SYS_SYS_SCR_RANDOMOPTIONRTRY {
        return Outcome::Ignored;
    }
    if equipped(m, id) {
        fx.text(TID_GAME_EQUIPPUT);
        return Outcome::Refused;
    }
    let Some(tp) = cx.prop(before.item_id) else {
        return Outcome::Ignored;
    };
    if tp.item_kind3 == IK3_EGG && m.player().is_some_and(|p| p.items.pet_obj_id.is_some()) {
        fx.text(TID_MMI_RANDOMOPTIONRETRY_ERRORTEXT05);
        return Outcome::Refused;
    }
    if awakening_kind(cx, &before) != Some(RandomOptKind::Awakening) {
        return Outcome::Ignored;
    }
    let count = before.random_option.line_count();
    if !(2..=3).contains(&count) {
        return Outcome::Ignored;
    }
    let Some(line) = usize::try_from(msg.line)
        .ok()
        .filter(|l| (1..=count).contains(l))
    else {
        return Outcome::Ignored;
    };
    let mut fresh = RandomOptWord(0);
    gen_random_option(
        &cx.env.data().random_option,
        RandomOptKind::Awakening,
        tp.parts,
        &mut fresh,
        false,
        rng,
    );
    let Some(new_line) = fresh.line(0) else {
        // no table: nothing to roll with, nothing is consumed
        return Outcome::Ignored;
    };
    let old: Vec<RandomOptLine> = before.random_option.lines().collect();
    let mut word = before.random_option;
    word.initialize();
    for (i, l) in old.iter().enumerate() {
        let pick = if i + 1 == line { new_line } else { *l };
        let _ = word.push_line(pick);
    }
    fx.mark_urgent();
    ops::update_item_ex(m, cx.env, id, word, fx);
    take_one(m, cx, scroll, fx);
    fx.text(TID_GAME_SMELT_SAFETY_SUCCESS);
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "awaken_retry",
        "scroll",
        "success",
        &before,
        after.as_ref(),
        vec![
            material(scroll_item.item_id, 1),
            serde_json::json!({ "line": line }),
        ],
    );
    Outcome::Success
}

/// `OnRemoveItemLevelDown{item}`: free, no NPC. An item without a level-down answers
/// `TID_GAME_INVALID_TARGET_ITEM`; otherwise both level-down bits are cleared.
pub fn on_remove_level_down(
    m: &mut Mover,
    cx: &UpgradeCx<'_>,
    msg: &pb::RemoveItemLevelDown,
    fx: &mut Fx,
) -> Outcome {
    let Some(id) = usable(m, msg.item_obj_id) else {
        return Outcome::Ignored;
    };
    let Some(before) = item_at(m, id).cloned() else {
        return Outcome::Ignored;
    };
    if before.random_option.level_down().levels() == 0 {
        fx.text(TID_GAME_INVALID_TARGET_ITEM);
        return Outcome::Refused;
    }
    fx.mark_urgent();
    let mut word = before.random_option;
    word.set_level_down(flyff_rules::item::random_option::LevelDown::None);
    ops::update_item_ex(m, cx.env, id, word, fx);
    let after = item_at(m, id).cloned();
    audit(
        fx,
        "level_down_remove",
        "free",
        "success",
        &before,
        after.as_ref(),
        vec![],
    );
    Outcome::Success
}
