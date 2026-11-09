//! The admin commands the runtime executes itself (Plan/10 §13.1, 23 §8.2; WP-OPS-09): `ListOnline`, `Kick` of
//! characters, `Broadcast`, the rates and event flags (`GetRates`, `SetRate`, `GetEvents`, `SetEvent`) and the
//! generic runtime keys (`SetRuntime`, `UnsetRuntime`, the console words `set` / `unset`).
//!
//! The GM line (`RunLine`), the authority change and the script reload belong to the [`crate::AdminHandler`] the
//! doc-23 and doc-20 work packages install. A handler that wants to execute one of the commands below instead
//! claims it with `AdminHandler::handles`.
//!
//! Every change of the live configuration is a new [`crate::live::LiveConfig`] generation (`crate::overrides`), persisted as a
//! `runtime_override` row when `fidelity.persist_gm_overrides` is on.

use crate::event::flags::{FlagOrigin, MAX_EVENT};
use crate::input::{
    AdminActor, AdminCmd, AdminOutput, AdminTarget, EventEntry, OnlineEntry, RateEntry,
};
use crate::overrides::{
    self, Applied, MAX_FLAG_ID, OverrideError, RATE_KEYS, RateKey, RuntimeKey, canonical_key,
    event_key,
};
use crate::session::LeaveTrigger;
use crate::tick::Game;
use flyff_common::rt::KickReason;
use flyff_db::DbOp;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Longest operator notice (`/sys`, 23 §8.2).
pub const MAX_NOTICE_BYTES: usize = 512;

/// `true` for the commands the runtime's own `admin_builtin` executes (unless a handler claims them).
pub fn is_builtin(cmd: &AdminCmd) -> bool {
    matches!(
        cmd,
        AdminCmd::ListOnline { .. }
            | AdminCmd::Kick { .. }
            | AdminCmd::Broadcast { .. }
            | AdminCmd::GetRates
            | AdminCmd::SetRate { .. }
            | AdminCmd::GetEvents
            | AdminCmd::SetEvent { .. }
            | AdminCmd::SetRuntime { .. }
            | AdminCmd::UnsetRuntime { .. }
            | AdminCmd::ReloadScripts { .. }
            | AdminCmd::RunMaintenance
    )
}

/// The `set_by` label of a runtime change (`runtime_override.set_by`, 13 §5.1): `admin:console`, `admin:cli`,
/// `admin:owner:local`, `admin:owner:token` or `admin:staff:<account>` (25 §1.1).
fn set_by(actor: AdminActor) -> String {
    actor.set_by()
}

impl Game {
    /// Executes one built-in admin command.
    pub(crate) fn admin_builtin(&mut self, cmd: AdminCmd, actor: AdminActor) -> AdminOutput {
        match cmd {
            AdminCmd::ListOnline { limit } => self.admin_list_online(limit),
            AdminCmd::Kick {
                target,
                reason,
                banned,
            } => self.admin_kick(&target, banned, reason.as_deref()),
            AdminCmd::Broadcast { text } => self.admin_broadcast(&text),
            AdminCmd::RunMaintenance => {
                let (svc, mut ctx) = self.split();
                let selected = crate::persist::maintenance::run_now(svc, &mut ctx);
                AdminOutput::Done {
                    affected: u32::try_from(selected).unwrap_or(u32::MAX),
                }
            }
            AdminCmd::GetRates => AdminOutput::Rates(self.rate_entries()),
            AdminCmd::GetEvents => AdminOutput::Events(self.event_entries()),
            AdminCmd::SetRate { key, value } => self.admin_set_rate(&key, value, actor),
            AdminCmd::SetEvent { id, state } => self.admin_set_event(id, state, actor),
            AdminCmd::ReloadScripts {
                target,
                npc_sources,
            } => self.admin_reload_scripts(target, npc_sources),
            AdminCmd::SetRuntime { key, value } => match overrides::parse_key(&key) {
                Ok(k) => self.change_runtime(k, Some(&value), actor).map_or_else(
                    AdminOutput::Error,
                    |generation| {
                        AdminOutput::Lines(vec![format!(
                            "{} = {value} (generation {generation})",
                            canonical_key(k)
                        )])
                    },
                ),
                Err(e) => AdminOutput::Error(e.to_string()),
            },
            AdminCmd::UnsetRuntime { key } => match overrides::parse_key(&key) {
                Ok(k) => self.change_runtime(k, None, actor).map_or_else(
                    AdminOutput::Error,
                    |generation| {
                        AdminOutput::Lines(vec![format!(
                            "{} override removed (generation {generation})",
                            canonical_key(k)
                        )])
                    },
                ),
                Err(e) => AdminOutput::Error(e.to_string()),
            },
            other => AdminOutput::Error(format!(
                "{} is not a runtime command",
                crate::dispatch::admin_name(&other)
            )),
        }
    }

    // ---- online list and kicks ------------------------------------------------------------------------------

    fn admin_list_online(&self, limit: Option<u32>) -> AdminOutput {
        let mut rows: Vec<&crate::session::SessionEntry> = self.sessions.iter().collect();
        // newest first
        rows.sort_by(|a, b| {
            b.joined_at
                .cmp(&a.joined_at)
                .then_with(|| b.player.cmp(&a.player))
        });
        let take = limit.map_or(usize::MAX, |n| usize::try_from(n).unwrap_or(usize::MAX));
        let out = rows
            .into_iter()
            .take(take)
            .map(|e| {
                let dir = self.services.presence.get(e.character);
                let mover = self.worlds.player(e.player);
                let (level, job) = mover.map_or_else(
                    || dir.map_or((0, 0), |d| (u16::from(d.level), d.job)),
                    |m| {
                        (
                            u16::try_from(m.mv.level.max(0)).unwrap_or(u16::MAX),
                            u8::try_from(m.mv.job.max(0)).unwrap_or(0),
                        )
                    },
                );
                OnlineEntry {
                    session: e.last_session.unwrap_or(flyff_common::SessionId(0)),
                    account_id: e.account,
                    character_id: e.character,
                    name: dir.map_or_else(|| format!("#{}", e.character.0), |d| d.name.clone()),
                    level,
                    job,
                    world: mover.map_or(0, |m| m.core.world.0),
                    layer: mover.map_or(0, |m| m.core.layer.0),
                    pos: mover.map_or([0.0; 3], |m| [m.core.pos.x, m.core.pos.y, m.core.pos.z]),
                    authority: e.authority.letter(),
                    ip: String::new(),
                    in_world_since: e.joined_at_unix.to_unix_time(),
                    lingering: e.session.is_none(),
                }
            })
            .collect();
        AdminOutput::Online(out)
    }

    fn admin_kick(
        &mut self,
        target: &AdminTarget,
        banned: bool,
        reason_text: Option<&str>,
    ) -> AdminOutput {
        let reason = if banned {
            KickReason::Banned
        } else {
            KickReason::GmKick
        };
        let players: Vec<flyff_common::ObjId> = match target {
            AdminTarget::Character(name) => {
                let Some(id) = self.services.presence.by_name(name).map(|d| d.id) else {
                    return AdminOutput::Error("not found".to_owned());
                };
                self.sessions
                    .iter()
                    .filter(|e| e.character == id)
                    .map(|e| e.player)
                    .collect()
            }
            AdminTarget::AllPlayers => self
                .sessions
                .iter()
                .filter(|e| !e.authority.is_gm())
                .map(|e| e.player)
                .collect(),
            AdminTarget::Account(_) => {
                return AdminOutput::Error(
                    "an account target is resolved by the server (GameInput::KickAccount)"
                        .to_owned(),
                );
            }
        };
        if players.is_empty() && !matches!(target, AdminTarget::AllPlayers) {
            return AdminOutput::Error("not online".to_owned());
        }
        let affected = u32::try_from(players.len()).unwrap_or(u32::MAX);
        // the operator's text becomes `Kick.detail` (at most 255 bytes, on a character boundary)
        let detail = reason_text
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| {
                let mut end = t.len().min(255);
                while end > 0 && !t.is_char_boundary(end) {
                    end -= 1;
                }
                t.get(..end).unwrap_or("").to_owned()
            });
        for p in players {
            if let Some(e) = self.sessions.get_mut(p) {
                e.kick_detail.clone_from(&detail);
            }
            self.begin_leave(p, LeaveTrigger::Kicked(reason));
        }
        AdminOutput::Done { affected }
    }

    fn admin_broadcast(&mut self, text: &str) -> AdminOutput {
        let text = text.trim();
        if text.is_empty() {
            return AdminOutput::Error("the notice is empty".to_owned());
        }
        if text.len() > MAX_NOTICE_BYTES {
            return AdminOutput::Error(format!(
                "the notice is longer than {MAX_NOTICE_BYTES} bytes"
            ));
        }
        let sessions: Vec<_> = self.sessions.bound().map(|(s, _)| s).collect();
        for s in &sessions {
            self.outbox.to_session(
                *s,
                0,
                flyff_proto::pb::SystemNotice {
                    text: text.to_owned(),
                },
            );
        }
        AdminOutput::Done {
            affected: u32::try_from(sessions.len()).unwrap_or(u32::MAX),
        }
    }

    // ---- rates and events -------------------------------------------------------------------------------------

    fn rate_entry(&self, k: &'static RateKey) -> RateEntry {
        RateEntry {
            key: k.short,
            config_key: k.key,
            data_default: k.data_default(),
            toml: k.get(&self.cfg.rules.rates),
            overridden: self
                .overrides
                .get(k.key)
                .and_then(serde_json::Value::as_f64)
                .map(|v| v as f32),
            effective: k.effective(&self.live.rates),
        }
    }

    pub(crate) fn rate_entries(&self) -> Vec<RateEntry> {
        RATE_KEYS.iter().map(|k| self.rate_entry(k)).collect()
    }

    fn event_state_of(&self, id: u16) -> u8 {
        // the script range (500..) is not part of the live configuration: the store holds it (22 §3.1)
        if id > MAX_FLAG_ID {
            return self.services.event.flags.get(id);
        }
        self.live.event_flags.get(&id).copied().unwrap_or_else(|| {
            self.live
                .events
                .initial_flags()
                .iter()
                .find(|(e, _)| e.id() == id)
                .map_or(0, |(_, v)| *v)
        })
    }

    fn event_entry(&self, id: u16) -> EventEntry {
        EventEntry {
            id,
            key: event_key(id),
            state: self.event_state_of(id),
            overridden: self.live.event_flags.contains_key(&id),
        }
    }

    pub(crate) fn event_entries(&self) -> Vec<EventEntry> {
        let mut ids: BTreeSet<u16> = overrides::EVENT_KEYS.iter().map(|(_, e)| e.id()).collect();
        ids.extend(self.live.event_flags.keys().copied());
        // the generic-event flags that are on (22 §6): listed, never overridden
        ids.extend(
            self.services
                .event
                .flags
                .nonzero()
                .map(|(id, _)| id)
                .filter(|id| *id > MAX_FLAG_ID),
        );
        ids.into_iter().map(|id| self.event_entry(id)).collect()
    }

    fn admin_set_rate(&mut self, key: &str, value: Option<f64>, actor: AdminActor) -> AdminOutput {
        let Some(k) = RateKey::find(key) else {
            return AdminOutput::Error(format!("unknown rate '{key}'"));
        };
        let result = match value {
            Some(v) => self.change_runtime(RuntimeKey::Rate(k), Some(&serde_json::json!(v)), actor),
            None => self.change_runtime(RuntimeKey::Rate(k), None, actor),
        };
        match result {
            Ok(_) => AdminOutput::Rates(vec![self.rate_entry(k)]),
            Err(e) => AdminOutput::Error(e),
        }
    }

    /// `SetEvent` (23 §8.2): ids `0..=499` are a runtime change of doc 10 (the id 1 refusal, the `runtime_override`
    /// row); ids `500..=1023` belong to the generic-event timer and are set in the store directly, never persisted
    /// (22 §3.3, WP-GM-25); anything above is refused.
    fn admin_set_event(&mut self, id: u16, state: u8, actor: AdminActor) -> AdminOutput {
        if usize::from(id) >= MAX_EVENT {
            return AdminOutput::Error(format!("event ids go up to {}", MAX_EVENT - 1));
        }
        if id > MAX_FLAG_ID {
            let origin = FlagOrigin::Admin { by: actor.label() };
            let (svc, mut ctx) = self.split();
            return match svc.event.set_flag(&mut ctx, id, state, origin) {
                Ok(_) => AdminOutput::Events(vec![self.event_entry(id)]),
                Err(e) => AdminOutput::Error(e.to_string()),
            };
        }
        match self.change_runtime(
            RuntimeKey::Event(id),
            Some(&serde_json::json!(state)),
            actor,
        ) {
            Ok(_) => AdminOutput::Events(vec![self.event_entry(id)]),
            Err(e) => AdminOutput::Error(e),
        }
    }

    /// The one path of a runtime change: validate, build the next generation, swap it in, persist the row.
    /// `value = None` removes the override. Returns the generation in force afterwards.
    fn change_runtime(
        &mut self,
        key: RuntimeKey,
        value: Option<&serde_json::Value>,
        actor: AdminActor,
    ) -> Result<u64, String> {
        self.change_runtime_by(key, value, &set_by(actor))
    }

    /// [`Game::change_runtime`] with an explicit `runtime_override.set_by` label (`gm:<account>` for `/rates`,
    /// Plan/23 §8.5).
    pub(crate) fn change_runtime_by(
        &mut self,
        key: RuntimeKey,
        value: Option<&serde_json::Value>,
        by: &str,
    ) -> Result<u64, String> {
        let canonical = canonical_key(key);
        let applied = match value {
            Some(v) => overrides::set_value(&self.live, key, v, by),
            None => Ok(overrides::clear_value(&self.live, &self.cfg.rules, key, by)),
        }
        .map_err(|e: OverrideError| e.to_string())?;
        if let Applied::New(next) = applied {
            tracing::info!(
                key = %canonical,
                generation = next.generation,
                by,
                "runtime configuration changed"
            );
            self.replace_live(Arc::new(next));
        }
        // the row follows the override map even when the generation did not change (a first `set` of the file value)
        let persist = self.cfg.rules.fidelity.persist_gm_overrides;
        match value {
            Some(v) => {
                if self.overrides.get(&canonical) != Some(v) {
                    self.overrides.insert(canonical.clone(), v.clone());
                    if persist {
                        self.persist.op(DbOp::SetRuntimeOverride {
                            key: canonical,
                            value: v.clone(),
                            by: by.to_owned(),
                            at: self.clock.unix_ms.to_unix_time(),
                        });
                    }
                }
            }
            None => {
                if self.overrides.remove(&canonical).is_some() && persist {
                    self.persist
                        .op(DbOp::DeleteRuntimeOverride { key: canonical });
                }
            }
        }
        Ok(self.live.generation)
    }
}
