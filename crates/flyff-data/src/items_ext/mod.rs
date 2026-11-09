//! Item extension tables (Plan/14 §6.12-6.13): `except.txt` ([`except`]), the item rows of `propItemEtc.inc`
//! ([`item_etc`]), gift boxes and packs ([`giftbox`], [`pack_item`]), the pet, collecting, accessory,
//! random-option, ticket, ultimate-weapon and exchange tables ([`pet`], [`collecting`], [`accessory`],
//! [`random_option`], [`ticket`], [`ultimate`], [`exchange`]) and the server probabilities of `s.txt`
//! ([`server_script`], F10). Use rules and draw algorithms are doc 19's.

pub mod accessory;
pub mod collecting;
pub mod except;
pub mod exchange;
pub mod giftbox;
pub mod item_etc;
pub mod pack_item;
pub mod pet;
pub mod random_option;
pub mod server_script;
pub mod ticket;
pub mod ultimate;
