//! `honor` and `tables`.

use super::Sources;
use crate::bundle::BundleError;
use crate::bundle::types::{
    CoupleSection, ExpSection, GenericEventRow, GuildSection, HonorBundle, HonorTitleRow,
    JobItemsRow, JobRow, LordEventRate, LordEventTables, PkSection, SkillPointCosts, TablesBundle,
    Teleport16Row,
};
use crate::job_item::MAX_BEGINEQUIP;
use crate::props::job::MAX_JOB;
use crate::types::NULL_ID;

/// `honor`: the honor titles in file order.
pub fn project_honor(src: &Sources<'_>) -> Result<HonorBundle, BundleError> {
    Ok(HonorBundle {
        titles: src
            .game
            .honor
            .titles
            .iter()
            .map(|t| HonorTitleRow {
                id: t.id,
                large_group: t.large_group,
                small_group: t.small_group,
                need: t.need,
                title: t.title.clone(),
            })
            .collect(),
    })
}

/// `tables`: jobs, start equipment, skill-point costs, experience, guild, couple, PK, emotes, structure names,
/// teleport16 (with the client level rule), lord skills and generic events.
pub fn project_tables(src: &Sources<'_>) -> Result<TablesBundle, BundleError> {
    let g = src.game;
    let jobs = (0..MAX_JOB as u32)
        .filter_map(|id| {
            let info = g.jobs.info(id)?;
            Some(JobRow {
                id,
                name: info.name.clone(),
                base: info.job_base,
                kind: info.job_type,
                prop: g.jobs.prop(id).cloned(),
            })
        })
        .collect();
    let job_items = g
        .job_items
        .jobs
        .iter()
        .map(|(job, items)| {
            let used = items
                .equip
                .iter()
                .rposition(|p| p[0] != NULL_ID || p[1] != NULL_ID)
                .map_or(0, |i| i + 1);
            JobItemsRow {
                job: *job,
                equip: items
                    .equip
                    .iter()
                    .take(used.min(MAX_BEGINEQUIP))
                    .copied()
                    .collect(),
            }
        })
        .collect();
    let c = &g.constant;
    let e = &g.exp;
    let pk = &g.pk;
    Ok(TablesBundle {
        jobs,
        job_items,
        constants: SkillPointCosts {
            vag_sp: c.vag_sp,
            expert_sp: c.expert_sp,
            pro_sp: c.pro_sp,
        },
        exp: ExpSection {
            character: e
                .character
                .iter()
                .map(|r| [r.exp1, r.exp2, i64::from(r.lp_point), r.limit_exp])
                .collect(),
            party: e.party.iter().map(|r| [r.exp, r.point]).collect(),
            party_skill: e.party_skill.iter().map(|r| [r.exp, r.level]).collect(),
            upitem: e.upitem.iter().map(|col| col.to_vec()).collect(),
            set_item_avail: e.set_item_avail.clone(),
        },
        guild: GuildSection {
            levels: g
                .guild
                .levels
                .iter()
                .map(|l| [l.pxp_count, l.penya, l.max_member])
                .collect(),
            buffs: g.guild.buffs.clone(),
        },
        couple: CoupleSection {
            level_exp: g.couple.level_exp.clone(),
            skill_kinds: g.couple.skill_kinds.clone(),
            skill_levels: g.couple.skill_levels.clone(),
        },
        pk: PkSection {
            general_color: pk.general_color,
            ready_color: pk.ready_color,
            chao_color: pk.chao_color,
            limit_level: pk.limit_level,
            penalties: pk.penalties.clone(),
        },
        emotes: g.emotes.iter().map(|(_, m)| m.clone()).collect(),
        structures: g.etc.structures.clone(),
        guild_appellations: g.etc.guild_appellations.clone(),
        teleports16: g
            .teleports16
            .iter()
            .map(|t| Teleport16Row {
                world: t.world,
                pos: t.pos,
                name: t.name.clone(),
                desc: t.desc.clone(),
                image: t.image.clone(),
                boss: t.boss,
                min_level: t.min_level,
                // the client rule of 14 §10.6: `max < min` is `MAX_LEGEND_LEVEL` (129)
                max_level: t.effective_max_level(),
                class: t.class,
                cooldown: t.cooldown,
                boss_drops: t.boss_drops.clone(),
            })
            .collect(),
        lord_skills: g.lord.skills.clone(),
        // ---- lord (ui-lord, M7) ----
        lord_events: LordEventTables {
            exp: g
                .lord
                .event_exp
                .iter()
                .map(|&(factor, cost)| LordEventRate { factor, cost })
                .collect(),
            item: g
                .lord
                .event_item
                .iter()
                .map(|&(factor, cost)| LordEventRate { factor, cost })
                .collect(),
        },
        generic_events: g
            .generic_events
            .iter()
            .map(|e| GenericEventRow {
                id: e.id,
                title: e.title.clone(),
                begin: e.begin.to_vec(),
                end: e.end.to_vec(),
            })
            .collect(),
    })
}
