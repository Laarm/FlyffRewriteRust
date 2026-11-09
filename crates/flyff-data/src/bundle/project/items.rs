//! `items`, `item-ext`, `skills`, `movers`, `npcs`.

use super::{Sources, canon};
use crate::bundle::BundleError;
use crate::bundle::rowtable::RowTable;
use crate::bundle::types::{
    CLIENT_ITEM_OMIT, CLIENT_MOVER_OMIT, EnchantDef, ItemExtBundle, ItemsBundle, MoversBundle,
    NpcRow, NpcStock, NpcTab, NpcsBundle, PetAvail, PetTablesDef, SkillsBundle, TicketRow,
};
use crate::character::MAX_VENDOR_INVENTORY_TAB;
use crate::props::item::ItemProp;

/// The name the client shows: the **ultimate weapon** rule of 14 §10.6 (`CProject::LoadPropItem`, client only,
/// src:_Common/ProjectCmn.cpp:534-546): if the item is an ultimate weapon and its name contains `@` at byte
/// position `p` (first occurrence), the 2 bytes at `p - 1 ..= p` are deleted (the character before `@` and the
/// `@`; MFC clamps a start of -1 to 0). Other names are returned unchanged.
pub fn client_item_name(prop: &ItemProp) -> String {
    if !prop.is_ultimate() {
        return prop.name.clone();
    }
    let Some(p) = prop.name.find('@') else {
        return prop.name.clone();
    };
    let mut start = p.saturating_sub(1);
    while start > 0 && !prop.name.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + 2).min(prop.name.len());
    while end < prop.name.len() && !prop.name.is_char_boundary(end) {
        end += 1;
    }
    format!(
        "{}{}",
        prop.name.get(..start).unwrap_or(""),
        prop.name.get(end..).unwrap_or("")
    )
}

fn client_items<'a>(rows: impl Iterator<Item = (u32, &'a ItemProp)>) -> RowTable<ItemProp> {
    let v: Vec<ItemProp> = rows
        .map(|(_, p)| {
            let mut c = p.clone();
            c.name = client_item_name(p);
            c
        })
        .collect();
    RowTable::without(v, &CLIENT_ITEM_OMIT)
}

/// `items`: every `propItem` row in ascending id order.
pub fn project_items(src: &Sources<'_>) -> Result<ItemsBundle, BundleError> {
    Ok(ItemsBundle {
        items: client_items(src.game.items.iter()),
    })
}

/// `skills`: skills, party skills, add-skill rows and the per-job skill lists.
pub fn project_skills(src: &Sources<'_>) -> Result<SkillsBundle, BundleError> {
    let g = src.game;
    Ok(SkillsBundle {
        skills: client_items(g.skills.iter()),
        party_skills: client_items(g.party_skills.iter()),
        levels: RowTable::new(g.add_skills.iter().map(|(_, r)| r.clone()).collect()),
        job_skills: g.job_skills.by_job.clone(),
    })
}

/// `movers`: `propMover` without the AI/drop block and `propCtrl`.
pub fn project_movers(src: &Sources<'_>) -> Result<MoversBundle, BundleError> {
    let g = src.game;
    let movers: Vec<_> = g
        .movers
        .iter()
        .map(|(_, m)| {
            let mut c = m.clone();
            c.ex = crate::mover_ex::MoverEx::default();
            c
        })
        .collect();
    Ok(MoversBundle {
        movers: RowTable::without(movers, &CLIENT_MOVER_OMIT),
        ctrls: RowTable::new(g.ctrls.iter().map(|(_, c)| c.clone()).collect()),
    })
}

/// `item-ext`: the item extension tables the client shows.
pub fn project_item_ext(src: &Sources<'_>) -> Result<ItemExtBundle, BundleError> {
    let g = src.game;
    let e = &g.enchant;
    Ok(ItemExtBundle {
        set_items: g.item_etc.set_items.clone(),
        random_opt_items: g.item_etc.random_opt_items.clone(),
        piercing: g.item_etc.piercing.clone(),
        accessory: g.accessory.items.clone(),
        gem_ability: g.ultimate.gem_ability.clone(),
        random_options: g.random_option.groups.clone(),
        pet: PetTablesDef {
            avail: g
                .pet
                .avail
                .iter()
                .map(|a| PetAvail {
                    dst: a.dst,
                    values: a.values.clone(),
                    item: a.item,
                    movers: a.movers.clone(),
                })
                .collect(),
            max_energy: g
                .pet
                .max_energy
                .iter()
                .map(|v| i32::try_from(*v).unwrap_or(i32::MAX))
                .collect(),
            feed_energy: g.pet.feed_energy.to_vec(),
        },
        exchange: g.exchange.menus.clone(),
        enchant: EnchantDef {
            limit_level: e.limit_level.to_vec(),
            level_scale: e.level_scale.iter().map(|r| r.to_vec()).collect(),
        },
        // ---- tickets (task ui-minigames, WP-CFEAT-45): the `BTreeMap` is ascending by item id
        tickets: g
            .tickets
            .tickets
            .iter()
            .map(|(item, t)| TicketRow {
                item: *item,
                world: t.world,
                expand: g.tickets.expanded_layer(t.world),
            })
            .collect(),
    })
}

/// `npcs`: all NPC definitions in file order with exactly four vendor tabs each.
pub fn project_npcs(src: &Sources<'_>) -> Result<NpcsBundle, BundleError> {
    let npcs = src
        .game
        .npcs
        .iter()
        .map(|n| {
            let mut tabs: Vec<NpcTab> = n
                .vendor
                .tabs
                .iter()
                .map(|t| NpcTab {
                    title: t.title.clone(),
                    stock: t
                        .stock
                        .iter()
                        .map(|s| NpcStock {
                            item: s.item,
                            count: s.count,
                        })
                        .collect(),
                })
                .collect();
            tabs.resize_with(MAX_VENDOR_INVENTORY_TAB, NpcTab::default);
            NpcRow {
                key: n.key.clone(),
                name: n.name.clone(),
                portrait: if n.portrait.is_empty() {
                    String::new()
                } else {
                    canon(&format!("Char/{}", n.portrait))
                },
                dialog: n.dialog_file.is_some(),
                dialog_key: n.dialog_key.clone(),
                structure: n.structure,
                music: n.music,
                figure: n.figure,
                equip: n.equip.clone(),
                menus: n.menus.clone(),
                vendor_type: n.vendor.vendor_type,
                tabs,
                buff_skills: n.buff_skills.clone(),
                teleports: n.teleports.clone(),
            }
        })
        .collect();
    Ok(NpcsBundle { npcs })
}
