//! Item rules shared by the server and the browser (Plan/19 §22; the module is doc 19's, the crate doc
//! 16's): the packed random-option word, prices, the trade predicate and the upgrade odds helpers.

pub mod angel;
pub mod buffitem;
pub mod pet;
pub mod piercing;
pub mod price;
pub mod random_option;
pub mod refine;
pub mod smelt;
pub mod tooltip;
pub mod trade;
pub mod transy;
pub mod ultimate;
pub mod upgrade;

pub use flyff_data::props::item::IP_FLAG_BINDS;
pub use price::{
    BuyPrice, SellPrice, buy_unit_price, chip_cost, get_cost, hair_cost, is_taxable_item,
    mail_storage_fee, quick_sell_unit_price, sell_unit_price,
};
pub use random_option::{LevelDown, RandomOptKind, RandomOptLine, RandomOptWord};
pub use trade::{TradeView, tradeable};

/// `IP_FLAG_UNDESTRUCTABLE` (`ProjectCmn.h:232`): the item cannot be destroyed with `RemoveInvenItem`.
pub const IP_FLAG_UNDESTRUCTABLE: u32 = 0x02;
/// `IP_FLAG_EQUIP_BIND` (`ProjectCmn.h:234`): the item binds to the character when first equipped.
pub const IP_FLAG_EQUIP_BIND: u32 = 0x04;

/// `CItemElem::m_byFlag` bit: expired.
pub const ITEM_FLAG_EXPIRED: u8 = 0x01;
/// `CItemElem::m_byFlag` bit: binds.
pub const ITEM_FLAG_BINDS: u8 = 0x02;
/// `CItemElem::m_byFlag` bit: is being used (unused by the server).
pub const ITEM_FLAG_ISUSING: u8 = 0x04;
