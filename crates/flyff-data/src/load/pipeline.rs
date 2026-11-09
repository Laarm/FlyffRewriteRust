//! `load_all`: the phases P0-P8 in the original load order (Plan/14 §5.1-5.3).
//!
//! | Phase | Steps |
//! |---|---|
//! | P0 | open the VFS (the caller; `load_all` checks `Masquerade.prj`) |
//! | P1 | L01 defines, L02 strings, L03 `textClient.inc` |
//! | P2 | `Masquerade.prj`-driven loaders, in the file order of the project file (L10-L22) |
//! | P3 | the fixed list L30-L68 |
//! | P4 | world-server extras L70-L75 |
//! | P5 | per-world text data (L80) |
//! | P6 | Lua: static extraction and the live script registry (L90) |
//! | P7 | client-only files (L95), skipped when `opts.load_client_data` is false |
//! | P8 | validation, counts, the fingerprint, strict mode |
//!
//! Steps whose loader belongs to a later work package are **stubs**: they appear in the
//! [`LoadOutput::steps`] log with [`StepStatus::Stub`] and load nothing. The work package that implements a
//! loader replaces the `None` of its `STEPS` row (or its arm in `dispatch_prj`) by the glue function.

use super::ctx::{LoadCtx, Need};
use super::prj::{PrjEntry, PrjKey, parse_prj};
use super::steps::{self, client, ext, lua, models, npcs, props, quests, tables, worlds};
use crate::check::define_drift;
use crate::client_data::ClientData;
use crate::game_data::{GAMEDATA_LOADER_VERSION, GameData};
use crate::options::DataOptions;
use flyff_script::{DiagCode, Diagnostics, ScriptCtx, ScriptVariant, Severity, load_pre_files};
use flyff_vfs::{Vfs, data_fingerprint};
use std::sync::Arc;

/// The load phases (Plan/14 §5.1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Phase {
    /// Defines, strings, texts.
    P1,
    /// `Masquerade.prj`-driven loaders.
    P2,
    /// The fixed list L30-L68.
    P3,
    /// World-server extras L70-L75.
    P4,
    /// Per-world text data.
    P5,
    /// Lua configuration scripts.
    P6,
    /// Client-only data.
    P7,
    /// Validation and freezing.
    P8,
}

/// What happened to a step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepStatus {
    /// The loader ran.
    Done,
    /// The loader is not implemented yet (a later work package); nothing was loaded.
    Stub,
    /// Deliberately not run: dropped (`propKarma`, `billboard`) or deferred to another phase.
    Skipped,
}

/// One executed (or stubbed) step, in execution order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StepRecord {
    /// The step id of Plan/14 §5.3 (`L13`, `L33`, ...), `"L10/L11"` for the two model files.
    pub id: &'static str,
    /// The phase.
    pub phase: Phase,
    /// The file (or the project-file argument) the step is about.
    pub file: String,
    /// The outcome.
    pub status: StepStatus,
}

/// The result of [`load_all`].
#[derive(Debug)]
pub struct LoadOutput {
    /// The immutable game data (wrap it in `Arc`).
    pub game: GameData,
    /// The client-only data (`None` when `opts.load_client_data` is false).
    pub client: Option<ClientData>,
    /// Every diagnostic (the digest is in `game.meta.diag_summary`).
    pub diags: Diagnostics,
    /// The steps in execution order.
    pub steps: Vec<StepRecord>,
}

/// A Fatal condition: the server must not start (exit code 2, Plan/14 §8.2).
#[derive(Debug, thiserror::Error)]
#[error("data load failed: {message}")]
pub struct LoadError {
    /// The first Fatal (or strict-mode Error) diagnostic as text, `+n more` appended.
    pub message: String,
    /// Everything reported until the failure.
    pub diags: Diagnostics,
}

impl LoadError {
    fn from_diags(diags: Diagnostics, min: Severity) -> Self {
        let first = diags
            .items()
            .iter()
            .find(|d| d.severity >= min)
            .map_or_else(
                || "unspecified failure".to_owned(),
                |d| match &d.file {
                    Some(f) => format!("{}: {} ({}:{})", d.code, d.message, f, d.line),
                    None => format!("{}: {}", d.code, d.message),
                },
            );
        let n = diags.count_at_least(min);
        let message = if n > 1 {
            format!("{first} (+{} more)", n - 1)
        } else {
            first
        };
        Self { message, diags }
    }
}

#[allow(clippy::disallowed_types, clippy::disallowed_methods)]
mod clock {
    /// Wall-clock stopwatch for `meta.load_ms`. Telemetry only: no game logic reads it.
    pub(super) struct Stopwatch(std::time::Instant);

    impl Stopwatch {
        pub(super) fn start() -> Self {
            Self(std::time::Instant::now()) // det-ok: load-time telemetry (meta.load_ms), never read by game logic
        }

        pub(super) fn elapsed_ms(&self) -> u32 {
            u32::try_from(self.0.elapsed().as_millis()).unwrap_or(u32::MAX)
        }
    }
}

/// Files whose absence is Fatal (Plan/14 §8.2), by step. `Masquerade.prj` and `textClient.inc` are checked
/// where they are read.
const REQUIRED_SERVER: &[(&str, &str)] = &[
    ("L10", "mdlObj.inc"),
    ("L11", "mdlDyna.inc"),
    ("L12", "World.inc"),
    ("L13", "propMover.txt"),
    ("L14", "propItem.txt"),
    ("L15", "propSkill.txt"),
    ("L17", "propJob.inc"),
    ("L18", "propCtrl.txt"),
    ("L19", "propSkillAdd.csv"),
    ("L20", "character.inc"),
    ("L22", "expTable.inc"),
    ("L32", "PropMoverEx.inc"),
    ("L33", "jobItem.inc"),
    ("L34", "etc.inc"),
];

/// Additionally required when client data is loaded (P7).
const REQUIRED_CLIENT: &[(&str, &str)] = &[("L95", "resdata.inc"), ("L95", "Client/sound.inc")];

/// A step of the fixed lists (P3-P7). `run: None` is a stub for a later work package.
struct Step {
    id: &'static str,
    phase: Phase,
    file: &'static str,
    run: Option<fn(&mut LoadCtx<'_>)>,
}

const fn step(
    id: &'static str,
    phase: Phase,
    file: &'static str,
    run: Option<fn(&mut LoadCtx<'_>)>,
) -> Step {
    Step {
        id,
        phase,
        file,
        run,
    }
}

fn run_motion(cx: &mut LoadCtx<'_>) {
    props::load_motion(cx, "propMotion.txt");
}

/// The fixed load table (Plan/14 §5.3 L30-L95) in execution order.
const STEPS: &[Step] = &[
    // P3
    step(
        "L30",
        Phase::P3,
        "teleport16.inc",
        Some(ext::load_teleport16),
    ),
    step("L31", Phase::P3, "propMotion.txt", Some(run_motion)),
    step(
        "L32",
        Phase::P3,
        "PropMoverEx.inc",
        Some(props::load_mover_ex),
    ),
    step("L33", Phase::P3, "jobItem.inc", Some(tables::load_job_item)),
    step("L34", Phase::P3, "etc.inc", Some(tables::load_etc)),
    step(
        "L35",
        Phase::P3,
        "propEnchant.inc",
        Some(tables::load_enchant),
    ),
    step(
        "L36",
        Phase::P3,
        "Constant.inc",
        Some(tables::load_constant),
    ),
    step(
        "L37",
        Phase::P3,
        "propGuildQuest.inc",
        Some(quests::load_guild_quests),
    ),
    step(
        "L38",
        Phase::P3,
        "propPartyQuest.inc",
        Some(quests::load_party_quests),
    ),
    step(
        "L39",
        Phase::P3,
        "propDropEvent.inc",
        Some(props::load_drop_event),
    ),
    step("L40", Phase::P3, "propGiftbox.inc", Some(ext::load_giftbox)),
    step(
        "L41",
        Phase::P3,
        "propPackItem.inc",
        Some(ext::load_pack_items),
    ),
    step(
        "L42",
        Phase::P3,
        "DiePenalty.inc",
        Some(tables::load_die_penalty),
    ),
    step(
        "L43",
        Phase::P3,
        "PKSetting.inc",
        Some(tables::load_pk_setting),
    ),
    step(
        "L44",
        Phase::P3,
        "propItemEtc.inc",
        Some(ext::load_item_etc),
    ),
    step(
        "L45",
        Phase::P3,
        "propEvent.inc",
        Some(ext::load_generic_events),
    ),
    step(
        "L46",
        Phase::P3,
        "InvalidName_ENG.inc",
        Some(client::load_names),
    ),
    step("L48", Phase::P3, "pet.inc", Some(ext::load_pet)),
    step(
        "L49",
        Phase::P3,
        "collecting.inc",
        Some(ext::load_collecting),
    ),
    step("L50", Phase::P3, "accessory.inc", Some(ext::load_accessory)),
    step(
        "L51",
        Phase::P3,
        "randomoption.inc",
        Some(ext::load_random_option),
    ),
    step("L52", Phase::P3, "ticket.inc", Some(ext::load_tickets)),
    step(
        "L53",
        Phase::P3,
        "MiniGame_Alphabet.txt",
        Some(ext::load_minigames),
    ),
    step(
        "L54",
        Phase::P3,
        "Ultimate_UltimateWeapon.txt",
        Some(ext::load_ultimate),
    ),
    step(
        "L55",
        Phase::P3,
        "Exchange_Script.txt",
        Some(ext::load_exchange),
    ),
    step("L57", Phase::P3, "s.txt", Some(ext::load_server_script)),
    step(
        "L59",
        Phase::P3,
        "EnvironmentEffect.txt",
        Some(ext::load_environment),
    ),
    step("L60", Phase::P3, "lordevent.inc", Some(ext::load_lord)),
    step(
        "L61",
        Phase::P3,
        "randomeventmonster.inc",
        Some(ext::load_rangda_transform),
    ),
    step("L62", Phase::P3, "Client/Guide.inc", Some(ext::load_guide)),
    step("L64", Phase::P3, "couple.inc", Some(ext::load_couple)),
    step("L65", Phase::P3, "honorList.txt", Some(ext::load_honor)),
    step(
        "L66",
        Phase::P3,
        "guildbuff.inc",
        Some(ext::load_guild_buffs),
    ),
    step(
        "L67",
        Phase::P3,
        "MonsterBuffs.inc",
        Some(ext::load_monster_buffs),
    ),
    step("L68", Phase::P3, "DPSProp.txt", Some(ext::load_dps_props)),
    // P4
    step(
        "L70",
        Phase::P4,
        "WorldDialog.txt",
        Some(ext::load_dialog_text),
    ),
    step("L72", Phase::P4, "spevent.txt", Some(ext::load_spevent)),
    step(
        "L73",
        Phase::P4,
        "GuildCombat.txt",
        Some(ext::load_guild_combat),
    ),
    step("L75", Phase::P4, "election.inc", Some(ext::load_election)),
    // P5
    step("L80", Phase::P5, "World/*", Some(worlds::load_world_data)),
    // P6: the static and live Lua scripts (L47, L56, L58, L63, L71, L74 are read here)
    step("L90", Phase::P6, "*.lua", Some(lua::load_lua_configs)),
    // P7
    step("L95", Phase::P7, "resdata.inc", Some(client::load_client)),
];

fn record(
    log: &mut Vec<StepRecord>,
    id: &'static str,
    phase: Phase,
    file: &str,
    status: StepStatus,
) {
    log.push(StepRecord {
        id,
        phase,
        file: file.to_owned(),
        status,
    });
}

/// Runs the rows of [`STEPS`] of one phase.
fn run_phase(cx: &mut LoadCtx<'_>, log: &mut Vec<StepRecord>, phase: Phase) {
    for s in STEPS.iter().filter(|s| s.phase == phase) {
        match s.run {
            Some(f) => {
                f(cx);
                record(log, s.id, phase, s.file, StepStatus::Done);
            }
            None => record(log, s.id, phase, s.file, StepStatus::Stub),
        }
    }
}

/// Dispatches one entry of the project file (P2). The hard-coded names are the original's: it ignores the
/// project-file argument for `propMover`, `propItem`, `propSkill`, `propTroupeSkill`, `propJob` and
/// `propCtrl`.
fn dispatch_prj(cx: &mut LoadCtx<'_>, log: &mut Vec<StepRecord>, e: &PrjEntry) {
    use StepStatus::{Done, Skipped};
    let p = Phase::P2;
    match e.key {
        PrjKey::Model => {
            models::load_model_file(cx, &e.arg);
            record(log, "L10/L11", p, &e.arg, Done);
        }
        PrjKey::World => {
            worlds::load_world_list(cx, &e.arg);
            record(log, "L12", p, &e.arg, Done);
        }
        PrjKey::Terrain => record(log, "L95", p, &e.arg, Skipped), // client only: P7
        PrjKey::Billboard => record(log, "-", p, &e.arg, Skipped),
        PrjKey::PropMover => {
            props::load_movers(cx);
            record(log, "L13", p, "propMover.txt", Done);
        }
        PrjKey::PropItem => {
            props::load_prop_item(cx, "propItem.txt");
            record(log, "L14", p, "propItem.txt", Done);
        }
        PrjKey::PropSkill => {
            props::load_skills(cx);
            record(log, "L15", p, "propSkill.txt", Done);
        }
        PrjKey::MotionProp => {
            props::load_motion(cx, &e.arg);
            record(log, "L31", p, &e.arg, Done);
        }
        PrjKey::PropTroupeSkill => {
            props::load_party_skills(cx);
            record(log, "L16", p, "propTroupeSkill.txt", Done);
        }
        PrjKey::PropKarma => record(log, "-", p, &e.arg, Skipped), // __VER < 8 only
        PrjKey::PropJob => {
            props::load_jobs(cx);
            record(log, "L17", p, "propJob.inc", Done);
        }
        PrjKey::PropCtrl => {
            props::load_ctrls(cx);
            record(log, "L18", p, "propCtrl.txt", Done);
        }
        PrjKey::PropSkillAdd => {
            props::load_add_skills(cx, &e.arg);
            record(log, "L19", p, &e.arg, Done);
        }
        PrjKey::Character => {
            npcs::load_character(cx, &e.arg);
            record(log, "L20", p, &e.arg, Done);
        }
        PrjKey::PropQuest => {
            quests::load_prop_quest(cx, &e.arg);
            record(log, "L21", p, &e.arg, Done);
        }
        PrjKey::ExpTable => {
            tables::load_exp_table(cx, &e.arg);
            record(log, "L22", p, &e.arg, Done);
        }
        PrjKey::Help => record(log, "L95", p, &e.arg, Skipped), // client only: P7
    }
}

fn fail(cx: LoadCtx<'_>, min: Severity) -> LoadError {
    LoadError::from_diags(cx.diags, min)
}

/// Loads every text resource into an immutable [`GameData`] (Plan/14 §5.1). Single-threaded and
/// sequential; the order is normative (the fix-ups of 14 §5.4 depend on it).
///
/// Returns `Err` only for Fatal conditions: `Masquerade.prj` or another required file is missing, a
/// `propMover` id is duplicated or out of range, or (with `opts.strict`) any Error was reported.
pub fn load_all(vfs: &Vfs, opts: &DataOptions) -> Result<LoadOutput, LoadError> {
    let watch = clock::Stopwatch::start();
    let mut cx = LoadCtx::new(vfs, opts);
    let mut log: Vec<StepRecord> = Vec::new();
    cx.import_vfs_notes();

    // ---- P1 -----------------------------------------------------------------------------------
    let Some(prj_text) = cx.read_text("Masquerade.prj", Need::Required) else {
        return Err(fail(cx, Severity::Fatal));
    };
    p1(&mut cx, &mut log);
    if cx.diags.count_at_least(Severity::Fatal) > 0 {
        return Err(fail(cx, Severity::Fatal));
    }
    check_required(&mut cx);
    if cx.diags.count_at_least(Severity::Fatal) > 0 {
        return Err(fail(cx, Severity::Fatal));
    }

    // ---- P2: the project file drives the loaders in file order ---------------------------------
    let entries = parse_prj(&prj_text, "masquerade.prj");
    for e in &entries {
        dispatch_prj(&mut cx, &mut log, e);
        if cx.diags.count_at_least(Severity::Fatal) > 0 {
            return Err(fail(cx, Severity::Fatal)); // `return FALSE` of OpenProject
        }
    }
    npcs::finalize_vendor_stock(&mut cx); // F7: the end of L20

    // ---- P3-P7 ---------------------------------------------------------------------------------
    for phase in [Phase::P3, Phase::P4, Phase::P5, Phase::P6] {
        run_phase(&mut cx, &mut log, phase);
        if cx.diags.count_at_least(Severity::Fatal) > 0 {
            return Err(fail(cx, Severity::Fatal));
        }
    }
    let client = if opts.load_client_data {
        run_phase(&mut cx, &mut log, Phase::P7);
        Some(std::mem::take(&mut cx.client))
    } else {
        record(
            &mut log,
            "L95",
            Phase::P7,
            "(client data not requested)",
            StepStatus::Skipped,
        );
        None
    };
    if cx.diags.count_at_least(Severity::Fatal) > 0 {
        return Err(fail(cx, Severity::Fatal));
    }

    // ---- P8 ------------------------------------------------------------------------------------
    let drift = define_drift(&cx.gd.defines);
    for d in drift {
        cx.diags.push(d);
    }
    record(&mut log, "P8", Phase::P8, "validation", StepStatus::Done);
    finalize(&mut cx, watch.elapsed_ms());
    if opts.strict && cx.diags.count_at_least(Severity::Error) > 0 {
        return Err(fail(cx, Severity::Error));
    }
    let LoadCtx { diags, gd, .. } = cx;
    Ok(LoadOutput {
        game: gd,
        client,
        diags,
        steps: log,
    })
}

/// P1: L01 defines, L02 strings (`LoadPreFiles`), L03 `textClient.inc`.
fn p1(cx: &mut LoadCtx<'_>, log: &mut Vec<StepRecord>) {
    let pre = load_pre_files(|name| cx.read_text(name, Need::Silent));
    cx.diags.extend(pre.diags);
    let defines = Arc::new(pre.defines);
    let strings = Arc::new(pre.strings);
    cx.gd.meta.counts.defines = u32::try_from(defines.len()).unwrap_or(u32::MAX);
    cx.gd.meta.counts.string_keys = u32::try_from(strings.len()).unwrap_or(u32::MAX);
    cx.gd.defines = Arc::clone(&defines);
    cx.gd.strings = Arc::clone(&strings);
    cx.script = Arc::new(ScriptCtx::new(defines, strings, ScriptVariant::Server));
    record(log, "L01", Phase::P1, "define headers", StepStatus::Done);
    record(log, "L02", Phase::P1, "string tables", StepStatus::Done);
    steps::load_texts(cx, "textClient.inc");
    record(log, "L03", Phase::P1, "textClient.inc", StepStatus::Done);
}

/// Reports every missing required file (Fatal `FileMissingRequired`) in one go.
fn check_required(cx: &mut LoadCtx<'_>) {
    let client = cx.opts.load_client_data;
    let list = REQUIRED_SERVER
        .iter()
        .chain(REQUIRED_CLIENT.iter().filter(|_| client));
    for (id, file) in list {
        if !cx.exists(file) {
            cx.report(
                Severity::Fatal,
                DiagCode::FileMissingRequired,
                Some(file),
                0,
                format!("{file}: required file is missing (load step {id})"),
            );
        }
    }
}

/// P8: counts from the diagnostics, the source table, the fingerprint and the diagnostics digest.
fn finalize(cx: &mut LoadCtx<'_>, load_ms: u32) {
    let fingerprint = data_fingerprint(cx.source_records(), GAMEDATA_LOADER_VERSION).0;
    let sources = cx.source_infos();
    let overflow = cx.diags.count_code(DiagCode::NumberOverflow);
    let range = cx.diags.count_code(DiagCode::IndexOutOfRange);
    let capacity = cx.diags.count_code(DiagCode::CapacityExceeded);
    let divergence = cx.diags.count_code(DiagCode::ClientVariantDivergence);
    let summary = cx.diags.summary();
    let m = &mut cx.gd.meta;
    m.loader_version = GAMEDATA_LOADER_VERSION;
    m.fingerprint = fingerprint;
    m.sources = sources;
    m.counts.number_overflow = overflow;
    m.counts.index_out_of_range = range;
    m.counts.capacity_exceeded = capacity;
    m.counts.client_variant_divergence = divergence;
    m.diag_summary = summary;
    m.load_ms = load_ms;
}
