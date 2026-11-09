//! The data report of `flyff check` (Plan/14 §9, WP-DATA-17): the reference counts C01-C36 of this repack's
//! `Resource/`, the spot checks of §9.2 and the report type that the cross-reference checks of §9.3
//! ([`super::xref`]) fill in. Wasm-safe: nothing here touches the file system.
//!
//! With `--expect-reference` every row with a reference value and every spot check must match (exit code 1
//! otherwise); without the flag the values are only printed, because modded data legitimately differs.
//! The reference values are the plan's numbers except where the real data (measured by the corpus tests,
//! `NOTES.md`) differs from the plan text:
//!
//! * C01 defines 18,387 (the plan's 18,330 missed 57 literals with an `L` suffix),
//! * C05 as entries / stored texts (2,975 / 2,965),
//! * C10 `propCtrl` 78 rows,
//! * C13 gift-box blocks 141 / 0 / 21 / 7 / 1 / 26 and C14 pack blocks 440 (the plan's 142 and 460 also count
//!   sample blocks inside `/* */`),
//! * C34 `NumberOverflow` 6 (the `expFlight` rows 98-100).

use crate::game_data::{DataCounts, GameData};
use flyff_common::defs;
use flyff_script::{DiagSummary, Diagnostic};
use serde::Serialize;

/// One row of the data report: a plan id (`C01` ...), what is counted, the value found and the reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CountRow {
    /// `C01`...
    pub id: &'static str,
    /// What the row counts.
    pub label: &'static str,
    /// The value found (`a / b / c` for rows with several numbers).
    pub value: String,
    /// The reference of this repack's Resource, when the plan has one.
    pub reference: Option<&'static str>,
}

impl CountRow {
    /// `true` when there is a reference and the value differs from it.
    pub fn mismatch(&self) -> bool {
        self.reference.is_some_and(|r| r != self.value)
    }
}

fn row(
    id: &'static str,
    label: &'static str,
    value: impl Into<String>,
    reference: &'static str,
) -> CountRow {
    CountRow {
        id,
        label,
        value: value.into(),
        reference: Some(reference),
    }
}

/// `a / b / c`.
pub fn slash(vals: &[u32]) -> String {
    vals.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
}

/// The rows of Plan/14 §9.1 that `DataCounts` carries, with the reference values of the real Resource.
pub fn count_rows(c: &DataCounts) -> Vec<CountRow> {
    vec![
        row("C01", "distinct defines", c.defines.to_string(), "18387"),
        row("C04", "string keys", c.string_keys.to_string(), "27374"),
        row(
            "C05",
            "textClient entries / stored TIDs",
            slash(&[c.text_entries, c.text_tids]),
            "2975 / 2965",
        ),
        row(
            "C06",
            "propItem rows / unique ids / max id",
            slash(&[c.item_rows, c.item_unique_ids, c.item_max_id]),
            "5283 / 5282 / 224897",
        ),
        row(
            "C07",
            "propSkill / propTroupeSkill rows",
            slash(&[c.skill_rows, c.party_skill_rows]),
            "190 / 8",
        ),
        row(
            "C08",
            "propSkillAdd rows / max SA id / empty fields",
            slash(&[
                c.add_skill_rows,
                c.add_skill_max_id,
                c.add_skill_empty_fields,
            ]),
            "2301 / 4504 / 1",
        ),
        row(
            "C09",
            "propMover rows / max MI id / movers.size()",
            slash(&[c.mover_rows, c.mover_max_id, c.mover_size]),
            "783 / 1165 / 1166",
        ),
        row(
            "C10",
            "propCtrl / propMotion / propJob rows",
            slash(&[c.ctrl_rows, c.motion_rows, c.job_rows]),
            "78 / 27 / 32",
        ),
        row(
            "C11",
            "propMoverEx blocks / of unknown movers",
            slash(&[c.mover_ex_blocks, c.mover_ex_unknown]),
            "609 / 0",
        ),
        row(
            "C12",
            "NPCs (character / -etc / -school)",
            slash(&c.npc_rows),
            "366 / 7 / 30",
        ),
        row(
            "C13",
            "gift-box blocks (GiftBox / 2 / 3 / 4 / 5 / 6)",
            slash(&c.giftbox_blocks),
            "141 / 0 / 21 / 7 / 1 / 26",
        ),
        row("C14", "PackItem blocks", c.pack_blocks.to_string(), "440"),
        row(
            "C15",
            "propItemEtc Piercing / SetItem / RandomOptItem",
            slash(&c.item_etc),
            "40 / 134 / 48",
        ),
        row(
            "C16",
            "propDropEvent DropItem lines",
            c.drop_event_rows.to_string(),
            "200",
        ),
        row(
            "C17",
            "World.inc entries / loadable worlds / folders",
            slash(&[c.world_defs, c.worlds_loadable, c.world_folders]),
            "57 / 51 / 33",
        ),
        row(
            "C18",
            "quests / guild quests / party quests",
            slash(&c.quests),
            "0 / 0 / 0",
        ),
        row(
            "C19",
            "except: world-drop excluded / patches",
            slash(&[c.except_world_drop_excluded, c.except_patches]),
            "0 / 0",
        ),
        row(
            "C21",
            "resdata applets / controls / empty blocks / unknown names",
            slash(&[
                c.resdata_applets,
                c.resdata_controls,
                c.resdata_empty_blocks,
                c.resdata_unknown_names,
            ]),
            "365 / 2809 / 20 / 4",
        ),
        row("C22", "regions", c.regions.to_string(), "190"),
        row("C23", "respawns", c.respawns.to_string(), "1579"),
        row(
            "C24",
            "model entries / motion references",
            slash(&[c.model_elems, c.model_motion_refs]),
            "7299 / 17696",
        ),
        row(
            "C26",
            "sound.inc entries / ids / max id / empty",
            slash(&[c.sound_entries, c.sound_ids, c.sound_max_id, c.sound_empty]),
            "1667 / 1665 / 3608 / 179",
        ),
        row(
            "C27",
            "default.bgm entries",
            c.music_entries.to_string(),
            "42",
        ),
        row(
            "C28",
            "WorldDialog lines",
            c.dialog_lines.to_string(),
            "1250",
        ),
        row(
            "C29",
            "honor titles / lord skills / teleport16 / guild buffs / monster-buff pairs",
            slash(&[
                c.honor_titles,
                c.lord_skills,
                c.teleport16_rows,
                c.guild_buffs,
                c.monster_buff_pairs,
            ]),
            "58 / 8 / 17 / 4 / 127",
        ),
        row(
            "C30",
            "propEvent active events",
            c.generic_events.to_string(),
            "3",
        ),
        row(
            "C31",
            "tickets / layers",
            slash(&[c.tickets, c.ticket_layers]),
            "5 / 2",
        ),
        row(
            "C32",
            "atlases (Theme / Icon / SFX)",
            slash(&c.atlases),
            "27 / 9 / 3",
        ),
        row(
            "C33",
            "client-keyword divergences",
            c.client_variant_divergence.to_string(),
            "0",
        ),
        row(
            "C34",
            "NumberOverflow / IndexOutOfRange / CapacityExceeded",
            slash(&[c.number_overflow, c.index_out_of_range, c.capacity_exceeded]),
            "6 / 0 / 0",
        ),
        row(
            "C36",
            "world-map monster records / rainbow markers",
            slash(&[c.map_monster_records, c.rainbow_markers]),
            "78 / 7",
        ),
    ]
}

/// The rows C02 and C03 of Plan/14 §9.1: the level caps and the two defines that must not exist (they come from
/// the define table, not from `DataCounts`).
pub fn define_rows(defines: &flyff_script::Defines) -> Vec<CountRow> {
    let v = |n: &str| {
        defines
            .value_u32(n)
            .map_or_else(|| "undefined".to_owned(), |v| v.to_string())
    };
    vec![
        row(
            "C02",
            "MAX_LEVEL / MAX_LEGEND_LEVEL / MAX_MONSTER_LEVEL / MAX_JOB",
            format!(
                "{} / {} / {} / {}",
                v("MAX_LEVEL"),
                v("MAX_LEGEND_LEVEL"),
                v("MAX_MONSTER_LEVEL"),
                v("MAX_JOB")
            ),
            "120 / 129 / 160 / 32",
        ),
        row(
            "C03",
            "JOB_ALL / HM_SATISFIED in the define table",
            format!("{} / {}", v("JOB_ALL"), v("HM_SATISFIED")),
            "undefined / undefined",
        ),
    ]
}

/// One spot check of Plan/14 §9.2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SpotCheck {
    /// What is checked (`item II_WEA_SWO_WOODEN`).
    pub name: String,
    /// All the asserted fields matched.
    pub ok: bool,
    /// The fields that differ (empty when `ok`), as `field: found, expected`.
    pub detail: String,
}

/// Collects the differences of one spot check.
struct Diff(Vec<String>);

impl Diff {
    fn eq<T: PartialEq + std::fmt::Debug>(&mut self, field: &str, found: T, expected: T) {
        if found != expected {
            self.0
                .push(format!("{field}: {found:?}, expected {expected:?}"));
        }
    }

    fn finish(self, name: impl Into<String>) -> SpotCheck {
        SpotCheck {
            name: name.into(),
            ok: self.0.is_empty(),
            detail: self.0.join("; "),
        }
    }
}

fn d(name: &str) -> u32 {
    defs::lookup(name).map_or(u32::MAX, |v| v as u32)
}

/// The spot checks of §9.2 (asserted with `--expect-reference`) plus the Constant.inc check C20.
pub fn spot_checks(g: &GameData) -> Vec<SpotCheck> {
    let mut out = Vec::new();

    // II_WEA_SWO_WOODEN
    let id = d("II_WEA_SWO_WOODEN");
    out.push(match g.items.get(id) {
        None => missing("item II_WEA_SWO_WOODEN"),
        Some(p) => {
            let mut x = Diff(Vec::new());
            x.eq("ver", p.ver, 6);
            x.eq(
                "kinds",
                (p.item_kind1, p.item_kind2, p.item_kind3),
                (d("IK1_WEAPON"), d("IK2_WEAPON_DIRECT"), d("IK3_SWD")),
            );
            x.eq("job", p.item_job, d("JOB_VAGRANT"));
            x.eq("cost", p.cost, 50_000);
            x.eq("endurance", p.endurance, 7_200_000);
            x.eq("handed", p.handed, d("HD_ONE"));
            x.eq("parts", p.parts, d("PARTS_RWEAPON"));
            x.eq("item_lv", p.item_lv, 1);
            x.eq("ability", (p.ability_min, p.ability_max), (1, 9));
            x.eq("attack_speed", p.attack_speed, 0.085);
            x.eq("dest_param[0]", p.dest_param[0], d("DST_ATTACKSPEED"));
            x.eq("icon", p.icon.as_str(), "itm_WeaSwoWooden.dds");
            x.finish("item II_WEA_SWO_WOODEN")
        }
    });

    // SI_VAG_ONE_CLEANHIT
    let id = d("SI_VAG_ONE_CLEANHIT");
    out.push(match g.skills.get(id) {
        None => missing("skill SI_VAG_ONE_CLEANHIT"),
        Some(s) => {
            let mut x = Diff(Vec::new());
            x.eq(
                "kinds",
                (s.item_kind1, s.item_kind2, s.item_kind3),
                (d("JTYPE_BASE"), d("JOB_VAGRANT"), d("DIS_VAGRANT")),
            );
            x.eq("req_dis_lv", s.req_dis_lv, 1);
            x.eq("expert_max", s.expert_max, 10);
            x.eq("sub_define", s.sub_define, d("SA_CLEANHIT_L01"));
            x.eq("combo_style", s.combo_style, d("CT_STEP"));
            x.finish("skill SI_VAG_ONE_CLEANHIT")
        }
    });

    // MI_AIBATT1
    let id = d("MI_AIBATT1");
    out.push(match g.movers.get(id) {
        None => missing("mover MI_AIBATT1"),
        Some(m) => {
            let mut x = Diff(Vec::new());
            x.eq(
                "atk1..3",
                (m.atk1, m.atk2, m.atk3),
                (
                    d("II_WEA_MOB_AIBATT1_ATK1"),
                    d("II_WEA_MOB_AIBATT1_ATK2"),
                    d("II_WEA_MOB_AIBATT1_ATK3"),
                ),
            );
            x.eq("atk4", m.atk4, 0xFFFF_FFFF);
            x.eq("frame", m.frame, 1.0);
            x.eq("speed", m.speed, 0.075);
            x.eq(
                "resists (elec, fire, wind, water, earth)",
                (
                    m.resist_electricity,
                    m.resist_fire,
                    m.resist_wind,
                    m.resist_water,
                    m.resist_earth,
                ),
                (0, -30, 0, 0, 30),
            );
            x.finish("mover MI_AIBATT1")
        }
    });

    // exp.character
    {
        let e = &g.exp.character;
        let at = |i: usize| e.get(i).map(|r| r.exp1);
        let mut x = Diff(Vec::new());
        x.eq("L1->2", at(2), Some(14));
        x.eq("L59->60", at(60), Some(9_421_875));
        x.eq("L119->120", at(120), Some(20_291_626_883));
        x.eq("L120->121", at(121), Some(22_353_256_174));
        out.push(x.finish("exp.character"));
    }

    // npcs["mafl_mikyel"]
    out.push(match g.npc("mafl_mikyel") {
        None => missing("npc mafl_mikyel"),
        Some(n) => {
            let mut x = Diff(Vec::new());
            x.eq("name non-empty", !n.name.is_empty(), true);
            x.eq("dialog_key", n.dialog_key.as_deref(), Some("mafl_mikyel"));
            x.eq("structure", n.structure, d("SRT_QUESTOFFICE") as i32);
            x.finish("npc mafl_mikyel")
        }
    });

    // C20: no Constant.inc block applies (rates 1.0, SP 1/2/3)
    {
        let c = &g.constant;
        let mut x = Diff(Vec::new());
        x.eq("applied block", c.applied_lang_block, None);
        x.eq(
            "rates",
            (
                c.item_drop_rate,
                c.monster_exp_rate,
                c.monster_hit_rate,
                c.gold_drop_rate,
            ),
            (1.0, 1.0, 1.0, 1.0),
        );
        x.eq("sp", (c.vag_sp, c.expert_sp, c.pro_sp), (1, 2, 3));
        out.push(x.finish("Constant.inc (C20)"));
    }
    out
}

fn missing(name: &str) -> SpotCheck {
    SpotCheck {
        name: name.to_owned(),
        ok: false,
        detail: "the entry does not exist".to_owned(),
    }
}

/// The result of one cross-reference check of §9.3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct XrefResult {
    /// `X1`..`X7` (the numbered checks of §9.3, `a`/`b` for the parts of check 4).
    pub id: &'static str,
    /// What is checked.
    pub label: &'static str,
    /// The severity of a finding (§9.3: Warning, Error or Info).
    pub severity: flyff_script::Severity,
    /// How many references were checked.
    pub checked: u32,
    /// How many do not resolve.
    pub missing: u32,
    /// The first few unresolved references.
    pub samples: Vec<String>,
}

/// How many unresolved references a result keeps as samples.
pub const XREF_SAMPLES: usize = 5;

impl XrefResult {
    /// A result with no finding yet.
    pub fn new(id: &'static str, label: &'static str, severity: flyff_script::Severity) -> Self {
        Self {
            id,
            label,
            severity,
            checked: 0,
            missing: 0,
            samples: Vec::new(),
        }
    }

    /// Counts one checked reference; `what` names it when `ok` is false (evaluated lazily).
    pub fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.checked = self.checked.saturating_add(1);
        if !ok {
            self.missing = self.missing.saturating_add(1);
            if self.samples.len() < XREF_SAMPLES {
                self.samples.push(what());
            }
        }
    }
}

/// The whole report of `flyff check` for the game data.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DataReport {
    /// The count rows (`C01`..), the define rows spliced in after `C01`.
    pub rows: Vec<CountRow>,
    /// The spot checks of §9.2.
    pub spot_checks: Vec<SpotCheck>,
    /// The cross-reference checks of §9.3 (empty without a VFS).
    pub xref: Vec<XrefResult>,
    /// What the registered `extra_checks` hooks (docs 15 and 20) found.
    pub extra: Vec<Diagnostic>,
    /// The diagnostics digest of the load.
    pub diagnostics: DiagSummary,
    /// The data fingerprint as 32 hex digits.
    pub fingerprint: String,
    /// Load time of `load_all` in ms (telemetry; not part of any comparison).
    pub load_ms: u32,
}

impl DataReport {
    /// The report of a loaded `GameData` without the VFS-dependent parts.
    pub fn from_game(g: &GameData) -> Self {
        let mut rows = count_rows(&g.meta.counts);
        rows.splice(1..1, define_rows(&g.defines));
        Self {
            rows,
            spot_checks: spot_checks(g),
            xref: Vec::new(),
            extra: Vec::new(),
            diagnostics: g.meta.diag_summary.clone(),
            fingerprint: format!("{:032x}", g.meta.fingerprint),
            load_ms: g.meta.load_ms,
        }
    }

    /// The `--expect-reference` findings: every count row that differs from its reference and every failed
    /// spot check, as one line each. Empty means exit code 0.
    pub fn mismatches(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .rows
            .iter()
            .filter(|r| r.mismatch())
            .map(|r| {
                format!(
                    "{} {}: {} (reference {})",
                    r.id,
                    r.label,
                    r.value,
                    r.reference.unwrap_or("-")
                )
            })
            .collect();
        out.extend(
            self.spot_checks
                .iter()
                .filter(|s| !s.ok)
                .map(|s| format!("spot check {}: {}", s.name, s.detail)),
        );
        out
    }

    /// `true` when [`Self::mismatches`] is empty.
    pub fn matches_reference(&self) -> bool {
        self.rows.iter().all(|r| !r.mismatch()) && self.spot_checks.iter().all(|s| s.ok)
    }
}
