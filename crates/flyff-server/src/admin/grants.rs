//! Grants of the console (Plan/25 §5.3, §5.6, §11.10; **WP-ADMIN-20**): the validation context, the conversions
//! between the wire DTOs and the game's types, the grant pipeline of `POST /admin/api/grants/v2`, and
//! [`insert`], the one function that writes a `pending_grants` row (shared by `POST /admin/api/grants`, the console
//! word `grant` and grants v2 in Pending mode).
//!
//! **The pipeline** ([`run`]):
//! 1. validate every item line with the game's own rules (`flyff_game::admin::actions::grants::validate`; the game
//!    thread validates again);
//! 2. resolve the recipients (characters, everybody online, a world) and apply rank protection per recipient;
//! 3. send `AdminCmd::GrantBatch` in chunks of at most 100 recipients (`request_id` = the idempotency key plus the
//!    chunk number, so a retry is recognised by the game for ten minutes);
//! 4. insert `pending_grants` rows for the recipients the game handed back (`NeedsPending`);
//! 5. answer the per-recipient [`GrantReport`] and audit `admin.grant` with it.

use super::api::ApiState;
use super::dto::{
    Element, ErrorCode, GrantOutcome, GrantReport, GrantRequest, ItemSpec, KeepUntil, PetRecord,
    Recipients,
};
use super::dto_items::{ItemFieldsDto, ItemIssue, StoredItemDto, ValidatedItem};
use super::error::{ApiError, ApiResult};
use super::guard::{ConfirmFacts, ReasonText, audit_admin, rank};
use super::identity::Identity;
use super::ops::AdminOps;
use super::players as pl;
use flyff_common::gm::Authority;
use flyff_common::{CharacterId, UnixTime};
use flyff_data::GameData;
use flyff_db::admin::items::RecipientFact;
use flyff_db::{
    AtomicBatch, AtomicKind, AtomicOutcome, CharacterState, DbOp, ItemRecord, PendingGrantRecord,
    PersistHandle,
};
use flyff_game::admin::actions::grants as g;
use flyff_game::admin::{AdminQuery, AdminQueryResult};
use flyff_game::input::{AdminActor, AdminCmd, AdminOutput};
use flyff_game::item::env::ItemConfig;
use flyff_rules::item::refine::UpgradeTables;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;

/// The most recipients of one grant (25 §5.3).
pub const RECIPIENTS_MAX: usize = 500;
/// The most item lines of one grant (25 §5.3).
pub const LINES_MAX: usize = g::MAX_LINES;

// ---- the validation context ---------------------------------------------------------------------------------------------

/// Everything validation reads: the game data, the item switches and the upgrade limits.
pub struct ValidateCx {
    /// The loaded game data.
    pub data: Arc<GameData>,
    /// `features.no_piercing`, `features.only_armor_awakes`, ...
    pub cfg: ItemConfig,
    /// `ItemUpgrade.lua`, `accessory.inc`, `collecting`.
    pub tables: UpgradeTables,
    /// The wall clock.
    pub now: UnixTime,
}

impl ValidateCx {
    /// The context of a running server.
    pub fn of(ops: &AdminOps) -> ValidateCx {
        let data = Arc::clone(&ops.http.assets.data);
        let cfg = ItemConfig::from_rules(&ops.current.lock().rules());
        let tables = UpgradeTables::from_data(
            data.lua.item_upgrade.as_ref(),
            &data.accessory,
            &data.collecting,
        );
        ValidateCx {
            data,
            cfg,
            tables,
            now: ops.admin.clock.unix(),
        }
    }

    /// The environment of the game's validation.
    pub fn env(&self) -> g::ValidateEnv<'_> {
        g::ValidateEnv {
            data: &self.data,
            cfg: &self.cfg,
            tables: &self.tables,
            now: self.now,
        }
    }

    /// `POST /admin/api/items/validate`: the answer for one spec.
    pub fn validate(&self, spec: &ItemSpec) -> ValidatedItem {
        let name = self
            .data
            .item(spec.item_id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let fields = self
            .data
            .item(spec.item_id)
            .map(|p| fields_dto(&g::fields_of(&self.env(), p)));
        match g::validate(&self.env(), &game_spec(spec)) {
            Ok(n) => ValidatedItem {
                ok: true,
                name,
                item: Some(stored_dto(&n.record)),
                errors: Vec::new(),
                warnings: n
                    .warnings
                    .into_iter()
                    .map(|w| ItemIssue {
                        field: w.field,
                        code: w.code.to_owned(),
                        message: w.message,
                    })
                    .collect(),
                fields,
            },
            Err(errors) => ValidatedItem {
                ok: false,
                name,
                item: None,
                errors: errors
                    .into_iter()
                    .map(|e| ItemIssue {
                        field: e.field,
                        code: e.code.to_owned(),
                        message: e.message,
                    })
                    .collect(),
                warnings: Vec::new(),
                fields,
            },
        }
    }

    /// The stored record of a valid spec.
    pub fn record(&self, spec: &ItemSpec) -> Result<ItemRecord, Vec<ItemIssue>> {
        g::validate(&self.env(), &game_spec(spec))
            .map(|n| n.record)
            .map_err(|errors| {
                errors
                    .into_iter()
                    .map(|e| ItemIssue {
                        field: e.field,
                        code: e.code.to_owned(),
                        message: e.message,
                    })
                    .collect()
            })
    }
}

/// The fields of an item as the wire shows them.
pub fn fields_dto(f: &g::ItemFields) -> ItemFieldsDto {
    ItemFieldsDto {
        stack_max: f.stack_max,
        upgrade_max: f.upgrade_max,
        element: f.element,
        element_level_max: f.element_level_max,
        random_options: f.random_options,
        piercing_max: f.piercing.map_or(0, |p| p.0),
        piercing_card_kind: f.piercing.map_or(0, |p| p.1),
        ultimate: f.ultimate,
        vis: f.vis,
        pet: f.pet,
        charged: f.charged,
        text: f.text,
        guild: f.guild,
    }
}

/// A stored item as the wire shows it (the random-option word as a decimal string).
pub fn stored_dto(r: &ItemRecord) -> StoredItemDto {
    StoredItemDto {
        item_id: r.item_id,
        count: u32::from(r.count),
        serial: r.serial,
        upgrade_level: r.upgrade_level,
        element: u32::from(r.element),
        element_level: r.element_level,
        element_stone_id: r.element_stone_id,
        guild_id: r.guild_id,
        charged: r.charged,
        flags: u32::from(r.flags),
        keep_until: r.keep_until,
        random_option: r.random_option.to_string(),
        text: r.text.clone(),
        endurance: r.endurance,
        piercing_cards: r.piercing_cards.clone(),
        ultimate_gems: r.ultimate_gems.clone(),
        vis_keep_until: r.vis_keep_until.clone(),
        pet: r.pet.as_ref().map(|p| PetRecord {
            kind: p.kind,
            level: p.level,
            exp: p.exp,
            energy: p.energy,
            life: p.life,
            avail_levels: p.avail_levels,
            name: p.name.clone(),
        }),
    }
}

fn game_keep(k: KeepUntil) -> g::KeepUntil {
    match k {
        KeepUntil::Item => g::KeepUntil::Item,
        KeepUntil::None => g::KeepUntil::None,
        KeepUntil::Duration { secs } => g::KeepUntil::Duration { secs },
        KeepUntil::At { unix } => g::KeepUntil::At { unix },
    }
}

/// The game's spec of a wire spec.
pub fn game_spec(s: &ItemSpec) -> g::ItemSpec {
    g::ItemSpec {
        item_id: s.item_id,
        count: s.count,
        upgrade: s.upgrade,
        element: match s.element {
            None => 0,
            Some(Element::Fire) => 1,
            Some(Element::Water) => 2,
            Some(Element::Electricity) => 3,
            Some(Element::Wind) => 4,
            Some(Element::Earth) => 5,
        },
        element_level: s.element_level,
        random_options: s
            .random_options
            .iter()
            .map(|l| g::RandomOptLine {
                dst: l.dst,
                adj: l.adj,
            })
            .collect(),
        safe: s.safe,
        piercing: s.piercing.clone(),
        ultimate_gems: s.ultimate_gems.clone(),
        vis: s
            .vis
            .iter()
            .map(|v| g::VisSpec {
                item_id: v.item_id,
                keep_until: game_keep(v.keep_until),
            })
            .collect(),
        pet: s.pet.as_ref().map(|p| flyff_db::PetRecord {
            kind: p.kind,
            level: p.level,
            exp: p.exp,
            energy: p.energy,
            life: p.life,
            avail_levels: p.avail_levels,
            name: p.name.clone(),
        }),
        charged: s.charged,
        keep_until: game_keep(s.keep_until),
        bound: s.bound,
        text: s.text.clone(),
        guild_id: s.guild_id,
    }
}

// ---- rows ---------------------------------------------------------------------------------------------------------------

/// What [`insert`] writes.
#[derive(Clone, Debug)]
pub struct NewGrant {
    /// The receiver.
    pub character: CharacterId,
    /// `Some`: an item grant (a template; its serial is assigned at delivery). `None`: a gold grant.
    pub item: Option<ItemRecord>,
    /// The penya of a gold grant (`> 0`).
    pub gold: i64,
    /// `admin_api`, `console`, `admin:<actor>`, ...
    pub source: String,
    /// The operator's note.
    pub note: String,
}

/// Inserts one `pending_grants` row and returns its id (Plan/23 §8.3 `admin::grants::insert`, 25 §5.3). The row is
/// delivered at the next enter-world and by the poll once WP-ECON-09 installs the delivery.
pub async fn insert(persist: &PersistHandle, now: UnixTime, grant: NewGrant) -> ApiResult<i64> {
    let rows = insert_many(persist, now, vec![grant]).await?;
    rows.into_iter()
        .next()
        .ok_or_else(|| ApiError::internal("a grant insert answered no id"))
}

/// Inserts several rows of one receiver in one transaction and returns their ids in order (SQLite hands out
/// consecutive rowids inside one transaction of the single writer; the first id is what the batch reports).
pub async fn insert_many(
    persist: &PersistHandle,
    now: UnixTime,
    grants: Vec<NewGrant>,
) -> ApiResult<Vec<i64>> {
    if grants.is_empty() {
        return Ok(Vec::new());
    }
    let count = i64::try_from(grants.len()).unwrap_or(0);
    let mut ops = Vec::with_capacity(grants.len());
    for gr in grants {
        let rec = match gr.item {
            Some(item) => PendingGrantRecord::item(gr.character, item, gr.source, gr.note, now),
            None if gr.gold > 0 => {
                PendingGrantRecord::gold(gr.character, gr.gold, gr.source, gr.note, now)
            }
            None => {
                return Err(ApiError::invalid(
                    "gold",
                    "a gold grant needs a positive amount",
                ));
            }
        };
        ops.push(DbOp::InsertPendingGrant(rec));
    }
    match persist
        .call(AtomicBatch::of_ops(
            AtomicKind::Other("admin_grant_pending"),
            ops,
        ))
        .await
    {
        Ok(AtomicOutcome::RowInserted(first)) => Ok((0..count).map(|i| first + i).collect()),
        Ok(other) => Err(ApiError::internal(&format!(
            "a pending grant insert answered {other:?}"
        ))),
        Err(e) => Err(match e.kind {
            flyff_db::DbErrorKind::NotFound => ApiError::not_found("no such character"),
            _ => ApiError::db(&e),
        }),
    }
}

// ---- the pipeline -------------------------------------------------------------------------------------------------------

/// `uuid`-shaped text of a 128-bit id (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
pub fn uuid_text(v: u128) -> String {
    let b = v.to_be_bytes();
    let mut s = String::with_capacity(36);
    for (i, byte) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// The request id of a grant: derived from the `Idempotency-Key` (so a retry after a timeout is recognised by the
/// game), random without one.
pub fn request_id(key: Option<&str>) -> u128 {
    match key {
        Some(k) => {
            let h = blake3::hash(k.as_bytes());
            let mut b = [0u8; 16];
            b.copy_from_slice(&h.as_bytes()[..16]);
            u128::from_be_bytes(b)
        }
        None => {
            let h = blake3::hash(
                format!("{:?}{}", std::time::SystemTime::now(), std::process::id()).as_bytes(),
            );
            let mut b = [0u8; 16];
            b.copy_from_slice(&h.as_bytes()[..16]);
            u128::from_be_bytes(b)
        }
    }
}

struct Target {
    id: CharacterId,
    name: String,
    /// The rank check refused it (or another reason that is final).
    refused: Option<(ErrorCode, String)>,
}

fn refusal(code: ErrorCode, message: impl Into<String>) -> GrantOutcome {
    GrantOutcome::Failed {
        code,
        message: message.into(),
    }
}

fn failed_code(f: &flyff_game::admin::AdminFailure) -> ErrorCode {
    ApiError::from(f).code
}

/// Resolves the recipients of a request (25 §5.3).
async fn resolve(
    st: &ApiState,
    who: &Identity,
    actor: AdminActor,
    recipients: &Recipients,
) -> ApiResult<Vec<Target>> {
    let ops = &st.ops;
    let mut targets: Vec<Target> = Vec::new();
    // "guild of ..." and "party of ..." are lists of characters (WP-ADMIN-23): the members, online first
    let expanded;
    let recipients = match recipients {
        Recipients::Guild(g) => {
            expanded = Recipients::Characters(guild_members(st, *g).await?);
            &expanded
        }
        Recipients::Party(p) => {
            expanded = Recipients::Characters(party_members(st, *p).await?);
            &expanded
        }
        other => other,
    };
    match recipients {
        Recipients::Characters(ids) => {
            let mut seen = BTreeSet::new();
            let ids: Vec<CharacterId> = ids
                .iter()
                .filter(|i| seen.insert(**i))
                .map(|i| CharacterId(*i))
                .collect();
            if ids.len() > RECIPIENTS_MAX {
                return Err(ApiError::new(
                    ErrorCode::Limit,
                    format!("at most {RECIPIENTS_MAX} recipients per grant"),
                ));
            }
            let facts = ops
                .persist
                .admin_recipient_facts(ids.clone())
                .await
                .map_err(|e| ApiError::db(&e))?;
            for id in ids {
                match facts.iter().find(|f| f.character_id == id) {
                    None => targets.push(Target {
                        id,
                        name: format!("#{}", id.0),
                        refused: Some((ErrorCode::NotFound, "no such character".to_owned())),
                    }),
                    Some(f) => targets.push(target_of(st, who, f)),
                }
            }
        }
        Recipients::AllOnline { .. } | Recipients::World(_) => {
            let (include_staff, world) = match recipients {
                Recipients::AllOnline { include_staff } => (*include_staff, None),
                Recipients::World(w) => (true, Some(*w)),
                _ => (false, None),
            };
            let rows = ops.online(None, actor).await?;
            for r in rows {
                if r.lingering || world.is_some_and(|w| r.world != w) {
                    continue;
                }
                if !include_staff && r.authority != 'F' {
                    continue;
                }
                let authority = Authority::from_db_byte(r.authority as u8);
                let refused = rank::check_target(who, authority)
                    .err()
                    .map(|e| (e.code, e.message));
                targets.push(Target {
                    id: r.character_id,
                    name: r.name,
                    refused,
                });
            }
            if targets.len() > RECIPIENTS_MAX {
                return Err(ApiError::new(
                    ErrorCode::Limit,
                    format!(
                        "{} players are online: at most {RECIPIENTS_MAX} recipients per grant",
                        targets.len()
                    ),
                ));
            }
        }
        // expanded to `Characters` above
        Recipients::Party(_) | Recipients::Guild(_) => {}
    }
    Ok(targets)
}

/// The members of guild `g`, the ones in the world first (the live model of the game thread).
async fn guild_members(st: &ApiState, g: u32) -> ApiResult<Vec<u32>> {
    match pl::game_query(st, AdminQuery::GuildDetail(flyff_common::GuildId(g))).await? {
        AdminQueryResult::Guild(d) => {
            let mut members: Vec<_> = d.members.iter().collect();
            members.sort_by_key(|m| !m.online);
            Ok(members.into_iter().map(|m| m.character.0).collect())
        }
        _ => Err(ApiError::internal("guild detail: unexpected answer")),
    }
}

/// The members of party `p`, in party order.
async fn party_members(st: &ApiState, p: u32) -> ApiResult<Vec<u32>> {
    match pl::game_query(st, AdminQuery::PartyMembers(flyff_common::PartyId(p))).await? {
        AdminQueryResult::Party(m) => Ok(m.into_iter().map(|c| c.0).collect()),
        _ => Err(ApiError::internal("party members: unexpected answer")),
    }
}

fn target_of(st: &ApiState, who: &Identity, f: &RecipientFact) -> Target {
    let authority = st.admin.resolver.target_authority(
        Authority::from_db_byte(f.account_authority),
        Some(Authority::from_db_byte(f.character_authority)),
        &f.account_name,
        Some(&f.name),
    );
    let refused = if f.state != CharacterState::Active {
        Some((
            ErrorCode::Invalid,
            format!("{} is not a playable character (deleted or sealed)", f.name),
        ))
    } else {
        rank::check_target(who, authority)
            .err()
            .map(|e| (e.code, e.message))
    };
    Target {
        id: f.character_id,
        name: f.name.clone(),
        refused,
    }
}

/// What the confirmation depends on (25 §2.3): recipients, everybody-online, penya and pieces over all recipients.
pub fn confirm_facts<'a>(
    req: &GrantRequest,
    recipients: usize,
    name: Option<&'a str>,
) -> ConfirmFacts<'a> {
    let per: u64 = req.items.iter().map(|i| u64::from(i.count)).sum();
    let n = u64::try_from(recipients).unwrap_or(u64::MAX);
    ConfirmFacts {
        targets: recipients,
        target_name: name,
        all_players: matches!(req.recipients, Recipients::AllOnline { .. }),
        irreversible: false,
        gold: req.gold.saturating_mul(n),
        items: per.saturating_mul(n),
    }
}

/// The request-level checks that need no database (limits, emptiness, validation of every line).
pub fn check_request(cx: &ValidateCx, req: &GrantRequest) -> ApiResult<()> {
    if req.items.len() > LINES_MAX {
        return Err(ApiError::new(
            ErrorCode::Limit,
            format!("at most {LINES_MAX} item lines per grant"),
        ));
    }
    if req.items.is_empty() && req.gold == 0 {
        return Err(ApiError::invalid("items", "there is nothing to grant"));
    }
    if req.gold > 2_147_483_647 {
        return Err(ApiError::invalid(
            "gold",
            "the penya is at most 2,147,483,647",
        ));
    }
    if let DeliveryModeCheck::Bad(msg) = mail_check(&req.mode) {
        return Err(ApiError::invalid("mode.subject", msg));
    }
    for (i, spec) in req.items.iter().enumerate() {
        if let Err(issues) = cx.record(spec) {
            let first = issues.first();
            return Err(ApiError::invalid(
                &format!(
                    "items[{i}].{}",
                    first.map_or("item_id", |f| f.field.as_str())
                ),
                issues
                    .iter()
                    .map(|f| format!("{}: {}", f.field, f.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
    }
    Ok(())
}

enum DeliveryModeCheck {
    Ok,
    Bad(&'static str),
}

fn mail_check(mode: &super::dto::DeliveryMode) -> DeliveryModeCheck {
    match mode {
        super::dto::DeliveryMode::Mail { subject, .. } if subject.trim().is_empty() => {
            DeliveryModeCheck::Bad("a mail needs a subject")
        }
        _ => DeliveryModeCheck::Ok,
    }
}

fn game_mode(mode: &super::dto::DeliveryMode) -> g::GrantMode {
    match mode {
        super::dto::DeliveryMode::Auto => g::GrantMode::Auto,
        super::dto::DeliveryMode::Direct => g::GrantMode::Direct,
        super::dto::DeliveryMode::Pending => g::GrantMode::Pending,
        super::dto::DeliveryMode::Mail { subject, text } => g::GrantMode::Mail {
            subject: subject.clone(),
            text: text.clone(),
        },
    }
}

fn outcome_of(o: &g::GrantOutcome) -> Option<GrantOutcome> {
    match o {
        g::GrantOutcome::Delivered { serials, gold } => Some(GrantOutcome::Delivered {
            serials: serials.clone(),
            gold: *gold,
        }),
        g::GrantOutcome::Mailed { mail_ids } => Some(GrantOutcome::Mailed {
            mail_ids: mail_ids.clone(),
        }),
        g::GrantOutcome::NeedsPending => None,
        g::GrantOutcome::Failed(f) => Some(GrantOutcome::Failed {
            code: failed_code(f),
            message: f.message.clone(),
        }),
    }
}

/// A grant that passed the request checks and has its recipients: the input of the confirmation and of [`run`].
pub struct Plan {
    cx: ValidateCx,
    actor: AdminActor,
    targets: Vec<Target>,
}

impl Plan {
    /// The recipients (resolved, with refusals marked).
    pub fn recipient_count(&self) -> usize {
        self.targets.len()
    }

    /// The name of the only recipient (what the operator types to confirm a one-person grant).
    pub fn only_name(&self) -> Option<&str> {
        match self.targets.as_slice() {
            [t] => Some(t.name.as_str()),
            _ => None,
        }
    }
}

/// Checks the request and resolves its recipients (steps 1 and 2 of the module documentation).
pub async fn plan(st: &ApiState, who: &Identity, req: &GrantRequest) -> ApiResult<Plan> {
    let cx = ValidateCx::of(&st.ops);
    check_request(&cx, req)?;
    let actor = who.actor();
    let targets = resolve(st, who, actor, &req.recipients).await?;
    Ok(Plan { cx, actor, targets })
}

/// Carries out a planned grant (steps 3 to 5). `reason` is the validated reason; `key` the idempotency key.
pub async fn run(
    st: &ApiState,
    who: &Identity,
    plan: Plan,
    req: &GrantRequest,
    reason: &ReasonText,
    key: Option<&str>,
) -> ApiResult<GrantReport> {
    let ops = &st.ops;
    let Plan { cx, actor, targets } = plan;
    let base = request_id(key);
    let now = ops.admin.clock.unix();

    // chunks of the targets the rank check let through
    let mut results: Vec<Option<GrantOutcome>> = Vec::with_capacity(targets.len());
    let mut pending: Vec<usize> = Vec::new();
    for t in &targets {
        results.push(
            t.refused
                .as_ref()
                .map(|(code, message)| refusal(*code, message.clone())),
        );
    }
    let live: Vec<usize> = (0..targets.len())
        .filter(|i| results[*i].is_none())
        .collect();
    let specs: Vec<g::ItemSpec> = req.items.iter().map(game_spec).collect();
    let mode = game_mode(&req.mode);
    for (chunk_no, chunk) in live.chunks(g::MAX_RECIPIENTS).enumerate() {
        let cmd = g::GrantBatch {
            request_id: base.wrapping_add(chunk_no as u128),
            mode: mode.clone(),
            grants: chunk
                .iter()
                .map(|i| g::GrantNow {
                    character: targets[*i].id,
                    items: specs.clone(),
                    gold: req.gold,
                })
                .collect(),
        };
        match ops.game(AdminCmd::GrantBatch(cmd), actor).await? {
            AdminOutput::Granted(report) => {
                for (idx, r) in chunk.iter().zip(report.per_recipient.iter()) {
                    match outcome_of(&r.outcome) {
                        Some(o) => results[*idx] = Some(o),
                        None => pending.push(*idx),
                    }
                }
            }
            AdminOutput::Failed(f) => {
                for idx in chunk {
                    results[*idx] = Some(GrantOutcome::Failed {
                        code: failed_code(&f),
                        message: f.message.clone(),
                    });
                }
            }
            other => {
                return Err(ApiError::internal(&format!(
                    "the game answered a grant with {other:?}"
                )));
            }
        }
    }

    // pending rows for the recipients the game handed back
    if !pending.is_empty() {
        let records: Vec<ItemRecord> = req.items.iter().filter_map(|s| cx.record(s).ok()).collect();
        let source = format!("admin:{}", who.label());
        for idx in pending {
            let t = &targets[idx];
            let mut rows: Vec<NewGrant> = records
                .iter()
                .map(|r| NewGrant {
                    character: t.id,
                    item: Some(r.clone()),
                    gold: 0,
                    source: source.clone(),
                    note: reason.as_str().to_owned(),
                })
                .collect();
            if req.gold > 0 {
                rows.push(NewGrant {
                    character: t.id,
                    item: None,
                    gold: i64::try_from(req.gold).unwrap_or(i64::MAX),
                    source: source.clone(),
                    note: reason.as_str().to_owned(),
                });
            }
            results[idx] = Some(match insert_many(&ops.persist, now, rows).await {
                Ok(ids) => GrantOutcome::Pending { grant_ids: ids },
                Err(e) => GrantOutcome::Failed {
                    code: e.code,
                    message: e.message,
                },
            });
        }
    }

    let report = GrantReport {
        request_id: uuid_text(base),
        results: targets
            .iter()
            .zip(results)
            .map(|(t, r)| {
                (
                    t.id.0,
                    r.unwrap_or_else(|| refusal(ErrorCode::Internal, "no result")),
                )
            })
            .collect(),
        names: targets.iter().map(|t| t.name.clone()).collect(),
    };
    audit_grant(st, who, req, reason, &report)?;
    Ok(report)
}

fn count_of(report: &GrantReport, pick: impl Fn(&GrantOutcome) -> bool) -> usize {
    report.results.iter().filter(|(_, o)| pick(o)).count()
}

/// `admin.grant{…}` with the full report (25 §5.3 "Audit"), plus one `item.create` per delivered item is written by
/// the game thread.
fn audit_grant(
    st: &ApiState,
    who: &Identity,
    req: &GrantRequest,
    reason: &ReasonText,
    report: &GrantReport,
) -> ApiResult<()> {
    let items: Vec<_> = req
        .items
        .iter()
        .map(|i| json!({"item_id": i.item_id, "count": i.count, "upgrade": i.upgrade}))
        .collect();
    audit_admin(
        &st.ops.audit,
        "admin.grant",
        who,
        "done",
        Some(reason),
        json!({
            "request_id": report.request_id,
            "mode": mode_name(&req.mode),
            "recipients": report.results.len(),
            "items": items,
            "gold": req.gold,
            "delivered": count_of(report, |o| matches!(o, GrantOutcome::Delivered { .. })),
            "mailed": count_of(report, |o| matches!(o, GrantOutcome::Mailed { .. })),
            "pending": count_of(report, |o| matches!(o, GrantOutcome::Pending { .. })),
            "failed": count_of(report, |o| matches!(o, GrantOutcome::Failed { .. })),
            "report": report,
        }),
    )
}

fn mode_name(m: &super::dto::DeliveryMode) -> &'static str {
    match m {
        super::dto::DeliveryMode::Auto => "auto",
        super::dto::DeliveryMode::Direct => "direct",
        super::dto::DeliveryMode::Pending => "pending",
        super::dto::DeliveryMode::Mail { .. } => "mail",
    }
}

// ---- the console word ---------------------------------------------------------------------------------------------------

/// Splits a console tail into words; a word in double quotes may contain blanks.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut any) = (false, false);
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The item called `name` (case-insensitive; the lowest id wins) or the item with that numeric id.
fn item_by_token(data: &GameData, token: &str) -> Option<u32> {
    if let Ok(id) = token.parse::<u32>() {
        return data.item(id).map(|_| id);
    }
    data.items
        .iter()
        .filter(|(_, p)| p.name.eq_ignore_ascii_case(token))
        .map(|(id, _)| id)
        .min()
}

/// The console word (Plan/23 §8.7): `grant <character> (item <itemId|"name"> [count] | gold <n>) [note...]` inserts
/// a `pending_grants` row with `source = "console"`; the answer is the line the console prints.
pub async fn console(ops: &AdminOps, tail: &str) -> Result<String, String> {
    const USAGE: &str = "grant <character> (item <itemId|\"name\"> [count] | gold <n>) [note...]";
    let w = words(tail);
    let (Some(who), Some(kind)) = (w.first(), w.get(1)) else {
        return Err(format!("usage: {USAGE}"));
    };
    let target = match who.parse::<u32>() {
        Ok(id) => ops
            .persist
            .admin_recipient_facts(vec![CharacterId(id)])
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .next(),
        Err(_) => ops
            .persist
            .admin_recipient_by_name(who.clone())
            .await
            .map_err(|e| e.to_string())?,
    };
    let Some(target) = target else {
        return Err(format!("no character called {who}"));
    };
    if target.state != CharacterState::Active {
        return Err(format!("{} is not a playable character", target.name));
    }
    let cx = ValidateCx::of(ops);
    let (item, gold, note_from): (Option<(u32, u16)>, i64, usize) =
        match kind.to_ascii_lowercase().as_str() {
            "gold" => {
                let n = w
                    .get(2)
                    .and_then(|t| t.parse::<i64>().ok())
                    .ok_or_else(|| format!("usage: {USAGE}"))?;
                (None, n, 3)
            }
            "item" => {
                let token = w.get(2).ok_or_else(|| format!("usage: {USAGE}"))?;
                let id = item_by_token(&cx.data, token)
                    .ok_or_else(|| format!("no item called {token}"))?;
                let (count, next) = match w.get(3).and_then(|t| t.parse::<u16>().ok()) {
                    Some(n) => (n, 4),
                    None => (1, 3),
                };
                (Some((id, count)), 0, next)
            }
            _ => return Err(format!("usage: {USAGE}")),
        };
    let note = w.get(note_from..).map(|r| r.join(" ")).unwrap_or_default();
    let record = match item {
        Some((id, count)) => {
            let spec = ItemSpec {
                item_id: id,
                count,
                upgrade: 0,
                element: None,
                element_level: 0,
                random_options: Vec::new(),
                safe: false,
                piercing: Vec::new(),
                ultimate_gems: Vec::new(),
                vis: Vec::new(),
                pet: None,
                charged: false,
                keep_until: KeepUntil::Item,
                bound: None,
                text: None,
                guild_id: None,
            };
            Some(cx.record(&spec).map_err(|issues| {
                issues
                    .iter()
                    .map(|i| format!("{}: {}", i.field, i.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            })?)
        }
        None => {
            if !(1..=2_147_483_647).contains(&gold) {
                return Err("the penya is 1..=2147483647".to_owned());
            }
            None
        }
    };
    let what = match &record {
        Some(r) => format!(
            "{} x {}",
            r.count,
            cx.data.item(r.item_id).map_or("?", |p| p.name.as_str())
        ),
        None => format!("{gold} penya"),
    };
    let id = insert(
        &ops.persist,
        ops.admin.clock.unix(),
        NewGrant {
            character: target.character_id,
            item: record,
            gold,
            source: "console".to_owned(),
            note,
        },
    )
    .await
    .map_err(|e| e.message)?;
    Ok(format!(
        "grant {id}: {what} for {} (delivered at the next login)",
        target.name
    ))
}
