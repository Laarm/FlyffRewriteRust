//! Alphabet / find-word (five letter cards spell the offered word; Plan/22 §16.3; src:_Common/MiniGame.cpp:287-431,
//! src:WORLDSERVER/DPSrvr.cpp:9624-9654).
//!
//! `AlphabetOpenWnd` offers a random word of `MiniGame_Alphabet.txt` with one letter shown. `AlphabetStart` hands in
//! five cards `II_SYS_SYS_EVE_A_CARD ..= Z_CARD`: the penya fee (`PENYA`, shipped 50 000) and the five cards are
//! consumed BEFORE the word is compared (KEEP), a right word pays the weighted prize of the word.
//! Answers: `998` = not enough penya, `999` = refused or wrong, else the question id with the prize.
//!
//! FIX: the original accepted any word index (not only the offered one) and any item as a card (it counted every
//! non-card item as an 'A'); both are refused now, with nothing consumed.

use super::deliver::{audit_bet, give_prize};
use super::pick::weighted_pick;
use super::{reply, usable_item, wallet};
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::StorageId;
use crate::item::fx::Fx;
use crate::item::gold::add_gold;
use crate::item::ops::{RemoveReason, remove_item};
use flyff_common::defs::item::{II_SYS_SYS_EVE_A_CARD, II_SYS_SYS_EVE_Z_CARD};
use flyff_common::defs::text::{TID_GAME_REAPITEM, TID_MMI_FINDWORD};
use flyff_common::rng::GameRng;
use flyff_proto::pb::mini_game::{AlphabetOpenWnd, AlphabetResult, Sub};
use serde_json::json;
use std::collections::BTreeMap;

/// `ALPHABET_NOTENOUGH_MONEY`.
pub const NOT_ENOUGH_MONEY: i32 = 998;
/// `ALPHABET_FAILED`.
pub const FAILED: i32 = 999;
/// The number of cards of one try.
pub const CARDS: usize = 5;

/// `CMiniGame::GetCharacter`: the letter of a card item.
fn letter_of(item_id: u32) -> Option<char> {
    (II_SYS_SYS_EVE_A_CARD..=II_SYS_SYS_EVE_Z_CARD)
        .contains(&item_id)
        .then(|| char::from(b'A' + u8::try_from(item_id - II_SYS_SYS_EVE_A_CARD).unwrap_or(0)))
}

/// `AlphabetOpenWnd` (after the NPC check): a random word is offered and remembered. No words: nothing. The shown
/// position is clamped to the word (FIX: the original indexed out of bounds).
pub(crate) fn open_wnd(svc: &mut Services, ctx: &mut GameCtx<'_>, who: PlayerRef) {
    let words = &ctx.data.minigames.alphabet;
    let Ok(n) = u32::try_from(words.len()) else {
        return;
    };
    if n == 0 {
        return;
    }
    let q = ctx.rng.event().random_below(n) as usize;
    let Some(w) = words.get(q) else { return };
    let bytes = w.word.as_bytes();
    if bytes.is_empty() {
        return;
    }
    let pos = usize::try_from(w.public).unwrap_or(1).clamp(1, bytes.len());
    let ch = bytes.get(pos - 1).copied().unwrap_or(b'A');
    svc.event.minigames.player_mut(who.0).alphabet_question = Some(q);
    let mut fx = Fx::new();
    reply(
        &mut fx,
        Sub::AlphabetOpenWnd(AlphabetOpenWnd {
            question_id: i32::try_from(q).unwrap_or(0),
            public_pos: i32::try_from(pos - 1).unwrap_or(0),
            public_char: i32::from(ch),
            question: w.question.clone(),
        }),
    );
    fx.flush(svc, ctx, who);
}

/// The five card slots as `(storage id, letter)`, or `None` when a slot is no own usable letter card or a stack is
/// used more often than it holds pieces.
fn check_cards(ctx: &GameCtx<'_>, who: PlayerRef, ids: &[u32]) -> Option<Vec<(StorageId, char)>> {
    let mut uses: BTreeMap<StorageId, u32> = BTreeMap::new();
    let mut slots = Vec::with_capacity(ids.len());
    for obj_id in ids {
        let (id, item_id, count) = usable_item(ctx, who, *obj_id)?;
        let letter = letter_of(item_id)?;
        let n = uses.entry(id).or_default();
        *n += 1;
        if *n > u32::from(count) {
            return None;
        }
        slots.push((id, letter));
    }
    Some(slots)
}

fn result(fx: &mut Fx, result: i32, item_id: u32, count: i32) {
    reply(
        fx,
        Sub::AlphabetResult(AlphabetResult {
            result,
            item_id,
            count,
        }),
    );
}

/// `AlphabetStart` (after the NPC check): see the module documentation.
pub(crate) fn start(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    question_id: i32,
    ids: &[u32],
) {
    // VR-064: five card ids
    if ids.len() != CARDS {
        return;
    }
    let data = ctx.data;
    let tables = &data.minigames;
    let pay = i64::from(tables.alphabet_pay_penya.max(0));
    let mut fx = Fx::new();
    if wallet(ctx, who) < pay {
        result(&mut fx, NOT_ENOUGH_MONEY, 0, 0);
        fx.flush(svc, ctx, who);
        return;
    }
    let offered = svc.event.minigames.player_mut(who.0).alphabet_question;
    let question = usize::try_from(question_id)
        .ok()
        .filter(|q| Some(*q) == offered && *q < tables.alphabet.len());
    let cards = check_cards(ctx, who, ids);
    let (Some(q), Some(cards)) = (question, cards) else {
        result(&mut fx, FAILED, 0, 0);
        fx.flush(svc, ctx, who);
        return;
    };
    // the fee and the cards are paid before the word is compared (KEEP)
    if pay > 0 {
        add_gold(svc, ctx, who, -pay, true);
        audit_bet(
            &mut fx,
            "alphabet",
            "fee",
            json!({"penya": pay, "question": q}),
        );
    }
    for (id, _) in &cards {
        remove_item(svc, ctx, who, *id, 1, RemoveReason::Other("alphabet"));
    }
    let word: String = cards.iter().map(|(_, c)| *c).collect();
    let Some(entry) = tables.alphabet.get(q) else {
        result(&mut fx, FAILED, 0, 0);
        fx.mark_urgent();
        fx.flush(svc, ctx, who);
        return;
    };
    if word != entry.word {
        audit_bet(
            &mut fx,
            "alphabet",
            "wrong",
            json!({"question": q, "word": word}),
        );
        result(&mut fx, FAILED, 0, 0);
        fx.mark_urgent();
        fx.flush(svc, ctx, who);
        return;
    }
    let r = ctx.rng.event().random_below(1_000_000);
    let answer = i32::try_from(q).unwrap_or(0);
    let (mut item, mut count) = (0, 0);
    if let Some(p) = weighted_pick(&entry.items, r).copied() {
        let given = give_prize(svc, ctx, who, p.item, p.count, TID_MMI_FINDWORD, &mut fx);
        if given == Some(super::deliver::Delivery::Inventory)
            && let Some(prop) = data.item(p.item)
        {
            fx.defined(TID_GAME_REAPITEM, vec![format!("\"{}\"", prop.name)]);
        }
        if given.is_some() {
            (item, count) = (p.item, p.count);
        }
    }
    audit_bet(
        &mut fx,
        "alphabet",
        "prize",
        json!({"question": q, "roll": r, "item": item, "count": count}),
    );
    result(&mut fx, answer, item, count);
    fx.mark_urgent();
    fx.flush(svc, ctx, who);
}
