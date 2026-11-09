//! Couples (Plan/21 §11, WP-SOCIAL-21; `src:WORLDSERVER/couplehelper.cpp`, `src:databaseserver/couplehelper.cpp`,
//! `src:_Common/couple.cpp`, `src:WORLDSERVER/User.cpp:3783-3880`).
//!
//! * [`CoupleService`] holds the couples and the proposal cooldowns the database loaded at start; the game thread is
//!   the only writer, every change goes to the database in the same commit block (`UpsertCouple`, `DeleteCouple`,
//!   `SetCoupleProposal`);
//! * [`flow`]: `Propose`, `Refuse`, `CoupleReq` (accept) and `Decouple`, in the order of the original checks;
//! * [`exp`]: the cadence step of a player (the 61-step tick of the higher id, the level buffs), `add_experience`
//!   and the level rewards mailed through the system mail;
//! * [`join`]: `CoupleNtf` in the join batch.
//!
//! The pending proposal is `PlayerSocial.proposer` of the TARGET (no TTL, replaced by a newer proposal, cleared on
//! refuse and accept, gone with the avatar). The pure rules (`couple_level`, `couple_buff_items`, ...) are
//! `flyff_rules::social`.

pub mod exp;
pub mod flow;
pub mod join;

use crate::ctx::{GameCtx, Services};
use flyff_common::{CharacterId, UnixTime};
use flyff_db::StartupState;
use flyff_rules::social::couple_level;
use std::collections::BTreeMap;

pub use exp::add_experience;
pub use flow::{accept, decouple, propose, refuse};

/// One couple (`CCouple`). `first` is the member with the HIGHER character id: it sends the experience tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Couple {
    /// `couples.id`.
    pub id: u32,
    /// Couple minutes (`m_nExperience`).
    pub experience: i32,
    /// The member with the higher id.
    pub first: CharacterId,
    /// The member with the lower id.
    pub second: CharacterId,
    /// When the couple formed.
    pub created_at: UnixTime,
}

impl Couple {
    /// The partner of `who`; `None` when `who` is no member.
    pub fn partner_of(&self, who: CharacterId) -> Option<CharacterId> {
        if who == self.first {
            Some(self.second)
        } else if who == self.second {
            Some(self.first)
        } else {
            None
        }
    }

    /// The couple level of the current experience (`GetLevel`, `level_exp` = `GameData.couple.level_exp`).
    pub fn level(&self, level_exp: &[i32]) -> i32 {
        couple_level(level_exp, self.experience)
    }

    /// The stored form.
    pub fn record(&self) -> flyff_db::CoupleRecord {
        flyff_db::CoupleRecord {
            id: self.id,
            experience: self.experience,
            created_at: self.created_at,
            members: [self.first, self.second],
        }
    }
}

/// The couples and the proposal cooldowns (`Services.social.couple`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoupleService {
    couples: BTreeMap<u32, Couple>,
    by_member: BTreeMap<CharacterId, u32>,
    cooldowns: BTreeMap<CharacterId, UnixTime>,
    next_id: u32,
}

impl CoupleService {
    /// The couples and cooldowns of `StartupState` (a couple whose members are not two different characters is
    /// skipped with a warning; the database refuses such a row anyway).
    pub fn load(startup: &StartupState) -> CoupleService {
        let mut s = CoupleService {
            next_id: startup.counters.next_couple_id.max(1),
            ..CoupleService::default()
        };
        for rec in &startup.couples {
            let [first, second] = rec.members;
            if first == second
                || s.by_member.contains_key(&first)
                || s.by_member.contains_key(&second)
            {
                tracing::warn!(
                    couple = rec.id,
                    "couple skipped: a member is invalid or coupled twice"
                );
                continue;
            }
            s.insert(Couple {
                id: rec.id,
                experience: rec.experience,
                first,
                second,
                created_at: rec.created_at,
            });
        }
        for p in &startup.couple_proposals {
            s.cooldowns.insert(p.proposer_id, p.cooldown_until);
        }
        s
    }

    fn insert(&mut self, c: Couple) {
        self.next_id = self.next_id.max(c.id.saturating_add(1));
        self.by_member.insert(c.first, c.id);
        self.by_member.insert(c.second, c.id);
        self.couples.insert(c.id, c);
    }

    /// Allocates the id of the next couple (`meta.next_couple_id`; the database raises the counter with the
    /// couple's batch).
    pub(crate) fn alloc_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }

    /// Forms a couple (the member with the higher id becomes `first`) and returns it.
    pub(crate) fn couple(
        &mut self,
        id: u32,
        a: CharacterId,
        b: CharacterId,
        now: UnixTime,
    ) -> Couple {
        let (first, second) = if a >= b { (a, b) } else { (b, a) };
        let c = Couple {
            id,
            experience: 0,
            first,
            second,
            created_at: now,
        };
        self.insert(c.clone());
        c
    }

    /// Ends the couple of `member`; returns it.
    pub(crate) fn remove_of(&mut self, member: CharacterId) -> Option<Couple> {
        let id = self.by_member.get(&member).copied()?;
        let c = self.couples.remove(&id)?;
        self.by_member.remove(&c.first);
        self.by_member.remove(&c.second);
        Some(c)
    }

    /// Adds `n` to the experience of the couple `id` (saturating); returns the couple afterwards.
    pub(crate) fn add_exp(&mut self, id: u32, n: i32) -> Option<&Couple> {
        let c = self.couples.get_mut(&id)?;
        c.experience = c.experience.saturating_add(n);
        Some(&*c)
    }

    /// The couple `member` belongs to.
    pub fn get(&self, member: CharacterId) -> Option<&Couple> {
        self.by_member
            .get(&member)
            .and_then(|id| self.couples.get(id))
    }

    /// The partner of `member`.
    pub fn partner_of(&self, member: CharacterId) -> Option<CharacterId> {
        self.get(member).and_then(|c| c.partner_of(member))
    }

    /// The couple level of `member` (`level_exp` = `GameData.couple.level_exp`); `None` when not coupled.
    pub fn level_of(&self, level_exp: &[i32], member: CharacterId) -> Option<i32> {
        self.get(member).map(|c| c.level(level_exp))
    }

    /// Number of couples.
    pub fn len(&self) -> usize {
        self.couples.len()
    }

    /// `true` when there is no couple.
    pub fn is_empty(&self) -> bool {
        self.couples.is_empty()
    }

    /// The couples in id order.
    pub fn couples(&self) -> impl Iterator<Item = &Couple> {
        self.couples.values()
    }

    /// The end of the proposal cooldown of `proposer`, if one is running in memory.
    pub fn cooldown_of(&self, proposer: CharacterId) -> Option<UnixTime> {
        self.cooldowns.get(&proposer).copied()
    }

    /// Stores (`Some`) or drops (`None`) a cooldown in memory.
    pub(crate) fn set_cooldown(&mut self, proposer: CharacterId, until: Option<UnixTime>) {
        match until {
            Some(t) => {
                self.cooldowns.insert(proposer, t);
            }
            None => {
                self.cooldowns.remove(&proposer);
            }
        }
    }

    /// Drops every cooldown with `cooldown_until < now` (`CCoupleHelper::OnTimer`: `t > i->second`); returns how
    /// many went. The database trims its rows in the daily job.
    pub fn expire(&mut self, now: UnixTime) -> usize {
        let before = self.cooldowns.len();
        self.cooldowns.retain(|_, until| *until >= now);
        before - self.cooldowns.len()
    }

    /// `OnClearPropose` (`/ncl`): forgets every cooldown and returns the proposers whose stored row must go
    /// (the caller sends `SetCoupleProposal{proposer_id, None}` for each).
    pub fn clear_cooldowns(&mut self) -> Vec<CharacterId> {
        let ids: Vec<CharacterId> = self.cooldowns.keys().copied().collect();
        self.cooldowns.clear();
        ids
    }

    /// The database purge in memory: couples with a purged member and the cooldowns of purged characters vanish.
    /// Returns the surviving partners of the couples that went (an online partner is told by the caller).
    pub fn purge(&mut self, ids: &[CharacterId]) -> Vec<CharacterId> {
        let mut partners = Vec::new();
        for &id in ids {
            self.cooldowns.remove(&id);
            if let Some(c) = self.remove_of(id)
                && let Some(p) = c.partner_of(id)
            {
                partners.push(p);
            }
        }
        partners.retain(|p| !ids.contains(p));
        partners
    }
}

/// Periodic job P6 "couple minute" (21 §11.3, 10 §5.3): proposal cooldowns whose time is up leave the memory.
pub fn expire(svc: &mut Services, ctx: &mut GameCtx<'_>) {
    let now = ctx.clock.unix_ms.to_unix_time();
    svc.social.couple.expire(now);
}
