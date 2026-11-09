//! The guild cloak (Plan/21 §5.8; ref/07a §15.6; `CDPSrvr::OnCreateGuildCloak`, `DPSrvr.cpp:1291-1387`).
//!
//! `CreateGuildCloak` turns [`CLOAK_COST`] penya of the guild pool into a cloak item in the guild bank. The checks, in
//! this order: the guild has a logo (`TID_GAME_GUILDSETTINGLOGO`), the actor is the master
//! (`TID_GAME_GUILDONLYMASTERLOGO`), the bank has a free slot (`TID_GAME_GUILDBANKFULL`), the pool holds the penya
//! (`TID_GAME_GUILDNEEDGOLD`). The item is `II_ARM_S_CLO_CLO_SYSCLOAK01 + logo - 1` (logo 999: the blank cloak; a
//! game master with a logo above 20: `SYSCLOAK01`), count 1, stamped with the guild id, a fresh serial, the
//! endurance of its prop. FIX: only `SYSCLOAK01..20` exist, so a master of a guild whose game-master-set logo is
//! 21..27 gets nothing (the original built an item that does not exist).
//!
//! Commit: the pool loses 10 000 (the master's contribution is NOT touched), the bank takes the cloak, ONE
//! `GuildCloak` batch (no snapshot: the cloak is born in the bank) with the log row `C`. Messages: the master gets
//! `PutItemGuildBankNtf{mode 1}`, the window-open members inside his observer set `{mode 3}`, EVERY online member
//! including the master `penya_all{10 000, master, cloak: true}` (the clients lower the guild pool, not the
//! contribution) and the master `TID_GAME_GUILDCREATECLOAK`.

use super::{CLOAK_COST, emit_bank, member, remember, tell_viewers};
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::item::container::SerialSource;
use crate::item::model::Item;
use crate::item::ops::CtxSerials;
use crate::social::guild::commit::{self, Origin};
use crate::social::guild::wire;
use flyff_common::defs::item::{II_ARM_S_CLO_CLO_BLANK, II_ARM_S_CLO_CLO_SYSCLOAK01};
use flyff_common::defs::text::{
    TID_GAME_GUILDBANKFULL, TID_GAME_GUILDCREATECLOAK, TID_GAME_GUILDNEEDGOLD,
    TID_GAME_GUILDONLYMASTERLOGO, TID_GAME_GUILDSETTINGLOGO,
};
use flyff_common::{GuildId, ItemSerial};
use flyff_db::{AtomicBatch, GuildBankLogKind, GuildBankLogRecord};
use flyff_proto::pb;
use flyff_proto::pb::get_item_guild_bank_ntf as get;
use flyff_rules::social::GUILD_LOGO_MAX;

/// The logo that asks for the blank (custom) cloak.
pub const LOGO_CUSTOM: u32 = 999;

/// The logos above this are game-master logos (`CUSTOM_LOGO_MAX - 7`).
const LOGO_GM_ABOVE: u32 = 20;

/// The first game-master logo.
const LOGO_GM_FIRST: u32 = LOGO_GM_ABOVE + 1;

/// The cloak item id of a logo: 999 the blank cloak, 1..=20 `SYSCLOAK01 + logo - 1`, a game master with 21..=27
/// `SYSCLOAK01`; `None` for logo 0 and for a non-GM logo 21..=27 (no such item).
pub fn cloak_item_id(logo: u32, as_gm: bool) -> Option<u32> {
    match logo {
        0 => None,
        LOGO_CUSTOM => Some(II_ARM_S_CLO_CLO_BLANK),
        LOGO_GM_FIRST..=GUILD_LOGO_MAX if as_gm => Some(II_ARM_S_CLO_CLO_SYSCLOAK01),
        l @ 1..=LOGO_GM_ABOVE => Some(II_ARM_S_CLO_CLO_SYSCLOAK01 + (l - 1)),
        _ => None,
    }
}

/// The cloak of `guild` for `logo` (see [`cloak_item_id`]): count 1, `guild_id` stamped, the endurance of its prop,
/// a fresh serial. `None` when the logo has no cloak, the prop does not exist or no serial can be allocated. The
/// admin console puts the optional cloak of a new guild into `AdminGuildSpec.bank` with it.
pub(crate) fn cloak_item(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    guild: GuildId,
    logo: u32,
    as_gm: bool,
) -> Option<Item> {
    let id = cloak_item_id(logo, as_gm)?;
    let prop = ctx.data.item(id)?;
    let mut item = Item::new(id, 1);
    item.guild_id = guild.0;
    item.endurance = i32::try_from(prop.endurance).unwrap_or(i32::MAX);
    let Services { serials, .. } = &mut *svc;
    let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
    match src.next_serial() {
        Ok(s) => item.serial = ItemSerial(s),
        Err(e) => {
            tracing::warn!(error = %e, "a guild cloak got no serial");
            return None;
        }
    }
    Some(item)
}

/// `CreateGuildCloak` (see the module documentation).
pub fn on_create_guild_cloak(
    svc: &mut Services,
    ctx: &mut GameCtx<'_>,
    who: PlayerRef,
    _m: pb::CreateGuildCloak,
) {
    let Some(me) = member(svc, ctx, who) else {
        return;
    };
    let Some(g) = svc.social.guild.get(me.guild) else {
        return;
    };
    let (logo, is_master, bank_full, gold) = (
        g.logo,
        g.master() == Some(me.c),
        g.bank.count() >= usize::from(super::BANK_SLOTS),
        g.gold,
    );
    if logo == 0 {
        notice::defined(ctx, who, TID_GAME_GUILDSETTINGLOGO, Vec::new());
        return;
    }
    if !is_master {
        notice::defined(ctx, who, TID_GAME_GUILDONLYMASTERLOGO, Vec::new());
        return;
    }
    if bank_full {
        notice::defined(ctx, who, TID_GAME_GUILDBANKFULL, Vec::new());
        return;
    }
    if gold < CLOAK_COST {
        notice::defined(ctx, who, TID_GAME_GUILDNEEDGOLD, Vec::new());
        return;
    }
    let as_gm = ctx
        .player(who)
        .and_then(|mv| mv.player())
        .is_some_and(|p| p.ident.authority.is_gm());
    let Some(item) = cloak_item(svc, ctx, me.guild, logo, as_gm) else {
        return;
    };
    let Some(mut bank) = svc.social.guild.get(me.guild).map(|g| g.bank.clone()) else {
        return;
    };
    let plan = {
        let Services { serials, .. } = &mut *svc;
        let mut src = CtxSerials::new(serials, ctx.persist, ctx.tickets);
        match bank.items.add(&item, ctx.data, &mut src) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::warn!(error = %e, "a guild cloak was refused by the bank container");
                return;
            }
        }
    };
    // ---- the commit block ----
    let first = plan.touched.first().map_or(0, |t| t.0);
    let item_pb = item.to_pb(first);
    if let Some(g) = svc.social.guild.get_mut(me.guild) {
        g.gold -= CLOAK_COST;
        g.bank = bank;
    }
    let log = GuildBankLogRecord::new(me.guild, me.c, GuildBankLogKind::Cloak, commit::now(ctx))
        .with_item(item.item_id, 0, 1);
    remember(svc, me.guild, &log);
    if let Some(g) = svc.social.guild.get(me.guild) {
        let batch = AtomicBatch::guild_cloak(g.to_record(), g.bank.to_doc(), log);
        commit::submit(svc, ctx, "GuildCloak", batch, &[], None);
    }
    wire::to_one(
        ctx,
        who.0,
        pb::PutItemGuildBankNtf {
            mode: 1,
            item: Some(item_pb.clone()),
        },
    );
    tell_viewers(
        svc,
        ctx,
        who,
        me.guild,
        pb::PutItemGuildBankNtf {
            mode: 3,
            item: Some(item_pb),
        },
    );
    wire::to_members(
        svc,
        ctx,
        me.guild,
        pb::GetItemGuildBankNtf {
            sub: Some(get::Sub::PenyaAll(get::PenyaAllUpdate {
                gold: CLOAK_COST,
                id_player: me.c.0,
                cloak: true,
            })),
        },
    );
    notice::defined(ctx, who, TID_GAME_GUILDCREATECLOAK, Vec::new());
    emit_bank(
        ctx,
        Origin::Player(who),
        me.guild,
        me.c,
        GuildBankLogKind::Cloak,
        Some(&item),
        i64::from(CLOAK_COST),
    );
}
