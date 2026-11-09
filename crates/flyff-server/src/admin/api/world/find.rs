//! `GET /admin/api/world/find?q=` (Plan/25 §3.9 "Find on map"; WP-ADMIN-10): what the toolbar's search box asks when the target is not in the
//! subscription of the map.
//!
//! * **Live hits** come from the game thread (`AdminQuery::Find`: online players by name or character id, monsters and NPCs by name,
//!   prop id or `character.inc` key, at most [`LIVE_LIMIT`]): kind, object id, world, layer, position, level.
//! * **Static hits** are the titled areas (regions, portals, revival points) and the `teleport16` places of every loaded world, matched by
//!   name. They come from the same documents as `GET /admin/api/world/{id}/meta`, built once per process (the data does not change) into
//!   an index of `(world, name, position)`; at most [`STATIC_LIMIT`] are returned, after the live ones.
//!
//! Matching is a case-insensitive substring; an exact name ranks before a prefix before a contains.

use super::meta::{self, WorldEnv};
use flyff_game::admin::queries::world::FindHit;
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Most live hits (the game thread's own cap).
pub const LIVE_LIMIT: usize = flyff_game::admin::queries::world::FIND_LIMIT;
/// Most static hits.
pub const STATIC_LIMIT: usize = 20;
/// The shortest query that is looked up (a single letter would match half the world).
pub const MIN_QUERY: usize = 2;

/// One hit of `GET /admin/api/world/find`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FindHitDoc {
    /// `player`, `monster`, `npc`, `pet`, `region` or `location`.
    pub kind: String,
    /// The object id of a live hit, `0` for a region or a location.
    pub obj_id: u32,
    /// The character id of a player, `0` otherwise.
    pub character_id: u32,
    /// The prop id of a live hit.
    pub prop: u32,
    /// The display name.
    pub name: String,
    /// A second line: the description of a region, where a portal leads, the kind of a location.
    pub detail: String,
    /// The world.
    pub world: u32,
    /// The layer (`0` for a static hit).
    pub layer: i32,
    /// Position x.
    pub x: f32,
    /// Position z.
    pub z: f32,
    /// The level of a player or a monster.
    pub level: u32,
}

/// `GET /admin/api/world/find`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FindDoc {
    /// Live hits first (players, monsters, NPCs), then regions and places.
    pub hits: Vec<FindHitDoc>,
}

/// One titled place of a world.
#[derive(Debug)]
pub struct StaticEntry {
    world: u32,
    kind: &'static str,
    name: String,
    lower: String,
    detail: String,
    x: f32,
    z: f32,
}

fn mid(rect: &[i32; 4]) -> (f32, f32) {
    (
        (rect[0] as f32 + rect[2] as f32) / 2.0,
        (rect[1] as f32 + rect[3] as f32) / 2.0,
    )
}

/// Builds the index from the meta documents of every loadable world. Duplicates (the same title in the same world) keep the first.
pub fn build_index(env: &WorldEnv<'_>) -> Vec<StaticEntry> {
    let worlds = meta::worlds_doc(env, &BTreeMap::new());
    let mut out = Vec::new();
    for w in &worlds.worlds {
        let Some(doc) = meta::meta_doc(env, w.id) else {
            continue;
        };
        let mut seen: BTreeSet<(&'static str, String)> = BTreeSet::new();
        let mut push = |kind: &'static str, name: &str, detail: String, x: f32, z: f32| {
            let name = name.trim();
            if name.is_empty() || !seen.insert((kind, name.to_lowercase())) {
                return;
            }
            out.push(StaticEntry {
                world: w.id,
                kind,
                name: name.to_owned(),
                lower: name.to_lowercase(),
                detail,
                x,
                z,
            });
        };
        for r in &doc.regions {
            if let Some(title) = &r.title {
                let (x, z) = mid(&r.rect);
                push("region", title, r.desc.clone().unwrap_or_default(), x, z);
            }
        }
        for p in &doc.portals {
            if let Some(title) = &p.title {
                let (x, z) = mid(&p.rect);
                push("region", title, format!("to {}", p.to_name), x, z);
            }
        }
        for r in &doc.revival {
            if let Some(title) = &r.title {
                push(
                    "location",
                    title,
                    "revival point".to_owned(),
                    r.pos[0],
                    r.pos[2],
                );
            }
        }
        for t in &doc.teleports {
            push(
                "location",
                &t.name,
                "teleport point".to_owned(),
                t.pos[0],
                t.pos[2],
            );
        }
    }
    out
}

/// The index of this process (built on first use; the data is immutable once the server runs).
pub fn static_index(env: &WorldEnv<'_>) -> &'static [StaticEntry] {
    static INDEX: OnceLock<Vec<StaticEntry>> = OnceLock::new();
    INDEX.get_or_init(|| build_index(env))
}

/// A hit's sort rank: exact name, prefix, the rest.
fn rank(name_lower: &str, q: &str) -> Option<u8> {
    if name_lower == q {
        Some(0)
    } else if name_lower.starts_with(q) {
        Some(1)
    } else if name_lower.contains(q) {
        Some(2)
    } else {
        None
    }
}

/// The static hits of `q` (already lower-cased and trimmed), best match first, at most `limit`.
pub fn static_hits(index: &[StaticEntry], q: &str, limit: usize) -> Vec<FindHitDoc> {
    let mut found: Vec<(u8, &StaticEntry)> = index
        .iter()
        .filter_map(|e| rank(&e.lower, q).map(|r| (r, e)))
        .collect();
    found.sort_by(|a, b| (a.0, &a.1.lower, a.1.world).cmp(&(b.0, &b.1.lower, b.1.world)));
    found
        .into_iter()
        .take(limit)
        .map(|(_, e)| FindHitDoc {
            kind: e.kind.to_owned(),
            obj_id: 0,
            character_id: 0,
            prop: 0,
            name: e.name.clone(),
            detail: e.detail.clone(),
            world: e.world,
            layer: 0,
            x: e.x,
            z: e.z,
            level: 0,
        })
        .collect()
}

/// A live hit as the document has it.
pub fn live_hit(h: &FindHit) -> FindHitDoc {
    use flyff_game::admin::map::EntityKind;
    let kind = match h.kind {
        EntityKind::Player => "player",
        EntityKind::Monster => "monster",
        EntityKind::Npc => "npc",
        EntityKind::Pet => "pet",
        EntityKind::Item | EntityKind::Ctrl => "npc",
    };
    FindHitDoc {
        kind: kind.to_owned(),
        obj_id: h.obj_id,
        character_id: h.character_id,
        prop: h.prop,
        name: h.name.clone(),
        detail: String::new(),
        world: h.world,
        layer: h.layer,
        x: h.x,
        z: h.z,
        level: h.level,
    }
}

/// The query text the game thread gets, or `None` for one that is too short to look up.
pub fn clean_query(q: &str) -> Option<String> {
    let q = q.trim();
    (q.chars().count() >= MIN_QUERY).then(|| q.to_owned())
}

/// The live half: `AdminQuery::Find` through `game`, as documents.
pub async fn live_hits<G: super::super::actions::GameLink>(
    game: &G,
    q: &str,
) -> Result<Vec<FindHitDoc>, crate::admin::error::ApiError> {
    use crate::admin::error::ApiError;
    match game.query(AdminQuery::Find(q.to_owned())).await? {
        AdminQueryResult::Find(hits) => Ok(hits.iter().map(live_hit).collect()),
        AdminQueryResult::Failed(f) => Err(ApiError::from(&f)),
        other => Err(ApiError::internal(&format!("Find answered {other:?}"))),
    }
}

/// Declares the types of this module for the generated TypeScript of the DTO catalogue (feature `ts`).
#[cfg(feature = "ts")]
pub fn declare_ts(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;
    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }
    push::<FindHitDoc>(out, cfg);
    push::<FindDoc>(out, cfg);
}
