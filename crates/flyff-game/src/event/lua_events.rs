//! `EventLuaService`: the Event.lua engine on top of the `Event` Lua family (Plan/22 §4; ref/15 §4).
//!
//! What lives here (WP-NPC-02's consumer side and the Lua part of WP-EVENT-04/05):
//!
//! * the **state pass** (§4.2): `GetEventState()` flips `tEvent[i].State` by comparing the wall minute with the
//!   time windows; the returned changes are reported as [`EventChange`]s,
//! * the **minute evaluation** ([`EventLuaService::minute`], P9 of the scheduler): for every wall minute
//!   crossed since the previous run (at most 5, 22 §2.4 rule 4) the Lua clock is pinned to the start of that
//!   minute, so a minute replayed after a stall is evaluated with its own time and `IsNoticeTime`'s exact
//!   `"%a %H:%M"` comparison never misses one; the auto-notice check comes first, then the state pass,
//! * the **proxy** (§4.3): after a change, a reload or the first pass at start the aggregates are cached in
//!   [`EventModifiers`]; gameplay reads [`EventLuaService::modifiers`] and never calls Lua for these,
//! * the **level-up gifts** ([`EventLuaService::level_up_gifts`], §4.6) and the **Lua event items**
//!   ([`EventLuaService::lua_event_items`], §4.7), both resolved to item ids.
//!
//! A family that is `Missing` or `Faulted` gives [`EventModifiers::NEUTRAL`], no gifts and no event items
//! (legacy `IsPossible() == FALSE`, PL-106); the host contains the error and the game goes on at plain rates.
//!
//! Not here: the broadcast of `EventLuaDesc` / `GameRate` (the messages arrive with the protocol WPs; the
//! pass reports the changes with their descriptions so the broadcaster has what it needs) and the spawn events
//! ([`super::spawns`]).
//!
//! **Coupons and keep-connect mail (22 §4.8, §4.9, WP-EVENT-06).** The 1 s timer of the world phase
//! ([`super::EventSecondHooks`]) has no `Services`: it reads the intervals from the [`DescBoard`] facts that every
//! [`EventLuaService::refresh`] stores, counts the coupon itself (`PlayerMisc.coupon_count`, `EventCoupon`) and
//! sends the character of a due keep-connect tick over the board's channel. [`keep_connect_drain`] (P15) receives
//! them and mails the items.

use super::desc::DescBoard;
use super::modifiers::EventModifiers;
use super::scripts::{EventScripts, WarnOnce};
use crate::ctx::{GameCtx, Services};
use crate::econ::mail::{MailEntry, commit::on_send_result};
use crate::item::container::SerialSource;
use crate::item::model::Item;
use crate::item::ops::CtxSerials;
use crate::player::PlayerMisc;
use crate::sched::wall::MinuteCursor;
use crate::tickets::{Pending, ServiceTicket, TicketResult};
use flyff_common::{CharacterId, GameMs, ItemSerial, MailId, UnixMs};
use flyff_data::GameData;
use flyff_data::types::ItemId;
use flyff_db::{AtomicBatch, AtomicKind, DbOp};
use flyff_lua::{FamilyStatus, LuaHost, ScriptFamily};
use std::collections::BTreeMap;

pub use super::scripts::LevelUpGift;

const EVENT: ScriptFamily = ScriptFamily::Event;

/// One state change returned by `GetEventState` (ids are the 1-based insertion order of `AddEvent`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventChange {
    /// The event id (`tEvent` index).
    pub id: u32,
    /// `true` = the event started, `false` = it ended.
    pub running: bool,
    /// `GetDesc(id)`: what the clients are told (`EventLuaDesc`).
    pub desc: String,
}

/// A running event (`GetEventList`) with its description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningEvent {
    /// The event id.
    pub id: u32,
    /// `GetDesc(id)`.
    pub desc: String,
}

/// What one run of the minute evaluation did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MinuteReport {
    /// The wall minutes that were evaluated (`unix_ms / 60 000`), ascending.
    pub minutes: Vec<i64>,
    /// The state changes in the order Lua returned them, minute by minute.
    pub changes: Vec<EventChange>,
    /// The auto-notice lines of every minute for which `IsNoticeTime()` was true (`GetNoticeMessage`).
    pub notices: Vec<Vec<String>>,
}

impl MinuteReport {
    /// `true` when nothing happened (no change, no notice).
    pub fn is_quiet(&self) -> bool {
        self.changes.is_empty() && self.notices.is_empty()
    }
}

/// The Event.lua engine (see the module documentation).
#[derive(Debug, Default)]
pub struct EventLuaService {
    modifiers: EventModifiers,
    running: Vec<RunningEvent>,
    cursor: MinuteCursor,
    passes: u64,
    last_report: MinuteReport,
    warned: WarnOnce,
    board: DescBoard,
    /// The `Event` family was `Ready` at the last refresh (what the 1 s timer is told).
    ready: bool,
    /// The characters whose keep-connect mail is due and not yet sent (`m_vecKeepConnectUser`), in arrival order;
    /// the drain pops from the back.
    keep_pending: Vec<CharacterId>,
}

impl EventLuaService {
    /// The typed wrappers over the `Event` family with this service's warn-once memory (22 §2.5).
    pub fn scripts<'a>(&'a self, lua: &'a LuaHost, data: &'a GameData) -> EventScripts<'a> {
        EventScripts::new(lua, data, &self.warned)
    }

    /// The descriptions of the running events with a non-empty text, ascending by event id: what the join step and
    /// the 90 s resend send as `EventLuaDesc` `state` 2 (22 §4.12).
    pub fn descriptions(&self) -> Vec<String> {
        let mut v: Vec<&RunningEvent> =
            self.running.iter().filter(|r| !r.desc.is_empty()).collect();
        v.sort_by_key(|r| r.id);
        v.into_iter().map(|r| r.desc.clone()).collect()
    }

    /// Adopts the board the server built (`Handlers.event_desc`) and publishes the running events to it.
    pub fn set_board(&mut self, board: DescBoard) {
        self.board = board;
        self.publish_board();
    }

    /// The board gets the descriptions of the running events and the facts of the 1 s timer.
    fn publish_board(&self) {
        let ready = self.ready;
        let m = &self.modifiers;
        self.board.facts().store(
            ready,
            if ready { m.coupon_ms } else { 0 },
            if ready { m.keep_connect_ms } else { 0 },
        );
        self.board.publish(self.descriptions());
    }

    /// The characters whose keep-connect mail waits for [`keep_connect_drain`] (tests).
    pub fn keep_connect_pending(&self) -> usize {
        self.keep_pending.len() + self.board.pending_keep_connect()
    }

    /// The warn-once memory of the script adapters (shared with the config readers).
    pub fn warned(&self) -> &WarnOnce {
        &self.warned
    }

    /// The cached aggregates of the running events.
    pub fn modifiers(&self) -> &EventModifiers {
        &self.modifiers
    }

    /// Stores the `propEvent.inc` factors in the cached aggregates ([`EventModifiers::generic_exp`] and
    /// [`EventModifiers::generic_item_drop`], 22 §6.3); called by `EventServices::recompute_modifiers`.
    pub(crate) fn set_generic_factors(&mut self, exp: f32, item_drop: f32) {
        self.modifiers.generic_exp = exp;
        self.modifiers.generic_item_drop = item_drop;
    }

    /// Stores the lord-event factors in the cached aggregates ([`EventModifiers::lord_exp`] and
    /// [`EventModifiers::lord_item`], `1 + sum fE` and `1 + sum fI`, 22 §9.5); called by the lord service after the
    /// load, a create, a tick and an initialize. A `refresh` keeps them (`with_lua_neutral`).
    pub(crate) fn set_lord_factors(&mut self, exp: f32, item: f32) {
        self.modifiers.lord_exp = exp;
        self.modifiers.lord_item = item;
    }

    /// The events that are running now (`GetEventList`), ascending by id.
    pub fn running(&self) -> &[RunningEvent] {
        &self.running
    }

    /// How many state passes ran (tests, `/metrics`).
    pub fn passes(&self) -> u64 {
        self.passes
    }

    /// What the last run of the minute evaluation did (the broadcaster of `EventLuaDesc` and the auto notice
    /// read it; it is replaced by the next run).
    pub fn last_report(&self) -> &MinuteReport {
        &self.last_report
    }

    /// The first pass, before the first tick (22 §4.1 step 2): the events that are due start now and the
    /// aggregates are computed, so the rates are in force from tick 0. Also consumes the current minute of the
    /// minute cursor, so the first P9 run does not repeat it. No descriptions are broadcast (legacy: the DB sent
    /// the running list without them on world connect).
    pub fn start(&mut self, lua: &LuaHost, data: &GameData, now: UnixMs) -> Vec<EventChange> {
        let _ = self.cursor.crossed(now);
        let changes = self.state_pass(lua, data, false);
        self.refresh(lua, data);
        changes
    }

    /// The minute evaluation (P9, 22 §2.3/§4.2/§4.11): for each wall minute crossed since the previous call
    /// (the first call yields the current minute only; a backwards jump yields none; after a forward jump the
    /// 5 most recent), with the Lua clock pinned to the start of the minute and unpinned afterwards: the
    /// auto-notice check, then the state pass. The aggregates are recomputed once when anything changed.
    pub fn minute(&mut self, lua: &LuaHost, data: &GameData, now: UnixMs) -> MinuteReport {
        let mut report = MinuteReport::default();
        for m in self.cursor.crossed(now) {
            report.minutes.push(m);
            if lua.family_status(EVENT) != FamilyStatus::Ready {
                continue;
            }
            lua.with_wall_pinned(UnixMs(m.saturating_mul(60_000)), |h| {
                let scripts = EventScripts::new(h, data, &self.warned);
                if scripts.is_notice_time() {
                    report.notices.push(scripts.notice_messages());
                }
                report.changes.extend(self.state_pass(h, data, true));
            });
        }
        if !report.changes.is_empty() || lua.family_status(EVENT) != FamilyStatus::Ready {
            self.refresh(lua, data);
        }
        self.last_report = report.clone();
        report
    }

    /// After a successful reload of the Event family (22 §4.1 step 3, 20 §3.9): the Lua state restarted from its
    /// initial values, so the state pass runs again (with descriptions) and the aggregates are recomputed. A
    /// failed reload keeps the previous state; call this only for a reload that succeeded.
    pub fn on_reload(&mut self, lua: &LuaHost, data: &GameData, now: UnixMs) -> MinuteReport {
        self.warned.clear();
        let m = now.0.div_euclid(60_000);
        let mut report = MinuteReport {
            minutes: vec![m],
            ..MinuteReport::default()
        };
        if lua.family_status(EVENT) == FamilyStatus::Ready {
            lua.with_wall_pinned(UnixMs(m.saturating_mul(60_000)), |h| {
                report.changes = self.state_pass(h, data, true);
            });
        }
        self.refresh(lua, data);
        self.last_report = report.clone();
        report
    }

    /// `GetEventState()` and the descriptions of the changed events.
    fn state_pass(&mut self, lua: &LuaHost, data: &GameData, text_out: bool) -> Vec<EventChange> {
        let scripts = EventScripts::new(lua, data, &self.warned);
        if !scripts.ready() {
            return Vec::new();
        }
        self.passes += 1;
        scripts
            .event_state()
            .into_iter()
            .map(|c| EventChange {
                id: c.id,
                running: c.state == 1,
                desc: if text_out {
                    scripts.desc(c.id)
                } else {
                    String::new()
                },
            })
            .collect()
    }

    /// Recomputes the aggregates and the list of running events (the legacy `CEventLuaProxy::Initialize`). The
    /// generic, lord and weather parts of the modifiers keep the values their services stored.
    pub fn refresh(&mut self, lua: &LuaHost, data: &GameData) {
        let scripts = EventScripts::new(lua, data, &self.warned);
        let mut m = self.modifiers.with_lua_neutral();
        if !scripts.ready() {
            self.modifiers = m;
            self.running.clear();
            self.ready = false;
            self.publish_board();
            return;
        }
        m.lua_exp = scripts.exp_factor();
        m.lua_item_drop = scripts.item_drop_rate();
        m.lua_piece_drop = scripts.piece_item_drop_rate();
        m.lua_gold = scripts.gold_drop_factor();
        m.lua_attack = scripts.attack_power();
        m.lua_defense = scripts.defense_power();
        m.lua_cheer_exp = scripts.cheer_exp_factor();
        // missing in the shipped EventFunc.lua (KQ-11): the wrappers give the neutral factors
        m.lua_weather_exp = scripts.weather_exp_factor();
        m.shop_buy = scripts.shop_buy_factor();
        m.shop_sell = scripts.shop_sell_factor();
        m.coupon_ms = scripts.coupon_event_ms();
        m.keep_connect_ms = scripts.keep_connect_ms();
        m.keep_connect_items = scripts.keep_connect_items();
        let running: Vec<RunningEvent> = scripts
            .event_list()
            .into_iter()
            .map(|id| RunningEvent {
                id,
                desc: scripts.desc(id),
            })
            .collect();
        // a call that faulted the family above leaves nothing but the neutral rates
        if scripts.ready() {
            self.modifiers = m;
            self.running = running;
        } else {
            self.modifiers = m.with_lua_neutral();
            self.running.clear();
        }
        self.ready = scripts.ready();
        self.publish_board();
    }

    /// `GetLevelUpGift(level, account)` (22 §4.6): the gifts for reaching `level`, in script order. Lua matches
    /// `strAccount == "all"` or `string.find(account, strAccount)`. An unknown item is skipped with a warning.
    pub fn level_up_gifts(
        &self,
        lua: &LuaHost,
        data: &GameData,
        level: u32,
        account: &str,
    ) -> Vec<LevelUpGift> {
        self.scripts(lua, data).level_up_gifts(level, account)
    }

    /// `GetItem(tick, monsterLevel)` (22 §4.7): the event items a monster kill drops now, as `(item, count)`.
    /// The C++ collected them in a `map<item, count>`: **KEEP-QUIRK** a duplicate item id keeps the first count
    /// and the result is ordered by ascending item id. The pacing state (`TimeOut`, `Skip`) lives in Lua.
    pub fn lua_event_items(
        &self,
        lua: &LuaHost,
        data: &GameData,
        game_ms: GameMs,
        monster_level: u32,
    ) -> Vec<(ItemId, i32)> {
        self.scripts(lua, data).event_items(game_ms, monster_level)
    }
}

/// The legacy `/coupon n` (22 §4.8): sets the coupon count and sends nothing (the count is saved with the next
/// snapshot). For the GM command of WP-GM-25.
pub fn set_coupons(p: &mut PlayerMisc, n: i32) {
    p.coupon_count = n;
}

/// Most characters mailed per P15 run (`KeepConnectEventProcess`).
const KEEP_CONNECT_PER_TICK: usize = 10;

/// P15 `event::lua_events::keep_connect_drain` (22 §2.3, §4.9, `KeepConnectEventProcess`): a family that is not
/// `Ready` keeps everything (the legacy returns early); otherwise the characters the 1 s timer queued join the
/// pending list in arrival order, and at most 10 are popped from the BACK (KEEP: the newest first). Each gets one
/// system mail per keep-connect item (empty title and text, the count as `(int)`, a fresh serial), all of one
/// player in ONE `AtomicBatch` (`keep_connect`, no snapshot) behind a ticket that forgets the mails the database
/// does not have when the batch fails (Plan/19 §20.3), plus one `mail.send` audit line per mail. The mail goes
/// by character id whether or not the player is still online (the C++ never checks).
pub fn keep_connect_drain(s: &mut Services, ctx: &mut GameCtx<'_>) {
    if ctx.lua.family_status(EVENT) != FamilyStatus::Ready {
        return;
    }
    let svc = &mut s.event.lua_events;
    let arrived = svc.board.take_keep_connect();
    svc.keep_pending.extend(arrived);
    if svc.keep_pending.is_empty() {
        return;
    }
    let items = keep_connect_items(svc);
    let mut batch = Vec::new();
    while batch.len() < KEEP_CONNECT_PER_TICK {
        let Some(who) = svc.keep_pending.pop() else {
            break;
        };
        batch.push(who);
    }
    for who in batch {
        mail_keep_connect(s, ctx, who, &items);
    }
}

/// `GetKeepConnectItem()` as the C++ held it (`map<DWORD, int>`): KEEP-QUIRK a duplicate item keeps its FIRST
/// count and the list is ordered by ascending item id. A count below 1 is skipped with one warning (a FIX: the C++
/// mailed it).
fn keep_connect_items(svc: &EventLuaService) -> Vec<(ItemId, u16)> {
    let mut map: BTreeMap<ItemId, i32> = BTreeMap::new();
    for (item, count) in &svc.modifiers.keep_connect_items {
        map.entry(*item).or_insert(*count);
    }
    map.into_iter()
        .filter_map(|(item, count)| {
            if count < 1 {
                if svc.warned.first(&format!("keep_connect_count:{item}")) {
                    tracing::warn!(
                        item,
                        count,
                        "a keep-connect item has a count below 1: it is not mailed"
                    );
                }
                return None;
            }
            Some((item, u16::try_from(count).unwrap_or(u16::MAX)))
        })
        .collect()
}

/// The mails of one player: one system mail per item, one batch.
fn mail_keep_connect(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: CharacterId,
    items: &[(ItemId, u16)],
) {
    let mut ops: Vec<DbOp> = Vec::new();
    let mut ids: Vec<MailId> = Vec::new();
    for &(item, count) in items {
        let mut it = Item::new(item, count);
        let Services { serials, .. } = &mut *s;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        match SerialSource::next_serial(&mut src) {
            Ok(n) => it.serial = ItemSerial(n),
            Err(e) => {
                tracing::error!(character = who.0, item, error = %e, "keep-connect mail: no serial, the item is not mailed");
                continue;
            }
        }
        let (id, pair) = s.econ.mail.send_system_with_id(who, "", "", Some(it), 0);
        ops.extend(pair);
        ids.push(id);
        if let Some(rec) = s.econ.mail.get(id).map(MailEntry::to_record) {
            ctx.audit.emit(flyff_db::audit::econ::mail_send(&rec, 0));
        }
    }
    if ops.is_empty() {
        return;
    }
    let batch = AtomicBatch::of_ops(AtomicKind::Other("keep_connect"), ops);
    let ticket = ctx.tickets.issue(Pending::Service(ServiceTicket::new(
        move |svc, _ctx, result| match result {
            TicketResult::Failed(e) => {
                for id in ids {
                    on_send_result(svc, id, TicketResult::Failed(e.clone()));
                }
            }
            other => {
                if let Some(first) = ids.first() {
                    on_send_result(svc, *first, other);
                }
            }
        },
    )));
    ctx.persist.atomic(batch.with_ticket(ticket));
}
