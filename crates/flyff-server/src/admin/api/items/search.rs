//! The ownership search and the duplicate report (Plan/25 §5.5, §12 rule 13; WP-ADMIN-20).
//!
//! `POST /admin/api/items/search` answers an NDJSON stream (`application/x-ndjson`, one [`SearchEvent`] per line):
//! `start` (the live half is answered), the live hits, then the database scan with `hit` and `progress` lines, and
//! `done` (or `error`). **Live hits override the database copies** of online accounts: the game thread's answer
//! lists every item of every online character (and the ground), and the rows of the same accounts that the scan
//! finds in inventories, pockets and banks are skipped (an online character's saved copy can be 90 s old); mails and
//! pending grants are not in memory and stay. One streaming query per identity at a time (`429`); one heavy scan
//! server-wide (`429` while another runs). Dropping the response (the client aborts) cancels the scan at its next
//! chunk boundary.
//!
//! `POST /admin/api/items/duplicates` runs the same check as `flyff db check --serials` and lists every duplicated
//! serial with its places.

use super::{ApiState, game_query, parse_json};
use crate::admin::auth::Who;
use crate::admin::dto_items::{
    DuplicateDto, DuplicatePlaceDto, DuplicatesDto, GroundDto, ItemHitDto, ItemSearchBody,
    SearchEvent,
};
use crate::admin::error::{ApiError, ApiResult};
use crate::admin::grants::stored_dto;
use crate::admin::guard::StreamGuard;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::Response;
use flyff_common::AccountId;
use flyff_db::DbErrorKind;
use flyff_db::ScanEvent;
use flyff_db::admin::items::{ItemHit, ItemSearchQuery, ItemSite};
use flyff_db::admin::scan::ScanStream;
use flyff_game::admin::queries::items::{ItemQuery, LiveItemHit, LiveSite};
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use std::collections::{BTreeSet, VecDeque};
use std::convert::Infallible;

fn site_parts(site: ItemSite) -> (&'static str, Option<u8>) {
    match site {
        ItemSite::Equipment => ("equipment", None),
        ItemSite::Inventory => ("inventory", None),
        ItemSite::Pocket(n) => ("pocket", Some(n)),
        ItemSite::Bank => ("bank", None),
        ItemSite::GuildBank => ("guild_bank", None),
        ItemSite::Mail => ("mail", None),
        ItemSite::PendingGrant => ("pending_grant", None),
    }
}

fn live_site_parts(site: LiveSite) -> (&'static str, Option<u8>) {
    match site {
        LiveSite::Equipment => ("equipment", None),
        LiveSite::Inventory => ("inventory", None),
        LiveSite::Pocket(n) => ("pocket", Some(n)),
        LiveSite::Bank => ("bank", None),
        LiveSite::Ground => ("ground", None),
    }
}

/// A database hit as the wire shows it.
pub fn db_hit(h: &ItemHit) -> ItemHitDto {
    let (site, pocket) = site_parts(h.site);
    ItemHitDto {
        source: "db".to_owned(),
        site: site.to_owned(),
        pocket,
        character_id: h.owner.character_id.map(|c| c.0),
        character_name: h.owner.character_name.clone(),
        account_name: h.owner.account_name.clone(),
        guild_id: h.owner.guild_id.map(|g| g.0),
        guild_name: h.owner.guild_name.clone(),
        mail_id: h.owner.mail_id,
        grant_id: h.owner.grant_id,
        slot: h.slot.map(u32::from),
        bank_slot: h.owner.bank_slot,
        ground: None,
        saved_at: h.owner.saved_at.map(|t| t.0),
        item: stored_dto(&h.item),
    }
}

/// A live hit as the wire shows it.
pub fn live_hit(h: &LiveItemHit) -> ItemHitDto {
    let (site, pocket) = live_site_parts(h.site);
    ItemHitDto {
        source: "live".to_owned(),
        site: site.to_owned(),
        pocket,
        character_id: h.character_id.map(|c| c.0),
        character_name: h.character_name.clone(),
        account_name: None,
        guild_id: None,
        guild_name: None,
        mail_id: None,
        grant_id: None,
        slot: h.slot.map(u32::from),
        bank_slot: h.bank_slot,
        ground: h.ground.map(|g| GroundDto {
            obj_id: g.obj_id,
            world: g.world.0,
            layer: g.layer.0,
            x: g.pos[0],
            y: g.pos[1],
            z: g.pos[2],
        }),
        saved_at: None,
        item: stored_dto(&h.item),
    }
}

fn line(ev: &SearchEvent) -> String {
    let mut s = serde_json::to_string(ev).unwrap_or_else(|_| "{\"type\":\"error\"}".to_owned());
    s.push('\n');
    s
}

/// The state of the response stream.
struct Run {
    lines: VecDeque<String>,
    scan: Option<ScanStream<ItemHit>>,
    _guard: StreamGuard,
    skip: BTreeSet<AccountId>,
    hits: u32,
    cap: u32,
    scanned: u64,
    finished: bool,
}

impl Run {
    fn push(&mut self, ev: &SearchEvent) {
        self.lines.push_back(line(ev));
    }

    /// Reads the next scan event into lines; `false` when nothing follows.
    async fn pump(&mut self) -> bool {
        let Some(scan) = self.scan.as_mut() else {
            return false;
        };
        match scan.recv().await {
            Some(ScanEvent::Rows {
                items,
                scanned,
                total,
            }) => {
                self.scanned = scanned;
                for h in &items {
                    let stale = matches!(
                        h.site,
                        ItemSite::Equipment
                            | ItemSite::Inventory
                            | ItemSite::Pocket(_)
                            | ItemSite::Bank
                    ) && h.owner.account_id.is_some_and(|a| self.skip.contains(&a));
                    if stale {
                        continue;
                    }
                    if self.hits >= self.cap {
                        break;
                    }
                    self.hits += 1;
                    self.push(&SearchEvent::Hit(Box::new(db_hit(h))));
                }
                self.push(&SearchEvent::Progress { scanned, total });
                true
            }
            Some(ScanEvent::Done { scanned }) => {
                self.scanned = scanned;
                self.push(&SearchEvent::Done {
                    scanned,
                    hits: self.hits,
                    truncated: self.hits >= self.cap,
                });
                self.finished = true;
                true
            }
            Some(ScanEvent::Failed(e)) => {
                tracing::error!("item search failed: {e}");
                self.push(&SearchEvent::Error {
                    code: "unavailable".to_owned(),
                    message: "the search failed; try again".to_owned(),
                });
                self.finished = true;
                true
            }
            None => {
                self.finished = true;
                false
            }
        }
    }
}

/// `POST /admin/api/items/search`.
pub async fn search(State(st): State<ApiState>, Who(who): Who, body: Bytes) -> ApiResult<Response> {
    let b: ItemSearchBody = parse_json(&body)?;
    let query = ItemSearchQuery {
        item_id: b.item_id,
        serial: b.serial,
        min_upgrade: b.min_upgrade,
        element: b.element,
        max_hits: usize::try_from(b.max_hits.unwrap_or(0)).unwrap_or(0),
    };
    query
        .validate()
        .map_err(|e| ApiError::invalid("item_id", e.to_string()))?;
    let guard = st
        .admin
        .streams
        .acquire(&who.limit_key())
        .ok_or_else(|| ApiError::rate_limited(2, "another search of yours is still running"))?;
    // the live half first (it is cheap and current), then the scan
    let live = match game_query(
        &st.ops,
        AdminQuery::FindItems(ItemQuery {
            item_id: query.item_id,
            serial: query.serial,
            min_upgrade: query.min_upgrade,
            element: query.element,
        }),
    )
    .await?
    {
        AdminQueryResult::Items(r) => *r,
        AdminQueryResult::Failed(f) => return Err(ApiError::from(&f)),
        other => return Err(ApiError::internal(&format!("FindItems answered {other:?}"))),
    };
    let scan = match st.ops.persist.admin_item_search(query.clone()) {
        Ok(s) => s,
        Err(e) if e.kind == DbErrorKind::Busy => {
            return Err(ApiError::rate_limited(
                5,
                "another heavy read (item search, duplicates, economy) is running; try again in a moment",
            ));
        }
        Err(e) => return Err(ApiError::db(&e)),
    };
    let cap = u32::try_from(query.cap()).unwrap_or(u32::MAX);
    let mut run = Run {
        lines: VecDeque::new(),
        scan: Some(scan),
        _guard: guard,
        skip: live.online_accounts.iter().copied().collect(),
        hits: 0,
        cap,
        scanned: 0,
        finished: false,
    };
    run.push(&SearchEvent::Start {
        live: u32::try_from(live.hits.len()).unwrap_or(u32::MAX),
        skipped_accounts: u32::try_from(live.online_accounts.len()).unwrap_or(u32::MAX),
    });
    for h in &live.hits {
        if run.hits >= cap {
            break;
        }
        run.hits += 1;
        run.push(&SearchEvent::Hit(Box::new(live_hit(h))));
    }
    let stream = futures_util::stream::unfold(run, |mut run| async move {
        loop {
            if let Some(l) = run.lines.pop_front() {
                return Some((Ok::<Bytes, Infallible>(Bytes::from(l)), run));
            }
            if run.finished || !run.pump().await {
                if let Some(l) = run.lines.pop_front() {
                    return Some((Ok(Bytes::from(l)), run));
                }
                return None;
            }
        }
    });
    let mut resp = Response::new(Body::from_stream(stream));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp.headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(resp)
}

/// `POST /admin/api/items/duplicates`.
pub async fn duplicates(
    State(st): State<ApiState>,
    Who(who): Who,
) -> ApiResult<axum::Json<DuplicatesDto>> {
    let _guard = st
        .admin
        .streams
        .acquire(&who.limit_key())
        .ok_or_else(|| ApiError::rate_limited(2, "another search of yours is still running"))?;
    let mut scan = match st
        .ops
        .persist
        .admin_scan(flyff_db::admin::items::DuplicateSerialsScan::new())
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
    let mut found = Vec::new();
    let mut scanned = 0_u64;
    while let Some(ev) = scan.recv().await {
        match ev {
            ScanEvent::Rows {
                items, scanned: s, ..
            } => {
                scanned = s;
                found.extend(items);
            }
            ScanEvent::Done { scanned: s } => scanned = s,
            ScanEvent::Failed(e) => return Err(ApiError::db(&e)),
        }
    }
    let truncated = found.len() >= flyff_db::admin::items::DUPLICATES_MAX;
    Ok(axum::Json(DuplicatesDto {
        duplicates: found
            .into_iter()
            .map(|d| DuplicateDto {
                serial: d.serial,
                places: d
                    .places
                    .into_iter()
                    .map(|p| {
                        let (site, pocket) = site_parts(p.site);
                        DuplicatePlaceDto {
                            site: site.to_owned(),
                            pocket,
                            character_id: p.owner.character_id.map(|c| c.0),
                            character_name: p.owner.character_name,
                            account_name: p.owner.account_name,
                            guild_id: p.owner.guild_id.map(|g| g.0),
                            mail_id: p.owner.mail_id,
                            grant_id: p.owner.grant_id,
                            slot: p.slot.map(u32::from),
                            item_id: p.item_id,
                            count: u32::from(p.count),
                        }
                    })
                    .collect(),
            })
            .collect(),
        scanned,
        truncated,
    }))
}
