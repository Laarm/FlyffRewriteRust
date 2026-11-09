//! Glue of the NPC loader: `character.inc`, `character-etc.inc`, `character-school.inc` (L20) and the
//! vendor stock (F7; Plan/14 §6.8, §5.4).

use crate::character::parse_character_file;
use crate::load::ctx::{LoadCtx, Need};
use crate::vendor::generate_vendor_stock;
use flyff_script::{DiagCode, Severity};
use std::sync::Arc;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `CProject::LoadCharacter(path)`: one character file (called once per `character` entry of the project
/// file, in order). `AddShopItem` prices are written into `gd.items` here (F6). The vendor stock is **not**
/// built yet: [`finalize_vendor_stock`] runs after the last file, because the original builds it lazily at
/// run time and so sees every price write of every file.
pub fn load_character(cx: &mut LoadCtx<'_>, path: &str) {
    let Some(text) = cx.read_text(path, Need::Warn) else {
        return;
    };
    let sctx = cx.script_ctx();
    let file = cx.file_arc(path);
    let source = u8::try_from(cx.gd.npcs.sources.len()).unwrap_or(u8::MAX);
    cx.gd.npcs.sources.push(file.to_string());
    let add_shop_item = cx.opts.add_shop_item;
    let npcs = parse_character_file(
        &text,
        &file,
        &sctx,
        source,
        add_shop_item,
        &mut cx.gd.items,
        &mut cx.diags,
    );
    if let Some(slot) = cx.gd.meta.counts.npc_rows.get_mut(usize::from(source)) {
        *slot = slot.saturating_add(count(npcs.len()));
    }
    for npc in npcs {
        let key = npc.key.to_ascii_lowercase();
        let shown = npc.key.clone();
        if cx.gd.npcs.by_key.insert(key, npc).is_some() {
            cx.report(
                Severity::Warning,
                DiagCode::PropIdDuplicate,
                Some(path),
                0,
                format!("NPC key {shown} is defined more than once; the last definition wins"),
            );
        }
    }
    cx.gd.meta.counts.npc_unique = count(cx.gd.npcs.len());
}

/// F7: precomputes the stock of every vendor NPC once all character files are loaded.
pub fn finalize_vendor_stock(cx: &mut LoadCtx<'_>) {
    let LoadCtx { gd, diags, .. } = cx;
    let files: Vec<Arc<str>> = gd
        .npcs
        .sources
        .iter()
        .map(|s| Arc::from(s.as_str()))
        .collect();
    let mut vendors = 0usize;
    for npc in gd.npcs.by_key.values_mut() {
        let file = files.get(usize::from(npc.source));
        let stock = generate_vendor_stock(
            &npc.key,
            &npc.vendor,
            &gd.items,
            &gd.item_index,
            file,
            diags,
        );
        for (tab, s) in npc.vendor.tabs.iter_mut().zip(stock) {
            tab.stock = s;
        }
        if npc.is_vendor() {
            vendors += 1;
        }
    }
    gd.meta.counts.npc_vendors = count(vendors);
}
