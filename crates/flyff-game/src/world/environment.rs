//! `Environment` and `EnvironmentAll`, the season lookup and the weather-effect state (Plan/15 §17.2,
//! WP-WORLD-12; ref/19 §21; F-095 message part).
//!
//! The weather **timer, roll and authority** are doc 22's `WeatherService` (WP-EVENT-22, job P10); this module
//! owns the messages, the season table of `EnvironmentEffect.txt` and the effect state the join sequence and the
//! world change read, plus the broadcast helpers `WeatherService` and the GM commands call:
//!
//! * [`SeasonTable`]: `EnvironmentEffect.txt` is `<lang> <s1> .. <s12>` groups (one season per calendar month, the
//!   *real* month of `server.timezone`); the shipped file is a single `0`, so every month is `SEASON_NONE`;
//! * [`EnvironmentState`] (`Worlds.environment`): `effect` (starts false) and the event `title`;
//! * [`set_effect`], [`broadcast_environment`], [`fall_rain`] (effect on, **no** broadcast: the next minute job of
//!   `WeatherService` broadcasts), [`stop_rain`] (effect off and one `Environment` to every in-world player).

use crate::ctx::GameCtx;
use crate::outbox::Audience;
use crate::tick::LocalStamp;
use crate::world::Worlds;
use flyff_common::ObjId;
use flyff_common::defs::lang::LANG_ENG;
use flyff_proto::pb;
use std::collections::BTreeMap;

/// `SEASON_NONE`.
pub const SEASON_NONE: i32 = 0;
/// `SEASON_MAX`: the exclusive bound of a valid season value.
pub const SEASON_MAX: u32 = 5;

/// The weather effect as the clients know it (`CEnvironment::m_bEffect` and the event title).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentState {
    /// A weather effect is active.
    pub effect: bool,
    /// The title text of the weather event (`GetWeatherEventTitle()`, doc 22); `""` when unknown.
    pub title: String,
}

/// Stores the effect (no message). Returns `true` when `effect` changed (doc 22's `WeatherService` decides
/// whether to broadcast).
pub fn set_effect(w: &mut Worlds, on: bool, title: String) -> bool {
    let changed = w.environment.effect != on;
    w.environment.effect = on;
    w.environment.title = if on { title } else { String::new() };
    changed
}

/// `Environment{effect, title if effect else ""}` of the current state (join step 6, world change).
pub fn environment_msg(w: &Worlds) -> pb::Environment {
    pb::Environment {
        effect: w.environment.effect,
        title: if w.environment.effect {
            w.environment.title.clone()
        } else {
            String::new()
        },
    }
}

/// The season of the real month `local.month` (`1..=12`) for the server language.
pub fn season_msg(table: &SeasonTable, local: &LocalStamp) -> pb::EnvironmentAll {
    pb::EnvironmentAll {
        season: table.season(LANG_ENG, local.month),
    }
}

/// `Environment` to `Audience::AllOnline` (subject 0): every player that is in a world.
pub fn broadcast_environment(ctx: &mut GameCtx<'_>) {
    let msg = environment_msg(ctx.worlds);
    ctx.broadcast(None, ObjId(0), Audience::AllOnline, msg);
}

/// The effect half of the legacy `/FallRain`: effect on, no broadcast.
pub fn fall_rain(ctx: &mut GameCtx<'_>, title: String) {
    set_effect(ctx.worlds, true, title);
}

/// The effect half of the legacy `/StopRain`: effect off, then one `Environment{effect: false}` to everybody
/// who is in a world (lingering sessions have no sink and get nothing).
pub fn stop_rain(ctx: &mut GameCtx<'_>) {
    set_effect(ctx.worlds, false, String::new());
    broadcast_environment(ctx);
}

/// A bad `EnvironmentEffect.txt`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SeasonError {
    /// A token is not a number.
    #[error("EnvironmentEffect.txt: '{0}' is not a number")]
    NotANumber(String),
    /// A language outside `0..LANG_MAX`.
    #[error("EnvironmentEffect.txt: invalid language {0}")]
    BadLanguage(i64),
    /// A season outside `0..SEASON_MAX`.
    #[error("EnvironmentEffect.txt: invalid season {0}")]
    BadSeason(i64),
}

/// `CEnvironment::m_aEnvironmentEffect`: the season of every calendar month per language.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeasonTable {
    by_lang: BTreeMap<u32, [u8; 12]>,
}

impl SeasonTable {
    /// `CEnvironment::LoadScript` over the decoded text: `<lang> <12 seasons>` groups. Like the legacy scanner a
    /// group that runs into the end of the file reads zeros for the missing numbers (the shipped file is the
    /// single token `0`: language 0, twelve times `SEASON_NONE`).
    pub fn parse(text: &str) -> Result<SeasonTable, SeasonError> {
        const LANG_MAX: i64 = 16;
        let mut nums = Vec::new();
        for tok in text.split_whitespace() {
            let v: i64 = tok
                .parse()
                .map_err(|_| SeasonError::NotANumber(tok.to_owned()))?;
            nums.push(v);
        }
        let mut by_lang = BTreeMap::new();
        for group in nums.chunks(13) {
            let lang = group.first().copied().unwrap_or(0);
            if !(0..LANG_MAX).contains(&lang) {
                return Err(SeasonError::BadLanguage(lang));
            }
            let mut row = [0_u8; 12];
            for (i, slot) in row.iter_mut().enumerate() {
                let s = group.get(i + 1).copied().unwrap_or(0);
                if !(0..i64::from(SEASON_MAX)).contains(&s) {
                    return Err(SeasonError::BadSeason(s));
                }
                *slot = s as u8;
            }
            by_lang.insert(lang as u32, row);
        }
        Ok(SeasonTable { by_lang })
    }

    /// Reads `EnvironmentEffect.txt` through the VFS; a missing or bad file gives the empty table (every season
    /// `SEASON_NONE`) and a warning.
    pub fn load(vfs: &flyff_vfs::Vfs) -> SeasonTable {
        let Ok(decoded) = vfs.read_text_auto("EnvironmentEffect.txt") else {
            return SeasonTable::default();
        };
        match SeasonTable::parse(&decoded.text) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("{e}; no seasonal weather");
                SeasonTable::default()
            }
        }
    }

    /// `GetSeason()` for `month` (`1..=12`); `SEASON_NONE` for an unknown language or month.
    pub fn season(&self, lang: u32, month: u8) -> i32 {
        let Some(idx) = usize::from(month).checked_sub(1) else {
            return SEASON_NONE;
        };
        self.by_lang
            .get(&lang)
            .and_then(|row| row.get(idx))
            .map_or(SEASON_NONE, |&s| i32::from(s))
    }
}
