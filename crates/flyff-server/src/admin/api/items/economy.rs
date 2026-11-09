//! `GET /admin/api/economy?item=<id>&refresh=1` (Plan/25 §5.7; WP-ADMIN-22): penya in circulation by holder, the 50
//! richest characters and accounts, the pieces of one item (default: the Perin) by site and the grants of the last
//! 30 days. It is a heavy scan (one at a time server-wide), computed on demand and cached for ten minutes per
//! counted item; the answer carries its "as of" time and age. `refresh=1` recomputes.

use super::{ApiState, ECONOMY_CACHE};
use crate::admin::auth::Who;
use crate::admin::dto_items::{
    CensusDto, EconomyDto, GrantDayDto, PenyaDto, RichAccountDto, RichCharacterDto,
};
use crate::admin::error::{ApiError, ApiResult};
use axum::Json;
use axum::extract::{Query, State};
use flyff_common::defs::item::II_SYS_SYS_SCR_PERIN;
use flyff_db::DbErrorKind;
use flyff_db::admin::items::{EconomyQuery, EconomySummary};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

/// The query string.
#[derive(Debug, Default, Deserialize)]
pub struct EconomyParams {
    item: Option<u32>,
    refresh: Option<String>,
}

fn dto_of(s: EconomySummary, name: String, now: i64) -> EconomyDto {
    EconomyDto {
        as_of: s.as_of.0,
        age_s: now.saturating_sub(s.as_of.0).max(0),
        cached: false,
        characters: s.characters,
        accounts: s.accounts,
        penya: PenyaDto {
            inventories: s.penya.inventories,
            banks: s.penya.banks,
            guild_banks: s.penya.guild_banks,
            mails: s.penya.mails,
            vendors: s.penya.vendors,
            total: s.penya.total,
            pending_grants: s.penya.pending_grants,
        },
        richest_characters: s
            .richest_characters
            .into_iter()
            .map(|c| RichCharacterDto {
                character_id: c.character_id.0,
                name: c.name,
                account: c.account_name,
                gold: c.gold,
                bank_gold: c.bank_gold,
            })
            .collect(),
        richest_accounts: s
            .richest_accounts
            .into_iter()
            .map(|a| RichAccountDto {
                account_id: a.account_id.0,
                name: a.name,
                wallets: a.wallets,
                banks: a.banks,
                total: a.total,
            })
            .collect(),
        census: CensusDto {
            item_id: s.census.item_id,
            name,
            pieces: s.census.pieces,
            stacks: s.census.stacks,
            by_site: s.census.by_site,
        },
        grants_per_day: s
            .grants_per_day
            .into_iter()
            .map(|d| GrantDayDto {
                day: d.day,
                grants: d.grants,
                gold_grants: d.gold_grants,
                gold: d.gold,
            })
            .collect(),
    }
}

/// `GET /admin/api/economy`.
pub async fn economy(
    State(st): State<ApiState>,
    Who(_who): Who,
    Query(p): Query<EconomyParams>,
) -> ApiResult<Json<EconomyDto>> {
    let item = p.item.unwrap_or(II_SYS_SYS_SCR_PERIN);
    let now = st.ops.admin.clock.unix();
    let refresh = p
        .refresh
        .as_deref()
        .is_some_and(|r| r == "1" || r == "true");
    if !refresh
        && let Some((at, _, dto)) = st.admin.items.economy.lock().get(&item)
        && at.elapsed() < ECONOMY_CACHE
    {
        let mut d = (**dto).clone();
        d.cached = true;
        d.age_s = now.0.saturating_sub(d.as_of).max(0);
        return Ok(Json(d));
    }
    let name = st
        .ops
        .http
        .assets
        .data
        .item(item)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let summary = match st
        .ops
        .persist
        .admin_economy_summary(
            EconomyQuery {
                item_id: Some(item),
            },
            now,
        )
        .await
    {
        Ok(s) => s,
        Err(e) if e.kind == DbErrorKind::Busy => {
            return Err(ApiError::rate_limited(
                5,
                "another heavy read (item search, duplicates, economy) is running; try again in a moment",
            ));
        }
        Err(e) => return Err(ApiError::db(&e)),
    };
    let dto = dto_of(summary, name, now.0);
    st.admin
        .items
        .economy
        .lock()
        .insert(item, (Instant::now(), now, Arc::new(dto.clone())));
    Ok(Json(dto))
}
