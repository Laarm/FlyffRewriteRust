//! Event tables (Plan/14 §6.14): the generic events of `propEvent.inc` ([`generic_event`]), the Christmas drop
//! weights ([`spevent`]), the season table ([`environment`]), the random-event monsters ([`rangda`]), the pet egg
//! transformations ([`transform_item`]), the mini-game scripts ([`minigame`]), `MonsterBuffs.inc`
//! ([`monster_buffs`], L67) and `DPSProp.txt` ([`dps`], L68). The event runtime is doc 22's.

pub mod dps;
pub mod environment;
pub mod generic_event;
pub mod minigame;
pub mod monster_buffs;
pub mod rangda;
pub mod spevent;
pub mod transform_item;
