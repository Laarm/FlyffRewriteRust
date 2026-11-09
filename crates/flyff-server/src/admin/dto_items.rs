//! The DTOs of the items area (Plan/25 §5, §11.3, §11.10; WP-ADMIN-20, WP-ADMIN-22): the validation answer, the
//! catalogue metadata, the grant and removal lists, templates and kits, the ownership search with its NDJSON events,
//! the duplicate report and the economy overview. They live in their own module so that the shared `dto.rs` stays
//! small; the generated `web/src/admin/gen/dto.ts` lists them after the catalogue of `dto.rs` (`declare`).
//!
//! Conventions as in [`super::dto`]: enums are `snake_case`, 64-bit values are plain JSON numbers (the 64-bit
//! random-option word is the exception: it is a decimal **string**, a JavaScript number cannot hold bit 63), ids
//! are `u32`, response types always write every key (`null` when empty), request types accept a missing optional
//! field.

use super::dto::{CharacterId, ItemSpec, Kit};
use serde::{Deserialize, Serialize};

// ---- validation ---------------------------------------------------------------------------------------------------

/// One problem of an item spec or one note about it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemIssue {
    /// The field the builder highlights (`random_options[1].adj`).
    pub field: String,
    /// A stable tag (`count_range`, `not_upgradable`, ...).
    pub code: String,
    /// For the operator.
    pub message: String,
}

/// Which builder fields an item has and their limits (25 §5.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemFieldsDto {
    /// The largest stack (1 = not stackable).
    pub stack_max: u32,
    /// The highest upgrade level (0 = not upgradable).
    pub upgrade_max: u8,
    /// The element fields apply.
    pub element: bool,
    /// The most element levels.
    pub element_level_max: u8,
    /// Random option lines can be set.
    pub random_options: bool,
    /// The most card sockets (0 = none).
    pub piercing_max: u8,
    /// The `IK3_*` of the cards the sockets take.
    pub piercing_card_kind: u32,
    /// Ultimate gems can be set.
    pub ultimate: bool,
    /// Vis can be set.
    pub vis: bool,
    /// A system pet can be set.
    pub pet: bool,
    /// The charged flag can be set.
    pub charged: bool,
    /// The text field is meaningful.
    pub text: bool,
    /// The item is a guild cloak.
    pub guild: bool,
}

/// A stored item as the console shows and sends it (`StoredItem`, 13 §6.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct StoredItemDto {
    /// The item id.
    pub item_id: u32,
    /// The stack count.
    pub count: u32,
    /// The serial (0 = numbered at delivery).
    pub serial: u64,
    /// `+N`.
    pub upgrade_level: i32,
    /// The element, 0 = none.
    pub element: u32,
    /// The element level.
    pub element_level: i32,
    /// The commercial element stone, 0 = none.
    pub element_stone_id: u32,
    /// The guild of a guild cloak.
    pub guild_id: u32,
    /// The charged flag.
    pub charged: bool,
    /// `m_byFlag` (1 expired, 2 binds).
    pub flags: u32,
    /// Unix seconds, 0 = permanent.
    pub keep_until: i64,
    /// The packed 64-bit random-option word as a decimal string.
    pub random_option: String,
    /// The item text.
    pub text: String,
    /// The `m_nHitPoint` of an ordinary item, `-1` = the item's own.
    pub endurance: i32,
    /// The cards of the sockets (0 = empty), or the vis of a vis pet.
    pub piercing_cards: Vec<u32>,
    /// The ability items of the ultimate sockets (0 = empty).
    pub ultimate_gems: Vec<u32>,
    /// The keep time of each vis slot.
    pub vis_keep_until: Vec<i64>,
    /// The system pet of an egg.
    pub pet: Option<super::dto::PetRecord>,
}

/// The answer of `POST /admin/api/items/validate`: always 200, the problems are in the body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ValidatedItem {
    /// No error.
    pub ok: bool,
    /// The item name from the game data (empty for an unknown id).
    pub name: String,
    /// The normalised item (`None` when there are errors).
    pub item: Option<StoredItemDto>,
    /// Every wrong field.
    pub errors: Vec<ItemIssue>,
    /// Notes that do not stop the grant.
    pub warnings: Vec<ItemIssue>,
    /// The fields of the item (`None` for an unknown id).
    pub fields: Option<ItemFieldsDto>,
}

// ---- the catalogue --------------------------------------------------------------------------------------------------

/// A node of the kind tree (`IK1` > `IK2` > `IK3`) with a readable name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct KindNode {
    /// `1`, `1.2`, `1.2.3`: the `IK1`, `IK2`, `IK3` values on the path.
    pub key: String,
    /// The readable name (`Weapons`, `Armour`, ...).
    pub name: String,
    /// Items below this node.
    pub count: u32,
    /// The children (empty at the third level).
    pub children: Vec<KindNode>,
}

/// An id with a name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NamedId {
    /// The id.
    pub id: u32,
    /// The name.
    pub name: String,
}

/// One ability a gem can carry on an ultimate weapon (`Ultimate_GemAbility.txt`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct AbilityDto {
    /// The ability item (what the socket stores).
    pub id: u32,
    /// The gem item.
    pub gem: u32,
    /// The destination parameter (`DST_*`).
    pub dst: u32,
    /// The five values, one per ultimate level.
    pub values: Vec<i32>,
}

/// The ranges of the builder (25 §5.2, 19 §2): the client shows them and the golden file pins them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemLimits {
    /// The largest stack of any item.
    pub count_max: u32,
    /// Gear upgrade levels (`+10`).
    pub upgrade_gear: u8,
    /// Accessory upgrade levels (`+20`).
    pub upgrade_accessory: u8,
    /// Collector upgrade levels (`+5`).
    pub upgrade_collector: u8,
    /// Ultimate weapon upgrade levels (`+10`).
    pub upgrade_ultimate: u8,
    /// Element levels.
    pub element_level: u8,
    /// Random option lines.
    pub option_lines: u8,
    /// The `DST` id range of a line.
    pub option_dst_max: u8,
    /// The `adj` magnitude of a line.
    pub option_adj_max: u16,
    /// Card sockets of a suit.
    pub piercing_suit: u8,
    /// Card sockets of a weapon or shield.
    pub piercing_weapon: u8,
    /// Ultimate gem sockets at most.
    pub ultimate_gems: u8,
    /// Vis at most.
    pub vis: u8,
    /// Pet experience of an egg / of a pet.
    pub pet_exp_egg: u32,
    /// Pet experience of a hatched pet.
    pub pet_exp: u32,
    /// Pet life.
    pub pet_life: u16,
    /// The energy maximum of each level, egg first.
    pub pet_energy: Vec<u32>,
    /// Item text bytes.
    pub text_bytes: u8,
    /// Pet name bytes.
    pub pet_name_bytes: u8,
    /// Item lines per grant.
    pub basket_lines: u32,
    /// Recipients per grant.
    pub recipients: u32,
    /// The wallet limit.
    pub gold_max: u64,
    /// Mailbox size.
    pub mailbox: u32,
}

/// Switches the builder shows as info chips.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemFeatures {
    /// `features.no_piercing`: gear sockets have no effect.
    pub no_piercing: bool,
    /// `features.only_armor_awakes`: only suits, gloves, boots and helmets awaken.
    pub only_armor_awakes: bool,
    /// The pending-grant delivery exists (WP-ECON-09): Pending and Auto-for-offline deliver.
    pub pending_delivery: bool,
    /// `features.inventory_168`.
    pub inventory_168: bool,
}

/// `GET /admin/api/items/catalog-meta`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CatalogMeta {
    /// The data fingerprint (the cache key of the client).
    pub version: String,
    /// The ranges.
    pub limits: ItemLimits,
    /// The switches.
    pub features: ItemFeatures,
    /// The kind tree.
    pub kinds: Vec<KindNode>,
    /// The elements.
    pub elements: Vec<NamedId>,
    /// The ultimate gem abilities.
    pub abilities: Vec<AbilityDto>,
    /// The distinct field profiles.
    pub profiles: Vec<ItemFieldsDto>,
    /// `(item id, profile index)` for every item.
    pub items: Vec<(u32, u16)>,
}

// ---- grants and removals --------------------------------------------------------------------------------------------

/// A pending grant (`GET /admin/api/grants`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GrantRowDto {
    /// The row id.
    pub id: i64,
    /// The receiver.
    pub character_id: CharacterId,
    /// Its name.
    pub character_name: String,
    /// `item` or `gold`.
    pub kind: String,
    /// The item of an item grant.
    pub item: Option<StoredItemDto>,
    /// The penya of a gold grant.
    pub gold: i64,
    /// `admin:<actor>`, `cli`, `event:`, `guild_salary:`.
    pub source: String,
    /// The operator's note.
    pub note: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds when delivered.
    pub delivered_at: Option<i64>,
    /// Unix seconds when cancelled.
    pub cancelled_at: Option<i64>,
    /// Who cancelled it.
    pub cancelled_by: Option<String>,
    /// `open`, `done` or `cancelled`.
    pub state: String,
}

/// A pending removal (`GET /admin/api/removals`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RemovalRowDto {
    /// The row id.
    pub id: i64,
    /// The character of a character removal.
    pub character_id: Option<CharacterId>,
    /// Its name.
    pub character_name: Option<String>,
    /// The guild of a guild-bank removal.
    pub guild_id: Option<u32>,
    /// Its name.
    pub guild_name: Option<String>,
    /// `inventory`, `bank`, `pocket` or `guild_bank`.
    pub location: String,
    /// The item id, 0 = penya.
    pub item_id: u32,
    /// The exact serial wanted.
    pub serial: Option<u64>,
    /// How many pieces, or the penya amount.
    pub count: i64,
    /// Who asked.
    pub requested_by: String,
    /// Why.
    pub reason: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds when applied.
    pub done_at: Option<i64>,
    /// Pieces removed.
    pub removed_count: Option<i64>,
    /// Unix seconds when cancelled.
    pub cancelled_at: Option<i64>,
    /// Who cancelled it.
    pub cancelled_by: Option<String>,
    /// `open`, `done` or `cancelled`.
    pub state: String,
}

/// The answer of a cancel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CancelResult {
    /// The row.
    pub id: i64,
    /// `cancelled` (this call did it) or `already_cancelled` (an earlier call did).
    pub state: String,
}

// ---- templates and kits ----------------------------------------------------------------------------------------------

/// `item` or `kit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TemplateKindDto {
    /// One item spec.
    Item,
    /// A basket of item specs plus penya.
    Kit,
}

/// A saved template or kit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TemplateRowDto {
    /// The row id.
    pub id: i64,
    /// Template or kit.
    pub kind: TemplateKindDto,
    /// The name (unique per kind, case-insensitive).
    pub name: String,
    /// The item of a template.
    pub item: Option<ItemSpec>,
    /// The basket of a kit.
    pub kit: Option<Kit>,
    /// The author (`staff:<account>`).
    pub created_by: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub updated_at: i64,
}

/// The body of a template create / update.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TemplateBody {
    /// Template or kit (ignored by an update: a row keeps its kind).
    pub kind: TemplateKindDto,
    /// The name.
    pub name: String,
    /// The item of a template.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub item: Option<ItemSpec>,
    /// The basket of a kit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub kit: Option<Kit>,
}

// ---- ownership search, duplicates, economy ----------------------------------------------------------------------------

/// The body of `POST /admin/api/items/search`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemSearchBody {
    /// The item id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub item_id: Option<u32>,
    /// The serial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub serial: Option<u64>,
    /// Only items with this `+N` or more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub min_upgrade: Option<i32>,
    /// Only items with this element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub element: Option<u8>,
    /// Stop after this many hits (default 2,000; at most 10,000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub max_hits: Option<u32>,
}

/// Where a ground item lies.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GroundDto {
    /// The object id of the drop.
    pub obj_id: u32,
    /// World.
    pub world: u32,
    /// Layer.
    pub layer: i32,
    /// Position.
    pub x: f32,
    /// Height.
    pub y: f32,
    /// Position.
    pub z: f32,
}

/// One found item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ItemHitDto {
    /// `live` (the game thread, current) or `db` (saved; an online account's rows are not listed).
    pub source: String,
    /// `equipment`, `inventory`, `pocket`, `bank`, `guild_bank`, `mail`, `pending_grant` or `ground`.
    pub site: String,
    /// The pocket number of a pocket.
    pub pocket: Option<u8>,
    /// The owner character.
    pub character_id: Option<CharacterId>,
    /// Its name.
    pub character_name: Option<String>,
    /// Its account.
    pub account_name: Option<String>,
    /// The guild of a guild bank.
    pub guild_id: Option<u32>,
    /// Its name.
    pub guild_name: Option<String>,
    /// The mail holding the item.
    pub mail_id: Option<i64>,
    /// The pending grant holding it.
    pub grant_id: Option<i64>,
    /// The slot in its container.
    pub slot: Option<u32>,
    /// The character slot of a bank compartment.
    pub bank_slot: Option<u8>,
    /// Where a ground item lies.
    pub ground: Option<GroundDto>,
    /// When the saved copy was written (`db` rows of characters), or the creation of a mail or a grant.
    pub saved_at: Option<i64>,
    /// The item.
    pub item: StoredItemDto,
}

/// One line of the NDJSON stream of `POST /admin/api/items/search`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchEvent {
    /// The live half is answered; the database scan starts.
    Start {
        /// Live hits (listed right after).
        live: u32,
        /// The accounts whose database rows are skipped (their live copy was listed).
        skipped_accounts: u32,
    },
    /// One hit.
    Hit(Box<ItemHitDto>),
    /// Progress of the scan.
    Progress {
        /// Rows examined.
        scanned: u64,
        /// Rows to examine.
        total: Option<u64>,
    },
    /// The scan ended.
    Done {
        /// Rows examined.
        scanned: u64,
        /// Hits listed.
        hits: u32,
        /// More hits exist than were listed.
        truncated: bool,
    },
    /// The scan failed.
    Error {
        /// The error code.
        code: String,
        /// For the operator.
        message: String,
    },
}

/// One place of a duplicated serial.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DuplicatePlaceDto {
    /// `equipment`, `inventory`, `pocket`, `bank`, `guild_bank`, `mail`, `pending_grant`.
    pub site: String,
    /// The pocket number.
    pub pocket: Option<u8>,
    /// The owner character.
    pub character_id: Option<CharacterId>,
    /// Its name.
    pub character_name: Option<String>,
    /// Its account.
    pub account_name: Option<String>,
    /// The guild of a guild bank.
    pub guild_id: Option<u32>,
    /// The mail.
    pub mail_id: Option<i64>,
    /// The pending grant.
    pub grant_id: Option<i64>,
    /// The slot.
    pub slot: Option<u32>,
    /// The item id there.
    pub item_id: u32,
    /// The stack size there.
    pub count: u32,
}

/// A serial that exists more than once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DuplicateDto {
    /// The serial.
    pub serial: u64,
    /// Its places, at least two.
    pub places: Vec<DuplicatePlaceDto>,
}

/// `POST /admin/api/items/duplicates`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct DuplicatesDto {
    /// The duplicated serials, ascending.
    pub duplicates: Vec<DuplicateDto>,
    /// Items examined.
    pub scanned: u64,
    /// More duplicates exist than were listed.
    pub truncated: bool,
}

/// Penya by holder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PenyaDto {
    /// Wallets of playable characters.
    pub inventories: i64,
    /// Their banks.
    pub banks: i64,
    /// Guild banks.
    pub guild_banks: i64,
    /// Open mails.
    pub mails: i64,
    /// Vendors (always 0: a stall keeps its proceeds in the wallet).
    pub vendors: i64,
    /// The sum of the five.
    pub total: i64,
    /// Undelivered gold grants.
    pub pending_grants: i64,
}

/// A rich character.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RichCharacterDto {
    /// The character.
    pub character_id: CharacterId,
    /// Its name.
    pub name: String,
    /// Its account.
    pub account: String,
    /// The wallet.
    pub gold: i64,
    /// Its own bank.
    pub bank_gold: i64,
}

/// A rich account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RichAccountDto {
    /// The account id.
    pub account_id: u32,
    /// Its name.
    pub name: String,
    /// The wallets of its characters.
    pub wallets: i64,
    /// Their banks.
    pub banks: i64,
    /// Both.
    pub total: i64,
}

/// Pieces of one item by site.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CensusDto {
    /// The item.
    pub item_id: u32,
    /// Its name.
    pub name: String,
    /// Pieces in total.
    pub pieces: u64,
    /// Stacks in total.
    pub stacks: u64,
    /// Pieces per site (`inventory`, `bank`, `mail`, ...).
    pub by_site: std::collections::BTreeMap<String, u64>,
}

/// Grants created on one day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GrantDayDto {
    /// `YYYY-MM-DD` (UTC).
    pub day: String,
    /// Pending grants created.
    pub grants: u64,
    /// Of which gold.
    pub gold_grants: u64,
    /// Penya in them.
    pub gold: i64,
}

/// `GET /admin/api/economy`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EconomyDto {
    /// When it was computed (unix seconds).
    pub as_of: i64,
    /// Seconds since then.
    pub age_s: i64,
    /// Served from the 10 minute cache.
    pub cached: bool,
    /// Playable characters.
    pub characters: u64,
    /// Accounts with a playable character.
    pub accounts: u64,
    /// Penya by holder.
    pub penya: PenyaDto,
    /// The richest characters.
    pub richest_characters: Vec<RichCharacterDto>,
    /// The richest accounts.
    pub richest_accounts: Vec<RichAccountDto>,
    /// The counted item.
    pub census: CensusDto,
    /// Grants per day, newest first.
    pub grants_per_day: Vec<GrantDayDto>,
}

/// Appends the declarations of this module to `out` (the generator of `dto.rs` calls it).
#[cfg(feature = "ts")]
pub fn declare(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;

    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    push::<ItemIssue>(out, cfg);
    push::<ItemFieldsDto>(out, cfg);
    push::<StoredItemDto>(out, cfg);
    push::<ValidatedItem>(out, cfg);
    push::<KindNode>(out, cfg);
    push::<NamedId>(out, cfg);
    push::<AbilityDto>(out, cfg);
    push::<ItemLimits>(out, cfg);
    push::<ItemFeatures>(out, cfg);
    push::<CatalogMeta>(out, cfg);
    push::<GrantRowDto>(out, cfg);
    push::<RemovalRowDto>(out, cfg);
    push::<CancelResult>(out, cfg);
    push::<TemplateKindDto>(out, cfg);
    push::<TemplateRowDto>(out, cfg);
    push::<TemplateBody>(out, cfg);
    push::<ItemSearchBody>(out, cfg);
    push::<GroundDto>(out, cfg);
    push::<ItemHitDto>(out, cfg);
    push::<SearchEvent>(out, cfg);
    push::<DuplicatePlaceDto>(out, cfg);
    push::<DuplicateDto>(out, cfg);
    push::<DuplicatesDto>(out, cfg);
    push::<PenyaDto>(out, cfg);
    push::<RichCharacterDto>(out, cfg);
    push::<RichAccountDto>(out, cfg);
    push::<CensusDto>(out, cfg);
    push::<GrantDayDto>(out, cfg);
    push::<EconomyDto>(out, cfg);
}
