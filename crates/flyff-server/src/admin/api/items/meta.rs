//! `GET /admin/api/items/catalog-meta` and `POST /admin/api/items/validate` (Plan/25 §5.1, §5.2, §11.3; WP-ADMIN-20).
//!
//! The catalogue itself (names, icons, levels) is the client's `items` bundle; the server adds what the bundle
//! cannot say: the **kind tree with readable names**, the **fields of every item** (as a few distinct profiles plus
//! an `(item, profile)` list, so the builder shows exactly the fields the validation accepts), the ranges and the
//! switches that decide info chips, and the ultimate gem abilities.

use super::{ApiState, parse_json};
use crate::admin::auth::Who;
use crate::admin::dto_items::{
    AbilityDto, CatalogMeta, ItemFeatures, ItemFieldsDto, ItemLimits, KindNode, NamedId,
};
use crate::admin::error::ApiResult;
use crate::admin::grants::{ValidateCx, fields_dto};
use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use flyff_common::defs::itemkind::IK3_VIRTUAL;
use flyff_data::GameData;
use flyff_game::admin::actions::grants as g;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Readable name of an `IK1_*`.
fn ik1_name(v: u32) -> String {
    match v {
        0 => "Penya",
        1 => "Weapons",
        2 => "Armour",
        3 => "General",
        4 => "Rides",
        5 => "System",
        6 => "Cash items",
        7 => "Housing",
        _ => return format!("Class {v}"),
    }
    .to_owned()
}

/// Readable name of an `IK2_*`.
fn ik2_name(v: u32) -> String {
    match v {
        0 => "Penya",
        1 => "Hands",
        2 => "Melee weapons",
        3 => "Magic weapons",
        7 => "Armour",
        8 => "Armour accessories",
        9 => "Fashion",
        10 => "Fashion accessories",
        11 => "Refreshers",
        12 => "Potions",
        13 => "Jewelry",
        14 => "Food",
        15 => "Magic items",
        16 => "Gems",
        17 => "Materials",
        18 => "Tools",
        19 => "System items",
        20 => "Riding",
        21 => "Monster items",
        22 => "Blinkwings",
        23 => "Fuel",
        24 => "Charms",
        25 => "Ammunition",
        26 => "Texts",
        27 => "GM texts",
        28 => "General",
        29 | 33 => "Buffs",
        30 => "Warp items",
        31 => "Skills",
        32 => "Wigs",
        34 => "Furniture",
        35 => "Papering",
        36 => "To cash",
        37 => "Gift buffs",
        38..=41 => "Guild house",
        _ => return format!("Kind {v}"),
    }
    .to_owned()
}

/// Readable name of an `IK3_*`.
fn ik3_name(v: u32) -> String {
    match v {
        0 => "Penya",
        1 => "Hand",
        2 => "Swords",
        3 => "Axes",
        4 => "Cheer sticks",
        5 => "Knuckle hammers",
        6 => "Wands",
        7 => "Staves",
        8 => "Two-handed swords",
        9 => "Two-handed axes",
        10 => "Virtual",
        11 => "Yoyos",
        12 => "Bows",
        13 => "Yo-bows",
        16 => "Shields",
        17 => "Helmets",
        18 => "Suits",
        19 => "Gauntlets",
        20 => "Boots",
        21 => "Hats",
        22 => "Masks",
        23 => "Shoes",
        24 => "Cloaks",
        25 => "Refreshers",
        26 => "Potions",
        27 => "Earrings",
        28 => "Necklaces",
        29 => "Rings",
        30 => "Instant food",
        31 => "Cooking",
        34 => "Gems",
        36 => "Collectors",
        37 => "Element cards",
        57 => "Fashion suits",
        58 => "Fashion gloves",
        59 => "Pills",
        60 => "Arrows",
        61 => "Piercing dice",
        62 => "Socket cards (suit)",
        63 => "Socket cards (weapon)",
        70 => "Books",
        71 => "Scrolls",
        72 => "Letters",
        80..=83 => "GM texts",
        100 => "Eat pets",
        101 => "Random scrolls",
        102 => "Ultimate",
        118 => "General",
        119 => "Enchant materials",
        120 => "Pet eggs",
        121 => "Pet food",
        122 => "Tickets",
        123 => "Pockets",
        124..=132 | 142..=146 => "Furniture",
        137 => "Wings",
        138 => "Vis",
        150..=152 => "Smelt materials",
        _ => return format!("Type {v}"),
    }
    .to_owned()
}

fn build_kinds(data: &GameData) -> Vec<KindNode> {
    // ik1 -> ik2 -> ik3 -> count
    let mut tree: BTreeMap<u32, BTreeMap<u32, BTreeMap<u32, u32>>> = BTreeMap::new();
    for (_, p) in data.items.iter() {
        if p.item_kind3 == IK3_VIRTUAL {
            continue;
        }
        *tree
            .entry(p.item_kind1)
            .or_default()
            .entry(p.item_kind2)
            .or_default()
            .entry(p.item_kind3)
            .or_insert(0) += 1;
    }
    tree.into_iter()
        .map(|(k1, m2)| {
            let children: Vec<KindNode> = m2
                .into_iter()
                .map(|(k2, m3)| {
                    let kids: Vec<KindNode> = m3
                        .into_iter()
                        .map(|(k3, n)| KindNode {
                            key: format!("{k1}.{k2}.{k3}"),
                            name: ik3_name(k3),
                            count: n,
                            children: Vec::new(),
                        })
                        .collect();
                    KindNode {
                        key: format!("{k1}.{k2}"),
                        name: ik2_name(k2),
                        count: kids.iter().map(|c| c.count).sum(),
                        children: kids,
                    }
                })
                .collect();
            KindNode {
                key: k1.to_string(),
                name: ik1_name(k1),
                count: children.iter().map(|c| c.count).sum(),
                children,
            }
        })
        .collect()
}

/// The ranges of the builder (also pinned by `tests/golden/admin/item_limits.json` and mirrored by
/// `web/src/admin/grant/ranges.ts`).
pub fn limits(cx: &ValidateCx) -> ItemLimits {
    ItemLimits {
        count_max: 9999,
        upgrade_gear: u8::try_from(cx.tables.max_general()).unwrap_or(10),
        upgrade_accessory: u8::try_from(flyff_rules::item::refine::MAX_ACCESSORY_AO).unwrap_or(20),
        upgrade_collector: u8::try_from(cx.tables.max_collector()).unwrap_or(5),
        upgrade_ultimate: u8::try_from(flyff_rules::item::ultimate::MAX_ULTIMATE_AO).unwrap_or(10),
        element_level: u8::try_from(cx.tables.max_attribute()).unwrap_or(20),
        option_lines: u8::try_from(g::validate::OPTION_LINES_MAX).unwrap_or(3),
        option_dst_max: g::validate::OPTION_DST_MAX,
        option_adj_max: u16::try_from(g::validate::OPTION_ADJ_MAX).unwrap_or(511),
        piercing_suit: u8::try_from(flyff_rules::item::piercing::MAX_PIERCING_SUIT).unwrap_or(4),
        piercing_weapon: u8::try_from(flyff_rules::item::piercing::MAX_PIERCING_WEAPON)
            .unwrap_or(10),
        ultimate_gems: u8::try_from(flyff_game::item::model::MAX_PIERCING_ULTIMATE).unwrap_or(5),
        vis: u8::try_from(flyff_game::item::model::MAX_VIS).unwrap_or(9),
        pet_exp_egg: flyff_rules::item::pet::MAX_PET_EGG_EXP,
        pet_exp: flyff_rules::item::pet::MAX_PET_EXP,
        pet_life: flyff_rules::item::pet::MAX_PET_LIFE,
        pet_energy: (0..=5).map(|l| cx.data.pet.max_energy(l)).collect(),
        text_bytes: u8::try_from(g::validate::TEXT_MAX_BYTES).unwrap_or(31),
        pet_name_bytes: u8::try_from(g::validate::PET_NAME_MAX_BYTES).unwrap_or(16),
        basket_lines: u32::try_from(crate::admin::grants::LINES_MAX).unwrap_or(50),
        recipients: u32::try_from(crate::admin::grants::RECIPIENTS_MAX).unwrap_or(500),
        gold_max: 2_147_483_647,
        mailbox: u32::try_from(flyff_game::econ::mail::MAX_MAIL).unwrap_or(50),
    }
}

/// Builds the metadata (cached per process: the game data does not change while the server runs).
pub fn build(st: &ApiState) -> CatalogMeta {
    let cx = ValidateCx::of(&st.ops);
    let env = cx.env();
    let mut profiles: Vec<ItemFieldsDto> = Vec::new();
    let mut seen: BTreeMap<String, u16> = BTreeMap::new();
    let mut items = Vec::new();
    for (_, p) in cx.data.items.iter() {
        let f = fields_dto(&g::fields_of(&env, p));
        let key = serde_json::to_string(&f).unwrap_or_default();
        let idx = match seen.get(&key) {
            Some(i) => *i,
            None => {
                let i = u16::try_from(profiles.len()).unwrap_or(u16::MAX);
                profiles.push(f);
                seen.insert(key, i);
                i
            }
        };
        items.push((p.id, idx));
    }
    let abilities = cx
        .data
        .ultimate
        .gem_ability
        .iter()
        .flat_map(|def| {
            def.rows.iter().map(|r| AbilityDto {
                id: r.gem06,
                gem: r.gem,
                dst: def.dst,
                values: r.values.clone(),
            })
        })
        .collect();
    let elements = ["Fire", "Water", "Electricity", "Wind", "Earth"]
        .iter()
        .enumerate()
        .map(|(i, n)| NamedId {
            id: u32::try_from(i + 1).unwrap_or(0),
            name: (*n).to_owned(),
        })
        .collect();
    CatalogMeta {
        version: format!("{:032x}", cx.data.meta.fingerprint),
        limits: limits(&cx),
        features: ItemFeatures {
            no_piercing: cx.cfg.no_piercing,
            only_armor_awakes: cx.cfg.only_armor_awakes,
            // WP-ECON-09: `Game::new` installs `econ::grants::deliver` (the poll and the enter-world pass)
            pending_delivery: true,
            inventory_168: st.ops.current.lock().rules().features.inventory_168,
        },
        kinds: build_kinds(&cx.data),
        elements,
        abilities,
        profiles,
        items,
    }
}

/// `GET /admin/api/items/catalog-meta`.
pub async fn catalog_meta(
    State(st): State<ApiState>,
    Who(_who): Who,
) -> ApiResult<Json<CatalogMeta>> {
    let meta: &Arc<CatalogMeta> = st.admin.items.meta.get_or_init(|| Arc::new(build(&st)));
    Ok(Json(CatalogMeta::clone(meta)))
}

/// `POST /admin/api/items/validate`: the body is one `ItemSpec`; the answer is always 200 with the problems inside.
pub async fn validate(
    State(st): State<ApiState>,
    Who(_who): Who,
    body: Bytes,
) -> ApiResult<Json<crate::admin::dto_items::ValidatedItem>> {
    let spec: crate::admin::dto::ItemSpec = parse_json(&body)?;
    Ok(Json(ValidateCx::of(&st.ops).validate(&spec)))
}
