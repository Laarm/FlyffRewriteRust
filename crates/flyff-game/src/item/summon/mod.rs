//! Angel creation and the monster-summon item (Plan/19 §5.10, §5.12, WP-ITEM-22).
//!
//! | module | content |
//! |---|---|
//! | [`angel`] | `CreateAngel`: up to 20 Orichalcum / moonstone stacks become one angel of a rolled colour (VR-025) |
//! | [`create_monster`] | `CreateMonster{data, pos}`: the inventory half of a monster-summon scroll; every summon rule is `ai::create_monster::try_create` (WP-AI-16) |
//!
//! `AngelBuff` (redeeming a finished angel) is WP-ITEM-20's; the angel EXP is doc 16's.

pub mod angel;
pub mod create_monster;
