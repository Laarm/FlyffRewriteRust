//! The anti-spam state machine of the server (Plan/23 §6.3, PL-110, VR-072): the logic of the legacy client
//! (`WndManager.cpp:3945-4060` for normal chat, `FuncTextCmd.cpp:1820-1997` for shouts) moved to the server and
//! driven only by the game clock (`GameMs`, D2 determinism). All windows start expired.
//!
//! The functions return the replies as data ([`SpamReply`]); the caller (the pipelines) queues them as
//! `DefinedText`. The constants are the `gameplay.chat_*` / `gameplay.shout_*` keys ([`SpamCfg`]).

use flyff_common::GameMs;
use flyff_common::defs::text::{
    TID_GAME_CHATSAMETEXT, TID_GAME_ERROR_CHATTING_1, TID_GAME_ERROR_CHATTING_2,
    TID_GAME_ERROR_CHATTING_3, TID_GAME_LIMIT_SHOUT,
};
use flyff_common::gm::Authority;

/// The timer constants (`gameplay.*`, defaults of 03 §4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpamCfg {
    /// `chat_same_text_ms` (2000).
    pub same_text_ms: u32,
    /// `chat_warn_ms` (700).
    pub warn_ms: u32,
    /// `chat_warn2_ms` (1000).
    pub warn2_ms: u32,
    /// `chat_shortcut_warn_ms` (3000).
    pub shortcut_warn_ms: u32,
    /// `chat_ban_points` (5).
    pub ban_points: u32,
    /// `chat_ban2_points` (2).
    pub ban2_points: u32,
    /// `chat_ban_ms` (180 000).
    pub ban_ms: u32,
    /// `shout_same_text_ms` (3000).
    pub shout_same_text_ms: u32,
    /// `shout_limit_count` (5); 0 = no limiter.
    pub shout_limit_count: u32,
    /// `shout_limit_window_ms` (5000).
    pub shout_limit_window_ms: u32,
}

impl SpamCfg {
    /// The values of the loaded `[gameplay]` section.
    pub fn from_gameplay(g: &flyff_common::config::Gameplay) -> Self {
        Self {
            same_text_ms: g.chat_same_text_ms,
            warn_ms: g.chat_warn_ms,
            warn2_ms: g.chat_warn2_ms,
            shortcut_warn_ms: g.chat_shortcut_warn_ms,
            ban_points: g.chat_ban_points,
            ban2_points: g.chat_ban2_points,
            ban_ms: g.chat_ban_ms,
            shout_same_text_ms: g.shout_same_text_ms,
            shout_limit_count: g.shout_limit_count,
            shout_limit_window_ms: g.shout_limit_window_ms,
        }
    }
}

impl Default for SpamCfg {
    fn default() -> Self {
        Self {
            same_text_ms: 2000,
            warn_ms: 700,
            warn2_ms: 1000,
            shortcut_warn_ms: 3000,
            ban_points: 5,
            ban2_points: 2,
            ban_ms: 180_000,
            shout_same_text_ms: 3000,
            shout_limit_count: 5,
            shout_limit_window_ms: 5000,
        }
    }
}

/// One `DefinedText` the speaker receives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpamReply {
    /// The `TID_*`.
    pub tid: u32,
    /// The positional arguments.
    pub args: Vec<String>,
}

impl SpamReply {
    fn new(tid: u32, args: &[&str]) -> Self {
        Self {
            tid,
            args: args.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// The verdict of one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpamVerdict {
    /// The line goes on.
    Send,
    /// The line is dropped (the replies say why).
    Refuse,
}

/// The per-player spam state (`m_strChatBackup`, `m_timerDobe`, the warning timers and counters, the ban timer
/// and the `TextCmd_shout` statics).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatSpamState {
    last_text: String,
    same_until: GameMs,
    warn_until: GameMs,
    warn2_until: GameMs,
    shortcut_until: GameMs,
    warn_count: u32,
    warn2_count: u32,
    ban_until: GameMs,
    shout_last: String,
    shout_same_until: GameMs,
    shout_window_end: Option<GameMs>,
    shout_count: u32,
}

impl ChatSpamState {
    /// `true` while the chat ban runs.
    pub fn banned(&self, now: GameMs) -> bool {
        now < self.ban_until
    }

    /// Step 1 of §6.3 and §6.4 step 2: the remaining ban time reply.
    fn ban_reply(&self, now: GameMs) -> Option<SpamReply> {
        if now >= self.ban_until {
            return None;
        }
        let secs = (self.ban_until.0 - now.0) / 1000;
        let (min, sec) = (secs / 60, secs % 60);
        Some(SpamReply::new(
            TID_GAME_ERROR_CHATTING_3,
            &[&min.to_string(), &sec.to_string()],
        ))
    }

    /// `ban()` of §6.3 step 5: the "chat blocked for N minutes" reply and the ban window.
    fn start_ban(&mut self, now: GameMs, cfg: &SpamCfg, replies: &mut Vec<SpamReply>) {
        let minutes = cfg.ban_ms / 60_000;
        replies.push(SpamReply::new(
            TID_GAME_ERROR_CHATTING_2,
            &[&minutes.to_string()],
        ));
        self.ban_until = GameMs(now.0 + u64::from(cfg.ban_ms));
    }

    /// Steps 6 and 7 of §6.3 (also step 7 of §6.4): the warning timers, with the verdict of the line.
    fn warning_timers(
        &mut self,
        now: GameMs,
        shortcut: bool,
        cfg: &SpamCfg,
        replies: &mut Vec<SpamReply>,
    ) -> SpamVerdict {
        if shortcut {
            let verdict = if now < self.shortcut_until {
                self.warn_count += 1;
                if self.warn_count >= cfg.ban_points {
                    self.warn_count = 0;
                    self.start_ban(now, cfg, replies);
                } else {
                    replies.push(SpamReply::new(TID_GAME_ERROR_CHATTING_1, &[]));
                }
                SpamVerdict::Refuse
            } else {
                SpamVerdict::Send
            };
            self.shortcut_until = GameMs(now.0 + u64::from(cfg.shortcut_warn_ms));
            return verdict;
        }
        let verdict = if now < self.warn_until {
            self.warn_count += 1;
            if self.warn_count >= cfg.ban_points {
                self.warn_count = 0;
                self.start_ban(now, cfg, replies);
            } else {
                replies.push(SpamReply::new(TID_GAME_ERROR_CHATTING_1, &[]));
            }
            SpamVerdict::Refuse
        } else if now < self.warn2_until {
            self.warn2_count += 1;
            if self.warn2_count >= cfg.ban2_points {
                self.warn2_count = 0;
                self.start_ban(now, cfg, replies);
                SpamVerdict::Refuse
            } else {
                SpamVerdict::Send
            }
        } else {
            SpamVerdict::Send
        };
        self.warn_until = GameMs(now.0 + u64::from(cfg.warn_ms));
        self.warn2_until = GameMs(now.0 + u64::from(cfg.warn2_ms));
        verdict
    }

    /// `check_normal` (§6.3): normal chat. `shortcut` = `ChatReq.shortcut`. GMs (`>= L`) bypass the timers.
    pub fn check_normal(
        &mut self,
        text: &str,
        shortcut: bool,
        authority: Authority,
        now: GameMs,
        cfg: &SpamCfg,
    ) -> (SpamVerdict, Vec<SpamReply>) {
        let mut replies = Vec::new();
        if let Some(r) = self.ban_reply(now) {
            replies.push(r);
            return (SpamVerdict::Refuse, replies);
        }
        if text == self.last_text && now < self.same_until && !authority.is_gm() {
            replies.push(SpamReply::new(TID_GAME_CHATSAMETEXT, &[]));
            return (SpamVerdict::Refuse, replies);
        }
        self.same_until = GameMs(now.0 + u64::from(cfg.same_text_ms));
        self.last_text = text.to_owned();
        if authority.is_gm() {
            return (SpamVerdict::Send, replies);
        }
        let verdict = self.warning_timers(now, shortcut, cfg, &mut replies);
        (verdict, replies)
    }

    /// Steps 2 to 5 of the shout pre-checks (§6.4): the ban, the same-text rule and the fixed-window limiter
    /// (skipped when `shout_full`, i.e. the speaker has a shout item), then the same-text bookkeeping. Returns
    /// `Some(replies)` when the shout is refused.
    pub fn shout_precheck(
        &mut self,
        body: &str,
        shout_full: bool,
        now: GameMs,
        cfg: &SpamCfg,
    ) -> Option<Vec<SpamReply>> {
        if let Some(r) = self.ban_reply(now) {
            return Some(vec![r]);
        }
        if body == self.shout_last && now < self.shout_same_until {
            return Some(vec![SpamReply::new(TID_GAME_CHATSAMETEXT, &[])]);
        }
        if !shout_full && cfg.shout_limit_count > 0 {
            self.shout_count += 1;
            // the legacy `m_dwTick` starts at 0, so the first call always opens a window
            if self.shout_window_end.is_none_or(|end| now > end) {
                self.shout_window_end = Some(GameMs(now.0 + u64::from(cfg.shout_limit_window_ms)));
                self.shout_count = 1;
            }
            if self.shout_count > cfg.shout_limit_count {
                // the ENG text reads "%d seconds, %d times": both values are 5 with the defaults
                return Some(vec![SpamReply::new(
                    TID_GAME_LIMIT_SHOUT,
                    &[
                        &(cfg.shout_limit_window_ms / 1000).to_string(),
                        &cfg.shout_limit_count.to_string(),
                    ],
                )]);
            }
        }
        self.shout_last = body.to_owned();
        self.shout_same_until = GameMs(now.0 + u64::from(cfg.shout_same_text_ms));
        None
    }

    /// Step 7 of §6.4: the warning timers of a shout (no GM exemption; `shortcut` = the line came from a macro
    /// slot). Returns the verdict and the replies.
    pub fn shout_warnings(
        &mut self,
        shortcut: bool,
        now: GameMs,
        cfg: &SpamCfg,
    ) -> (SpamVerdict, Vec<SpamReply>) {
        let mut replies = Vec::new();
        let v = self.warning_timers(now, shortcut, cfg, &mut replies);
        (v, replies)
    }
}
