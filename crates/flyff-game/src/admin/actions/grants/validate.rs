//! `ItemSpec` -> `ItemRecord` (Plan/25 §5.2, §11.10; WP-ADMIN-20): which fields an item has ([`ItemFields`]) and the
//! normalising validation behind `POST /admin/api/items/validate` and every grant (the game thread validates again:
//! the route is not trusted).
//!
//! The rules (the *golden table* of `grants/tests.rs` pins every row):
//!
//! | field | applies to | rule |
//! |---|---|---|
//! | `item_id` | - | a `propItem` row that is not `IK3_VIRTUAL` |
//! | `count` | all | `1..=pack_max` (a non-stackable item is 1) |
//! | `upgrade` | an ultimate weapon `0..=10`; gear (suit, armour, weapon) `0..=10`; an accessory of `accessory.inc` `0..=20`; the normal collector `0..=5` | else 0 only |
//! | `element`, `element_level` | `IsEleRefineryAble` (suits, weapons) | element `1..=5`, level `1..=20`; without an element the level is 0. `element_stone_id` is **derived**: always 0 (a commercial stone would make the item *charged*, forbid mailing it and block every later upgrade) |
//! | `random_options` | weapons, armour, system eggs, eat pets | at most 3 lines, `dst 1..=127`, `adj -511..=511` and not 0; a warning when the game ignores them (`features.only_armor_awakes`); `safe` needs a line |
//! | `piercing` | suit <= 4 sockets (`IK3_SOCKETCARD`), shield and weapons <= 10 (`IK3_SOCKETCARD2`) | `0` = an empty socket; a warning while `features.no_piercing` |
//! | `ultimate_gems` | ultimate weapons | at most `upgrade - 5` (and 5) sockets, each `0` or an ability item of `Ultimate_GemAbility.txt`; the list is padded to the socket count |
//! | `vis` | vis pets | at most 9, each an `IK3_VIS` item with its keep time |
//! | `pet` | system pet eggs | kind `0..=3`, level `0..=5`, exp <= 50,000 (egg) / 100,000, energy <= the level's maximum, life <= 99, rolled avail levels `0..=9` up to the pet's level, name <= 16 bytes |
//! | `charged` | items whose prop is a cash item | bool |
//! | `keep_until` | all | `Item` / `None` = no expiry; a duration or a date in the future (before 2100) |
//! | `bound` | all | `Some(true)` sets the binds flag, `None` keeps the item's default |
//! | `text` | all (shown for text, GM-text, seal and disguise items) | <= 31 bytes, no control characters |
//! | `guild_id` | `IK3_CLOAK` | a guild cloak; a warning: the load strips it unless the character is in that guild |

use super::{ItemSpec, KeepUntil};
use crate::item::env::ItemConfig;
use crate::item::model::{MAX_PIERCING_ULTIMATE, MAX_VIS};
use flyff_common::UnixTime;
use flyff_common::defs::item::II_GEN_TOO_COL_NORMAL;
use flyff_common::defs::itemkind::{
    IK1_ARMOR, IK1_WEAPON, IK2_GMTEXT, IK2_TEXT, IK2_WEAPON_DIRECT, IK2_WEAPON_MAGIC, IK3_CLOAK,
    IK3_EGG, IK3_PET, IK3_SHIELD, IK3_SOCKETCARD, IK3_SOCKETCARD2, IK3_SUIT, IK3_TEXT_DISGUISE,
    IK3_VIRTUAL, IK3_VIS,
};
use flyff_data::GameData;
use flyff_data::props::item::ItemProp;
use flyff_db::{ItemRecord, PetRecord};
use flyff_rules::item::random_option::{
    RandomOptKind, RandomOptLine, RandomOptWord, random_option_kind,
};
use flyff_rules::item::refine::{MAX_ACCESSORY_AO, UpgradeTables};
use flyff_rules::item::smelt::{is_dice_refinery_able, is_ele_refinery_able, is_ultimate_weapon};
use flyff_rules::item::ultimate::{MAX_ULTIMATE_AO, gem_sockets};
use flyff_rules::item::{ITEM_FLAG_BINDS, pet as pet_rules};

/// The highest keep time a spec may name (2100-01-01); `m_dwKeepTime` is a 32-bit unix time.
pub const KEEP_MAX: i64 = 4_102_444_800;
/// The longest item text (`m_szItemText[32]`).
pub const TEXT_MAX_BYTES: usize = 31;
/// The longest pet name (`CPet` name, 16 bytes).
pub const PET_NAME_MAX_BYTES: usize = 16;
/// The most random option lines (`MAX_RANDOM_OPTION`).
pub const OPTION_LINES_MAX: usize = 3;
/// The `adj` limit of a line (9-bit magnitude).
pub const OPTION_ADJ_MAX: i16 = 511;
/// The `dst` limit of a line (7 bits).
pub const OPTION_DST_MAX: u8 = 127;

/// What the validation reads besides the spec.
#[derive(Clone, Copy, Debug)]
pub struct ValidateEnv<'a> {
    /// The loaded game data.
    pub data: &'a GameData,
    /// The item switches (`features.no_piercing`, `features.only_armor_awakes`).
    pub cfg: &'a ItemConfig,
    /// The upgrade limits (`ItemUpgrade.lua`).
    pub tables: &'a UpgradeTables,
    /// The wall clock (durations become absolute times).
    pub now: UnixTime,
}

/// A wrong field: `field` is the dotted path the builder highlights (`random_options[1].adj`), `code` a stable
/// snake_case tag the tests and the client can match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    /// The field.
    pub field: String,
    /// The tag.
    pub code: &'static str,
    /// For the operator.
    pub message: String,
}

/// A note that does not stop the grant (shown as an info chip).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// The field.
    pub field: String,
    /// The tag.
    pub code: &'static str,
    /// For the operator.
    pub message: String,
}

/// A validated spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Normalised {
    /// The item as it will be stored: `serial` and `obj_id` are 0 (assigned at delivery).
    pub record: ItemRecord,
    /// Things the operator should know.
    pub warnings: Vec<Warning>,
}

/// Which builder fields an item has and their limits (the visibility rules of 25 §5.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemFields {
    /// The largest stack (1 = not stackable).
    pub stack_max: u32,
    /// The highest upgrade level (`0` = the item cannot be upgraded).
    pub upgrade_max: u8,
    /// The element fields apply.
    pub element: bool,
    /// The most element levels.
    pub element_level_max: u8,
    /// Random option lines can be set.
    pub random_options: bool,
    /// Sockets for cards: `(most sockets, IK3 of the card)`.
    pub piercing: Option<(u8, u32)>,
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

/// The fields of `prop` under `env` (the single source of the builder's visibility and of the validation).
pub fn fields_of(env: &ValidateEnv<'_>, prop: &ItemProp) -> ItemFields {
    let upgrade_max = if is_ultimate_weapon(prop) {
        MAX_ULTIMATE_AO
    } else if prop.id == II_GEN_TOO_COL_NORMAL {
        env.tables.max_collector()
    } else if env.data.accessory.items.iter().any(|a| a.item == prop.id) {
        MAX_ACCESSORY_AO
    } else if is_dice_refinery_able(prop) {
        env.tables.max_general()
    } else {
        0
    };
    let element = is_ele_refinery_able(prop);
    let piercing = if prop.item_kind3 == IK3_SUIT {
        Some((4, IK3_SOCKETCARD))
    } else if prop.item_kind3 == IK3_SHIELD
        || matches!(prop.item_kind2, IK2_WEAPON_DIRECT | IK2_WEAPON_MAGIC)
    {
        Some((10, IK3_SOCKETCARD2))
    } else {
        None
    };
    ItemFields {
        stack_max: prop.pack_max.max(1),
        upgrade_max: u8::try_from(upgrade_max).unwrap_or(0),
        element,
        element_level_max: if element {
            u8::try_from(env.tables.max_attribute()).unwrap_or(20)
        } else {
            0
        },
        random_options: prop.item_kind1 == IK1_WEAPON
            || prop.item_kind1 == IK1_ARMOR
            || prop.item_kind3 == IK3_EGG
            || prop.item_kind3 == IK3_PET,
        piercing,
        ultimate: is_ultimate_weapon(prop),
        vis: prop.is_vis_pet(),
        pet: prop.item_kind3 == IK3_EGG,
        charged: prop.charged != 0,
        text: matches!(prop.item_kind2, IK2_TEXT | IK2_GMTEXT)
            || prop.item_kind3 == IK3_TEXT_DISGUISE,
        guild: prop.item_kind3 == IK3_CLOAK,
    }
}

struct Sink {
    errors: Vec<FieldError>,
    warnings: Vec<Warning>,
}

impl Sink {
    fn err(&mut self, field: impl Into<String>, code: &'static str, message: impl Into<String>) {
        self.errors.push(FieldError {
            field: field.into(),
            code,
            message: message.into(),
        });
    }

    fn warn(&mut self, field: impl Into<String>, code: &'static str, message: impl Into<String>) {
        self.warnings.push(Warning {
            field: field.into(),
            code,
            message: message.into(),
        });
    }
}

/// The absolute keep time a rule asks for (`0` = none); `None` after an error was recorded.
fn keep_time(env: &ValidateEnv<'_>, field: &str, keep: KeepUntil, sink: &mut Sink) -> Option<i64> {
    match keep {
        KeepUntil::Item | KeepUntil::None => Some(0),
        KeepUntil::Duration { secs } => {
            let secs = i64::try_from(secs).unwrap_or(i64::MAX);
            let at = env.now.0.saturating_add(secs);
            if secs == 0 {
                sink.err(
                    field,
                    "keep_until",
                    "a duration must be at least one second",
                );
                None
            } else if at > KEEP_MAX {
                sink.err(
                    field,
                    "keep_until",
                    "the expiry is later than the year 2100",
                );
                None
            } else {
                Some(at)
            }
        }
        KeepUntil::At { unix } => {
            if unix <= env.now.0 {
                sink.err(field, "keep_until", "the expiry date is in the past");
                None
            } else if unix > KEEP_MAX {
                sink.err(
                    field,
                    "keep_until",
                    "the expiry is later than the year 2100",
                );
                None
            } else {
                Some(unix)
            }
        }
    }
}

/// Validates `spec` and returns the item as it will be stored, or every wrong field (see the module
/// documentation for the rules).
pub fn validate(env: &ValidateEnv<'_>, spec: &ItemSpec) -> Result<Normalised, Vec<FieldError>> {
    let mut sink = Sink {
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let Some(prop) = env.data.item(spec.item_id).filter(|_| spec.item_id != 0) else {
        sink.err(
            "item_id",
            "unknown_item",
            format!("item {} does not exist", spec.item_id),
        );
        return Err(sink.errors);
    };
    if prop.item_kind3 == IK3_VIRTUAL {
        sink.err(
            "item_id",
            "virtual_item",
            format!("{} is a placeholder, it cannot exist in a bag", prop.name),
        );
        return Err(sink.errors);
    }
    let f = fields_of(env, prop);
    let mut rec = ItemRecord::new(0, spec.item_id, spec.count, 0);

    // count
    if spec.count == 0 || u32::from(spec.count) > f.stack_max {
        sink.err(
            "count",
            "count_range",
            if f.stack_max == 1 {
                format!("{} does not stack: the count is 1", prop.name)
            } else {
                format!("the count is 1..={}", f.stack_max)
            },
        );
    }

    // upgrade
    if spec.upgrade > f.upgrade_max {
        sink.err(
            "upgrade",
            if f.upgrade_max == 0 {
                "not_upgradable"
            } else {
                "upgrade_range"
            },
            if f.upgrade_max == 0 {
                format!("{} cannot be upgraded", prop.name)
            } else {
                format!("the upgrade level is 0..={}", f.upgrade_max)
            },
        );
    } else {
        rec.upgrade_level = i32::from(spec.upgrade);
    }

    // element (the commercial stone id is derived: always 0)
    if spec.element != 0 || spec.element_level != 0 {
        if !f.element {
            sink.err(
                "element",
                "not_element_capable",
                format!("{} cannot take an element", prop.name),
            );
        } else if spec.element == 0 {
            sink.err(
                "element",
                "element_missing",
                "an element level needs an element",
            );
        } else if spec.element > 5 {
            sink.err(
                "element",
                "element_range",
                "the element is 1 (fire) to 5 (earth)",
            );
        } else if spec.element_level == 0 || spec.element_level > f.element_level_max {
            sink.err(
                "element_level",
                "element_level_range",
                format!("the element level is 1..={}", f.element_level_max),
            );
        } else {
            rec.element = spec.element;
            rec.element_level = i32::from(spec.element_level);
        }
    }

    // random options
    let mut word = RandomOptWord(0);
    if !spec.random_options.is_empty() || spec.safe {
        if !f.random_options {
            sink.err(
                "random_options",
                "no_random_options",
                format!("{} cannot carry random options", prop.name),
            );
        } else if spec.random_options.len() > OPTION_LINES_MAX {
            sink.err(
                "random_options",
                "random_option_lines",
                format!("at most {OPTION_LINES_MAX} option lines"),
            );
        } else {
            let mut ok = true;
            for (i, l) in spec.random_options.iter().enumerate() {
                if l.dst == 0 || l.dst > OPTION_DST_MAX {
                    sink.err(
                        format!("random_options[{i}].dst"),
                        "random_option_dst",
                        format!("the DST id is 1..={OPTION_DST_MAX}"),
                    );
                    ok = false;
                }
                if l.adj == 0 || l.adj.abs() > OPTION_ADJ_MAX {
                    sink.err(
                        format!("random_options[{i}].adj"),
                        "random_option_adj",
                        format!("the value is -{OPTION_ADJ_MAX}..={OPTION_ADJ_MAX}, not 0"),
                    );
                    ok = false;
                }
            }
            if spec.safe && spec.random_options.is_empty() {
                sink.err(
                    "safe",
                    "safe_without_lines",
                    "the protected-awakening flag needs at least one option line",
                );
                ok = false;
            }
            if ok {
                for l in &spec.random_options {
                    word.push_line(RandomOptLine {
                        dst: l.dst,
                        adj: l.adj,
                    });
                }
                if spec.safe {
                    word.set_safe_flag(true);
                }
                let egg_c = spec
                    .pet
                    .as_ref()
                    .is_some_and(|p| p.level >= pet_rules::PL_C);
                if random_option_kind(
                    prop.parts,
                    prop.item_kind3,
                    egg_c,
                    env.cfg.only_armor_awakes,
                )
                .is_none()
                {
                    sink.warn(
                        "random_options",
                        "options_ignored",
                        "the game ignores random options on this item while features.only_armor_awakes is on",
                    );
                } else if matches!(
                    random_option_kind(
                        prop.parts,
                        prop.item_kind3,
                        egg_c,
                        env.cfg.only_armor_awakes
                    ),
                    Some(RandomOptKind::Blessing | RandomOptKind::EatPet)
                ) {
                    sink.warn(
                        "random_options",
                        "binds",
                        "blessing and eat-pet lines bind the item to its owner",
                    );
                }
            }
        }
    }
    rec.random_option = word.0;

    // sockets for cards (or the vis of a vis pet)
    let vis_pet = f.vis;
    if !spec.piercing.is_empty() {
        match f.piercing.filter(|_| !vis_pet) {
            None => sink.err(
                "piercing",
                "not_pierceable",
                if vis_pet {
                    "a vis pet takes vis, not cards".to_owned()
                } else {
                    format!("{} has no card sockets", prop.name)
                },
            ),
            Some((max, kind)) => {
                if spec.piercing.len() > usize::from(max) {
                    sink.err(
                        "piercing",
                        "piercing_range",
                        format!("at most {max} sockets"),
                    );
                } else {
                    let mut ok = true;
                    for (i, &card) in spec.piercing.iter().enumerate() {
                        if card != 0 && env.data.item(card).is_none_or(|c| c.item_kind3 != kind) {
                            sink.err(
                                format!("piercing[{i}]"),
                                "piercing_card",
                                format!("item {card} is not a socket card of this item"),
                            );
                            ok = false;
                        }
                    }
                    if ok {
                        rec.piercing_cards = spec.piercing.clone();
                        if env.cfg.no_piercing {
                            sink.warn(
                                "piercing",
                                "no_piercing",
                                "gear sockets have no effect while features.no_piercing is on",
                            );
                        }
                    }
                }
            }
        }
    }

    // ultimate gems
    if !spec.ultimate_gems.is_empty() {
        if !f.ultimate {
            sink.err(
                "ultimate_gems",
                "not_ultimate",
                format!("{} is not an ultimate weapon", prop.name),
            );
        } else {
            let sockets = gem_sockets(i32::from(spec.upgrade)).min(MAX_PIERCING_ULTIMATE);
            if spec.ultimate_gems.len() > sockets {
                sink.err(
                    "ultimate_gems",
                    "gems_range",
                    format!(
                        "a +{} weapon has {sockets} gem sockets (upgrade - 5)",
                        spec.upgrade
                    ),
                );
            } else {
                let abilities = |id: u32| {
                    env.data
                        .ultimate
                        .gem_ability
                        .iter()
                        .any(|d| d.rows.iter().any(|r| r.gem06 == id))
                };
                let mut ok = true;
                for (i, &gem) in spec.ultimate_gems.iter().enumerate() {
                    if gem != 0 && !abilities(gem) {
                        sink.err(
                            format!("ultimate_gems[{i}]"),
                            "gem_unknown",
                            format!("item {gem} is not an ultimate gem ability"),
                        );
                        ok = false;
                    }
                }
                if ok {
                    rec.ultimate_gems = spec.ultimate_gems.clone();
                }
            }
        }
    }
    if f.ultimate
        && rec.upgrade_level > 5
        && rec.ultimate_gems.len() < gem_sockets(rec.upgrade_level)
    {
        // the sockets exist as soon as the weapon is +6 (`UI_ULTIMATE_PIERCING_SIZE upgrade - 5`)
        rec.ultimate_gems
            .resize(gem_sockets(rec.upgrade_level).min(MAX_PIERCING_ULTIMATE), 0);
    }

    // vis
    if !spec.vis.is_empty() {
        if !vis_pet {
            sink.err(
                "vis",
                "not_vis_pet",
                format!("{} is not a vis pet", prop.name),
            );
        } else if spec.vis.len() > MAX_VIS {
            sink.err("vis", "vis_range", format!("at most {MAX_VIS} vis"));
        } else {
            let mut cards = Vec::new();
            let mut keeps = Vec::new();
            let mut ok = true;
            for (i, v) in spec.vis.iter().enumerate() {
                if env
                    .data
                    .item(v.item_id)
                    .is_none_or(|p| p.item_kind3 != IK3_VIS)
                {
                    sink.err(
                        format!("vis[{i}].item_id"),
                        "vis_item",
                        format!("item {} is not a vis", v.item_id),
                    );
                    ok = false;
                    continue;
                }
                match keep_time(
                    env,
                    &format!("vis[{i}].keep_until"),
                    v.keep_until,
                    &mut sink,
                ) {
                    Some(t) => {
                        cards.push(v.item_id);
                        keeps.push(t);
                    }
                    None => ok = false,
                }
            }
            if ok {
                rec.piercing_cards = cards;
                rec.vis_keep_until = keeps;
            }
        }
    }

    // the system pet of an egg
    if let Some(p) = &spec.pet {
        if !f.pet {
            sink.err(
                "pet",
                "not_a_pet",
                format!("{} is not a pet egg", prop.name),
            );
        } else if let Some(pet) = validate_pet(env, p, &mut sink) {
            rec.pet = Some(pet);
        }
    }

    // flags
    if spec.charged {
        if f.charged {
            rec.charged = true;
        } else {
            sink.err(
                "charged",
                "not_chargeable",
                format!("{} is not a cash item", prop.name),
            );
        }
    }
    if let Some(t) = keep_time(env, "keep_until", spec.keep_until, &mut sink) {
        rec.keep_until = t;
    }
    if spec.bound == Some(true) {
        rec.flags |= ITEM_FLAG_BINDS;
    }
    if let Some(text) = &spec.text {
        if text.len() > TEXT_MAX_BYTES {
            sink.err(
                "text",
                "text_length",
                format!("the text is at most {TEXT_MAX_BYTES} bytes"),
            );
        } else if text.chars().any(char::is_control) {
            sink.err("text", "text_chars", "the text has control characters");
        } else {
            rec.text.clone_from(text);
        }
    }
    if let Some(g) = spec.guild_id.filter(|&g| g != 0) {
        if f.guild {
            rec.guild_id = g;
            sink.warn(
                "guild_id",
                "cloak_stripped",
                format!(
                    "the game removes this cloak at login unless the character is in guild {g}"
                ),
            );
        } else {
            sink.err(
                "guild_id",
                "not_a_cloak",
                format!("{} is not a cloak", prop.name),
            );
        }
    }
    // derived: a commercial element stone is never applied (see the module documentation)
    rec.element_stone_id = 0;

    if sink.errors.is_empty() {
        Ok(Normalised {
            record: rec,
            warnings: sink.warnings,
        })
    } else {
        Err(sink.errors)
    }
}

fn validate_pet(env: &ValidateEnv<'_>, p: &PetRecord, sink: &mut Sink) -> Option<PetRecord> {
    let before = sink.errors.len();
    if p.kind > 3 {
        sink.err(
            "pet.kind",
            "pet_kind",
            "the pet kind is 0 (tiger) to 3 (fox)",
        );
    }
    if p.level > pet_rules::PL_S {
        sink.err(
            "pet.level",
            "pet_level",
            "the pet level is 0 (egg) to 5 (S)",
        );
    }
    let level = p.level.min(pet_rules::PL_S);
    if p.exp > pet_rules::max_exp(level) {
        sink.err(
            "pet.exp",
            "pet_exp",
            format!("the experience is at most {}", pet_rules::max_exp(level)),
        );
    }
    let max_energy = env.data.pet.max_energy(level);
    if u32::from(p.energy) > max_energy {
        sink.err(
            "pet.energy",
            "pet_energy",
            format!("the energy is at most {max_energy}"),
        );
    }
    if p.life > pet_rules::MAX_PET_LIFE {
        sink.err(
            "pet.life",
            "pet_life",
            format!("the life is at most {}", pet_rules::MAX_PET_LIFE),
        );
    }
    for (i, &a) in p.avail_levels.iter().enumerate() {
        if a > 9 {
            sink.err(
                format!("pet.avail_levels[{i}]"),
                "pet_avail",
                "a stat roll is 0..=9",
            );
        } else if a != 0 && i >= usize::from(level) {
            sink.err(
                format!("pet.avail_levels[{i}]"),
                "pet_avail_level",
                "a roll above the pet's level does not exist yet",
            );
        }
    }
    if p.name.len() > PET_NAME_MAX_BYTES {
        sink.err(
            "pet.name",
            "pet_name",
            format!("the name is at most {PET_NAME_MAX_BYTES} bytes"),
        );
    } else if p.name.chars().any(char::is_control) {
        sink.err("pet.name", "pet_name", "the name has control characters");
    }
    (sink.errors.len() == before).then(|| p.clone())
}
