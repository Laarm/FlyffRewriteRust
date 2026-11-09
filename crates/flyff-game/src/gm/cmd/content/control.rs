//! `/content` (N17, alias `ctl`; WP-GM-26): the state of every timed M7 feature and the verbs that start, stop and advance
//! them, in one command that works from a chat line and from the console (`POST /admin/api/console`).
//!
//! * `/content`: one state line per feature, in the order of the feature table (`FEATURES`) (`lord`, `tax`, `sr`, `gc1to1`, `rr`, `quiz`,
//!   `gh`, `housing`), also while a flag is off (the line then ends with `, off (events.<key>)`);
//! * `/content <key>`: its line (`secretroom`, `gc11`, `rainbow` and `guildhouse` are aliases of `sr`, `gc1to1`, `rr`,
//!   `gh`);
//! * `/content gh <guild>` and `/content housing <name>` (the rest of the line, so a name may contain spaces; a verb word
//!   wins over a name): the line of one house or room. An unknown guild or character is the standard `does not exist` reply;
//! * `/content <key> <verb> [arg]`: the verb, then the state line of the key. An "advance" verb still shows the old state:
//!   the owner's slot item runs the transition within a second (guild house: within five), so a flow polls
//!   `/content <key>` with a regex such as `^sr: LineUp`.
//!
//! Every line is a `ChatText` (the BFIX1 list rule). The state lines are the ones the owners ship in their `gm.rs`
//! (`<key>: <State>[, <n> s left][, <counts>]`, the plan's enum name of the state, so `^<key>: [A-Z]\w*` matches). A verb
//! calls the same action function as its legacy row (`gc1to1.rs`, `secret_room.rs`, `lord.rs`, `tax.rs`, `rainbow.rs`,
//! `quiz.rs`), so there is one transition. A refused verb prints `"<key>: <reason>"` and the state line. While
//! `events.secret_room`, `events.guild_combat_1to1` or `events.rainbow_race` is off, the verbs of that feature print
//! `"<key>: <off text>"` and change nothing.
//!
//! The audit target is the key, followed by the guild name for `gh` and the character name for `housing`.

use super::{
    GC1TO1_OFF, RAINBOW_OFF, SECRET_ROOM_OFF, flag_on, gc1to1, lord, quiz, rainbow, secret_room,
    tax,
};
use crate::ctx::{GameCtx, Services};
use crate::event::guild_house::gm as gh_gm;
use crate::event::housing::gm as housing_gm;
use crate::event::lord::gm as lord_gm;
use crate::event::quiz::gm as quiz_gm;
use crate::event::rainbow::gm as rr_gm;
use crate::event::secret_room::gm as sr_gm;
use crate::event::tax::gm as tax_gm;
use crate::gm::target::{self, Target};
use crate::gm::{CmdArgs, CmdCtx, CmdDone, CmdError, CmdResult};
use crate::social::gc1to1::gm as gc_gm;
use flyff_common::config::EventId;
use flyff_common::{CharacterId, GuildId};
use flyff_db::ElectionState;

/// The event flag that gates the verbs of a feature.
struct FlagGate {
    id: EventId,
    /// The config key shown in the `, off (...)` tail.
    key: &'static str,
    /// What a gated verb prints after `"<key>: "`.
    off_text: &'static str,
}

/// What the name after a verb (or after the key) means.
#[derive(Clone, Copy)]
enum SubjectKind {
    Guild,
    Character,
}

/// A resolved name.
enum Subject {
    Guild { id: GuildId, name: String },
    Character { id: CharacterId, name: String },
}

impl Subject {
    fn name(&self) -> &str {
        match self {
            Subject::Guild { name, .. } | Subject::Character { name, .. } => name,
        }
    }
}

/// One row of the feature table.
struct ContentFeature {
    key: &'static str,
    aliases: &'static [&'static str],
    verbs: &'static [&'static str],
    flag: Option<FlagGate>,
    subject: Option<SubjectKind>,
    /// The state line(s) of the feature.
    state: fn(&Services, &GameCtx<'_>) -> Vec<String>,
    /// Runs a verb of [`ContentFeature::verbs`]: `Err(reason)` prints `"<key>: <reason>"`.
    run: fn(&mut CmdCtx<'_, '_>, &str, &mut CmdArgs<'_>, Option<&Subject>) -> Result<(), String>,
}

/// The features in the order `/content` prints them.
static FEATURES: &[ContentFeature] = &[
    ContentFeature {
        key: "lord",
        aliases: &[],
        verbs: &["on", "off", "next", "events"],
        flag: None,
        subject: None,
        state: |s, c| vec![lord_gm::state_line(s, c)],
        run: run_lord,
    },
    ContentFeature {
        key: "tax",
        aliases: &[],
        verbs: &["next", "pay"],
        flag: None,
        subject: None,
        state: |s, c| vec![tax_gm::state_line(s, c)],
        run: run_tax,
    },
    ContentFeature {
        key: "sr",
        aliases: &["secretroom"],
        verbs: &["open", "next", "close"],
        flag: Some(FlagGate {
            id: EventId::SecretRoom,
            key: "events.secret_room",
            off_text: SECRET_ROOM_OFF,
        }),
        subject: None,
        state: |s, c| vec![sr_gm::state_line(s, c)],
        run: run_sr,
    },
    ContentFeature {
        key: "gc1to1",
        aliases: &["gc11"],
        verbs: &["open", "next", "close"],
        flag: Some(FlagGate {
            id: EventId::GuildCombat1to1,
            key: "events.guild_combat_1to1",
            off_text: GC1TO1_OFF,
        }),
        subject: None,
        state: |s, c| vec![gc_gm::state_line(s, c)],
        run: run_gc1to1,
    },
    ContentFeature {
        key: "rr",
        aliases: &["rainbow"],
        verbs: &["application", "open", "next"],
        flag: Some(FlagGate {
            id: EventId::RainbowRace,
            key: "events.rainbow_race",
            off_text: RAINBOW_OFF,
        }),
        subject: None,
        state: |s, c| vec![rr_gm::state_line(s, c)],
        run: run_rr,
    },
    ContentFeature {
        key: "quiz",
        aliases: &[],
        verbs: &["open", "next", "close"],
        flag: None,
        subject: None,
        state: |s, c| vec![quiz_gm::state_line(s, c)],
        run: run_quiz,
    },
    ContentFeature {
        key: "gh",
        aliases: &["guildhouse"],
        verbs: &["give", "expire", "furniture"],
        flag: None,
        subject: Some(SubjectKind::Guild),
        state: |s, c| vec![gh_gm::state_line(s, c)],
        run: run_gh,
    },
    ContentFeature {
        key: "housing",
        aliases: &[],
        verbs: &[],
        flag: None,
        subject: Some(SubjectKind::Character),
        state: |s, c| vec![housing_gm::state_line(s, c)],
        run: run_housing,
    },
];

/// The feature of a key or alias, ASCII case-insensitive.
fn find(token: &str) -> Option<&'static ContentFeature> {
    FEATURES.iter().find(|f| {
        f.key.eq_ignore_ascii_case(token) || f.aliases.iter().any(|a| a.eq_ignore_ascii_case(token))
    })
}

/// The state line(s) of `f`, with the `, off (events.<key>)` tail while its flag is off.
fn state_lines(svc: &Services, ctx: &GameCtx<'_>, f: &ContentFeature) -> Vec<String> {
    let mut lines = (f.state)(svc, ctx);
    if let Some(flag) = &f.flag
        && ctx.live.event_state(flag.id) == 0
    {
        for l in &mut lines {
            l.push_str(&format!(", off ({})", flag.key));
        }
    }
    lines
}

fn print_state(cx: &mut CmdCtx<'_, '_>, f: &ContentFeature) {
    for l in state_lines(cx.svc, cx.ctx, f) {
        cx.out.chat_text(l);
    }
}

// ---- the verbs ------------------------------------------------------------------------------------------------------

fn unknown_verb(verb: &str) -> Result<(), String> {
    Err(format!("unknown verb {verb}"))
}

fn run_lord(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    match verb {
        "on" => lord::process(cx, true),
        "off" => lord::process(cx, false),
        "events" => lord::initialize(cx),
        "next" => {
            let step = match cx.svc.event.lord.state() {
                ElectionState::Ready => lord::begin_candidacy(cx),
                ElectionState::Candidacy => lord::begin_vote(cx),
                ElectionState::Vote => lord::end_vote(cx),
                ElectionState::Expired => return Err("no step from Expired".to_owned()),
            };
            step.map_err(|e| e.to_string())?;
        }
        other => return unknown_verb(other),
    }
    Ok(())
}

fn run_tax(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    match verb {
        "next" => tax::apply_now(cx).map_err(|_| "in vote".to_owned()),
        "pay" => {
            tax::pay_now(cx);
            Ok(())
        }
        other => unknown_verb(other),
    }
}

fn run_sr(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    match verb {
        "open" => secret_room::open(cx).map_err(str::to_owned),
        "next" => {
            secret_room::next(cx);
            Ok(())
        }
        "close" => secret_room::close(cx).map_err(str::to_owned),
        other => unknown_verb(other),
    }
}

fn run_gc1to1(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    match verb {
        "open" => {
            let server_down = args.number() == 1;
            gc1to1::open(cx, server_down).map_err(str::to_owned)
        }
        "next" => gc1to1::next(cx).map_err(str::to_owned),
        "close" => gc1to1::close(cx).map_err(str::to_owned),
        other => unknown_verb(other),
    }
}

fn run_rr(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    match verb {
        "application" => {
            rainbow::application(cx).map_err(|_| "application needs the Closed state".to_owned())
        }
        "open" => rainbow::open(cx, true)
            .map_err(|_| "open needs the Closed or Application state".to_owned()),
        "next" => {
            rainbow::next(cx);
            Ok(())
        }
        other => unknown_verb(other),
    }
}

fn run_quiz(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    match verb {
        "open" => quiz::open(cx).map_err(|e| e.to_string()),
        "next" => quiz::state_next(cx).map_err(|_| "not running".to_owned()),
        "close" => quiz::close(cx).map_err(|_| "not running".to_owned()),
        other => unknown_verb(other),
    }
}

fn run_gh(
    cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    subject: Option<&Subject>,
) -> Result<(), String> {
    let Some(Subject::Guild { id, name }) = subject else {
        return Err("needs a guild".to_owned());
    };
    match verb {
        "give" => gh_gm::grant_house(cx.svc, cx.ctx, *id).map_err(|e| e.to_string())?,
        "expire" => gh_gm::expire_upkeep(cx.svc, cx.ctx, *id).map_err(|e| e.to_string())?,
        "furniture" => {
            let n = gh_gm::expire_furniture(cx.svc, cx.ctx, *id);
            cx.out
                .chat_text(format!("gh {name}: {n} furniture entries due now"));
        }
        other => return unknown_verb(other),
    }
    let line = gh_gm::house_line(cx.svc, cx.ctx, *id);
    cx.out.chat_text(line);
    Ok(())
}

fn run_housing(
    _cx: &mut CmdCtx<'_, '_>,
    verb: &str,
    _args: &mut CmdArgs<'_>,
    _subject: Option<&Subject>,
) -> Result<(), String> {
    unknown_verb(verb)
}

// ---- the command ----------------------------------------------------------------------------------------------------

/// `name` without a pair of quotes around it.
fn unquoted(name: &str) -> &str {
    let t = name.trim();
    t.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(t)
}

/// Resolves the name of a guild or a character.
fn resolve(cx: &CmdCtx<'_, '_>, kind: SubjectKind, name: &str) -> Result<Subject, CmdError> {
    let name = unquoted(name);
    if name.is_empty() {
        return Err(CmdError::Usage);
    }
    match kind {
        SubjectKind::Guild => {
            let id = cx
                .svc
                .social
                .guild
                .by_name(name)
                .ok_or_else(|| CmdError::NotFound(name.to_owned()))?;
            let stored = cx
                .svc
                .social
                .guild
                .get(id)
                .map_or_else(|| name.to_owned(), |g| g.name.clone());
            Ok(Subject::Guild { id, name: stored })
        }
        SubjectKind::Character => match target::find(cx.svc, name) {
            Target::Unknown => Err(CmdError::NotFound(name.to_owned())),
            Target::Offline { id, name } | Target::Online { id, name, .. } => {
                Ok(Subject::Character { id, name })
            }
        },
    }
}

/// The line of one house or room.
fn subject_line(cx: &CmdCtx<'_, '_>, subject: &Subject) -> String {
    match subject {
        Subject::Guild { id, .. } => gh_gm::house_line(cx.svc, cx.ctx, *id),
        Subject::Character { id, .. } => housing_gm::room_line(cx.svc, cx.ctx, *id),
    }
}

/// N17 `/content [<key> [<verb> [<name>]]]`: see the module documentation.
pub fn content(cx: &mut CmdCtx<'_, '_>, args: &mut CmdArgs<'_>) -> CmdResult {
    let key = args.token().to_owned();
    if key.is_empty() {
        for f in FEATURES {
            print_state(cx, f);
        }
        return Ok(CmdDone::default());
    }
    let Some(f) = find(&key) else {
        return Err(CmdError::Usage);
    };
    let tail = args.rest_raw().trim().to_owned();
    if tail.is_empty() {
        print_state(cx, f);
        return Ok(CmdDone::on(f.key));
    }
    let mut rest = CmdArgs::new(&tail);
    let word = rest.token().to_owned();
    let verb = f.verbs.iter().find(|v| v.eq_ignore_ascii_case(&word));
    let Some(verb) = verb else {
        // not a verb: the name of a house or a room, or a mistake
        let Some(kind) = f.subject else {
            return Err(CmdError::Refused(format!(
                "{}: verbs {}",
                f.key,
                f.verbs.join(", ")
            )));
        };
        let subject = resolve(cx, kind, &tail)?;
        let line = subject_line(cx, &subject);
        cx.out.chat_text(line);
        return Ok(CmdDone::on(format!("{} {}", f.key, subject.name())));
    };
    // a verb that needs a name reads the rest of the line
    let subject = match f.subject {
        Some(kind) => {
            let name = rest.last_full();
            Some(resolve(cx, kind, &name)?)
        }
        None => None,
    };
    let done = match &subject {
        Some(s) => CmdDone::on(format!("{} {}", f.key, s.name())),
        None => CmdDone::on(f.key),
    };
    if let Some(flag) = &f.flag
        && !flag_on(cx, flag.id)
    {
        cx.out.chat_text(format!("{}: {}", f.key, flag.off_text));
        print_state(cx, f);
        return Ok(done);
    }
    if let Err(reason) = (f.run)(cx, verb, &mut rest, subject.as_ref()) {
        cx.out.chat_text(format!("{}: {reason}", f.key));
    }
    print_state(cx, f);
    Ok(done)
}
