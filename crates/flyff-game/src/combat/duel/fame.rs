//! The fame tables of the duel results (Plan/17 §8.6-§8.7; `CMover::ChangeFame`, `MoverParam.cpp:754-796`, and the
//! party branch of `CMover::DuelResult`, `MoverParam.cpp:810-845`).

/// The solo duel: `gap = loser level - winner level` -> `(fame for the winner, percent of its fame the loser
/// loses)`. A loser far above the winner is worth more.
///
/// | gap | winner | loser |
/// |---|---|---|
/// | `>= 80` | 100 | 15 % |
/// | `>= 30` | 50 | 10 % |
/// | `>= 20` | 30 | 8 % |
/// | `>= 10` | 10 | 6 % |
/// | `<= -80` | 1 | 1 % |
/// | `<= -30` | 2 | 2 % |
/// | `<= -20` | 3 | 3 % |
/// | `<= -10` | 4 | 4 % |
/// | otherwise | 5 | 5 % |
pub fn solo_fame(gap: i32) -> (i32, f32) {
    if gap >= 80 {
        (100, 15.0)
    } else if gap >= 30 {
        (50, 10.0)
    } else if gap >= 20 {
        (30, 8.0)
    } else if gap >= 10 {
        (10, 6.0)
    } else if gap <= -80 {
        (1, 1.0)
    } else if gap <= -30 {
        (2, 2.0)
    } else if gap <= -20 {
        (3, 3.0)
    } else if gap <= -10 {
        (4, 4.0)
    } else {
        (5, 5.0)
    }
}

/// The party duel: `gap = losing leader level - winning leader level` -> `(fame for every winner, percent every
/// loser loses)`. The steps are 80 / 50 / 20 / 10 (the solo table has an extra 30).
pub fn party_fame(gap: i32) -> (i32, f32) {
    if gap >= 80 {
        (100, 15.0)
    } else if gap >= 50 {
        (50, 10.0)
    } else if gap >= 20 {
        (30, 8.0)
    } else if gap >= 10 {
        (10, 6.0)
    } else if gap <= -80 {
        (1, 1.0)
    } else if gap <= -50 {
        (2, 2.0)
    } else if gap <= -20 {
        (3, 3.0)
    } else if gap <= -10 {
        (4, 4.0)
    } else {
        (5, 5.0)
    }
}

/// What a loser loses: `(int)(fame * percent / 100.0f)` (`f32`); the solo branch raises 0 and below to 1, the
/// party branch only a 0 (`solo` selects the rule).
pub fn fame_loss(fame: i32, percent: f32, solo: bool) -> i32 {
    let ratio = percent / 100.0_f32;
    let dec = (fame as f32 * ratio) as i32;
    if solo {
        if dec <= 0 { 1 } else { dec }
    } else if dec == 0 {
        1
    } else {
        dec
    }
}
