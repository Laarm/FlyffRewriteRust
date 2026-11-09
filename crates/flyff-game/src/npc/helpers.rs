//! The player queries and mutations of the helper API (Plan/20 §6.6): level, job, gold, EXP percent, inventory
//! queries, item creation and removal, gold, EXP, the stat reset helpers, level and growth points, and the
//! teleport helpers `Replace` / `ReplaceKey`. Everything goes through the owners' APIs (docs 15, 16, 19); this
//! file adds the script-visible quirks (KEEP): `CreateItem` sends its text before it tries to create the item,
//! `RemoveGold` clamps at zero, `RemoveItem` converts the count to a C `short`, `GetItemNum` is 0 while an item is
//! reserved.
//!
//! `ChangeJob` checks the tier gates of 20 §11.1 and then calls doc 16's `change_job` (WP-NPC-10).

use super::host::{DialogHost, float, int};
use crate::chat::ingress::notice;
use crate::ctx::PlayerRef;
use crate::item::{self, Item, ItemOrigin, RemoveReason};
use crate::quest::effects;
use crate::stats::Out;
use crate::stats::points::stats_changed;
use crate::stats::progression::{self, ExpGrant};
use flyff_common::defs::text::{TID_GAME_DROPITEM, TID_GAME_REAPEXP};
use flyff_common::math::Vec3;
use flyff_common::{CharacterId, LayerId, WorldId};
use flyff_db::ItemRecord;
use flyff_lua::{HVal, Helper};
use flyff_proto::pb;
use flyff_proto::pb::dialog_op::Op;
use flyff_rules::consts::{MAX_EXP_LEVEL, MAX_JOB_LEVEL, MAX_LEVEL};
use flyff_rules::jobs::{is_base, is_expert, is_pro};

/// The base value every stat is reset to (`InitStat`).
const BASE_STAT: i32 = 15;

impl DialogHost<'_, '_> {
    /// The helpers of §6.6 (called with a player; the context rule of §6.8 was applied by the caller).
    pub(super) fn player_helper(&mut self, h: Helper, args: &[HVal]) -> i64 {
        let Some(who) = self.who else { return 0 };
        use Helper as H;
        match h {
            H::GetPlayerLvl => self.player().map_or(0, |m| i64::from(m.mv.level)),
            H::GetPlayerJob => self.player().map_or(0, |m| i64::from(m.mv.job)),
            H::GetPlayerSex => self
                .player()
                .and_then(|m| m.player())
                .map_or(0, |p| i64::from(p.appearance.sex)),
            H::GetPlayerGold => self
                .player()
                .and_then(|m| m.player())
                .map_or(0, |p| i64::from(p.items.gold)),
            H::GetPlayerExpPercent => self.exp_percent(),
            H::GetEmptyInventoryNum => self
                .player()
                .and_then(|m| m.player())
                .map_or(0, |p| p.items.inventory.empty_count() as i64),
            H::GetItemNum => {
                let id = u32::try_from(int(args, 0)).unwrap_or(0);
                self.player()
                    .and_then(|m| m.player())
                    .map_or(0, |p| i64::from(p.items.quest_item_count(id)))
            }
            H::CreateItem => {
                self.create_item(who, int(args, 0), int(args, 1));
                0
            }
            H::RemoveItem => self.remove_item(who, int(args, 0), int(args, 1)),
            H::RemoveAllItem => {
                let id = u32::try_from(int(args, 0)).unwrap_or(0);
                item::ops::remove_item_by_id(
                    self.svc,
                    self.ctx,
                    who,
                    id,
                    None,
                    RemoveReason::Other("script"),
                );
                0
            }
            H::EquipItem => {
                self.equip_item(who, int(args, 0));
                0
            }
            H::AddGold => {
                self.add_gold(who, int(args, 0));
                0
            }
            H::RemoveGold => {
                self.remove_gold(who, int(args, 0));
                0
            }
            H::AddExp => {
                self.add_exp(who, int(args, 0));
                0
            }
            H::InitStat => {
                self.init_stat(who);
                0
            }
            H::InitStr | H::InitSta | H::InitDex | H::InitInt => {
                self.init_one_stat(who, h);
                0
            }
            H::ChangeJob => {
                self.change_job(who, int(args, 0));
                0
            }
            H::SetLevel => {
                self.set_level(who, int(args, 0));
                1
            }
            H::AddGPPoint => {
                self.add_gp(who, int(args, 0));
                1
            }
            H::Replace => {
                self.replace(who, int(args, 0), args);
                0
            }
            H::ReplaceKey => {
                self.replace_key(who, int(args, 0), int(args, 1));
                0
            }
            _ => 0,
        }
    }

    fn exp_percent(&mut self) -> i64 {
        let core = self.svc.stats.handle();
        let Some(m) = self.player() else { return 0 };
        let exp = m.player().map_or(0, |p| p.progression.exp);
        i64::from(flyff_rules::progression::exp_percent_x100(
            &core.rules,
            m.mv.level,
            exp,
        ))
    }

    /// `__CreateItem` (Plan/20 §6.6 #59): `effects::create_quest_item` with ability option 0 and no flag.
    fn create_item(&mut self, who: PlayerRef, item_id: i64, n: i64) {
        let Ok(id) = u32::try_from(item_id) else {
            return;
        };
        effects::create_quest_item(self.svc, self.ctx, who, id, n, 0, 0);
    }

    /// `RemoveItem(item, n)`: `-1` removes every piece; otherwise `n` converted with `as i16` (C `short`) pieces,
    /// in storage order, reserved or not. Returns the number removed.
    fn remove_item(&mut self, who: PlayerRef, item_id: i64, n: i64) -> i64 {
        let Ok(id) = u32::try_from(item_id) else {
            return 0;
        };
        let want = if n == -1 {
            None
        } else {
            let short = n as i16;
            if short <= 0 {
                return 0;
            }
            Some(u32::from(short.unsigned_abs()))
        };
        i64::from(item::ops::remove_item_by_id(
            self.svc,
            self.ctx,
            who,
            id,
            want,
            RemoveReason::Other("script"),
        ))
    }

    /// A ground item for `who` at `pos`: holder = the player's character, owner = the player.
    fn drop_for(&mut self, who: PlayerRef, item_id: u32, count: u16, pos: Vec3) -> bool {
        let Some(prop) = self.data.item(item_id) else {
            return false;
        };
        let Some(character) = self
            .ctx
            .player(who)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
        else {
            return false;
        };
        let mut record = ItemRecord::new(0, item_id, count, 0);
        record.endurance = prop.endurance as i32;
        let meta = item::GroundItemMeta {
            owner: Some(who.0),
            holder: Some(CharacterId(character.0)),
            drop_time: self.ctx.clock.game_ms,
            from_monster: false,
            event_monster: None,
            gold: None,
            respawned: false,
        };
        self.ctx
            .worlds
            .spawn_ground_item(self.world, self.layer, pos, 0.0, record, meta)
            .is_ok()
    }

    /// `EquipItem(item)`: creates one piece and equips it; a full inventory puts it on the ground at the player's
    /// feet (held for the player) with `TID_GAME_DROPITEM`.
    fn equip_item(&mut self, who: PlayerRef, item_id: i64) {
        let Ok(id) = u32::try_from(item_id) else {
            return;
        };
        let Some(prop) = self.data.item(id) else {
            return;
        };
        let name = prop.name.clone();
        match item::ops::create_item(self.svc, self.ctx, who, Item::new(id, 1), ItemOrigin::Quest) {
            Ok(plan) => {
                if let Some((storage, _)) = plan.touched.first() {
                    item::handlers::equip_toggle(self.svc, self.ctx, who, *storage, 0);
                }
            }
            Err(_) => {
                let pos = self.ctx.player(who).map_or(Vec3::ZERO, |m| m.core.pos);
                if self.drop_for(who, id, 1, pos) {
                    notice::defined(
                        self.ctx,
                        who,
                        TID_GAME_DROPITEM,
                        vec![format!("\"{name}\"")],
                    );
                }
            }
        }
    }

    /// `AddGold(n)`: `effects::add_gold_with_text` (the gold rules of doc 19 plus the gain text).
    fn add_gold(&mut self, who: PlayerRef, n: i64) {
        effects::add_gold_with_text(self.svc, self.ctx, who, n);
    }

    /// `RemoveGold(n)`: `effects::remove_gold_clamped` (with less than `n` the gold becomes 0, KEEP-QUIRK).
    fn remove_gold(&mut self, who: PlayerRef, n: i64) {
        effects::remove_gold_clamped(self.svc, self.ctx, who, n);
    }

    /// `AddExp(n)`: `AddExperience(n, TRUE, FALSE)` of doc 16 (level-ups processed, `SetExperience` to self) and
    /// the text `TID_GAME_REAPEXP`.
    fn add_exp(&mut self, who: PlayerRef, n: i64) {
        if n <= 0 {
            return;
        }
        progression::grant_experience(
            self.svc,
            self.ctx,
            who,
            ExpGrant::reward(n, false),
            Out::new(),
        );
        notice::defined(self.ctx, who, TID_GAME_REAPEXP, vec![n.to_string()]);
    }

    /// `ChangeJob(job)` (`ScriptLib.cpp:868`, Plan/20 §11.1): refused with a warning (the original's `Error()`) for a
    /// vagrant whose level is not 15, an expert below level 60, a pro below level 120 whose EXP bar is below 99.99 %;
    /// masters and heroes are not checked. Otherwise doc 16's `change_job`: the skills of the job's tier, the
    /// `JobChanged` messages to the owner and the observers, the directory, the audit line - no level or stat
    /// change.
    fn change_job(&mut self, who: PlayerRef, job: i64) {
        let core = self.svc.stats.handle();
        let (level, current) = match self.player() {
            Some(m) => (m.mv.level, m.mv.job),
            None => return,
        };
        let percent = self.exp_percent();
        let refused = (is_base(&core.rules, current) && level != MAX_JOB_LEVEL)
            || (is_expert(&core.rules, current) && level < MAX_JOB_LEVEL + MAX_EXP_LEVEL)
            || (is_pro(&core.rules, current) && level < MAX_LEVEL && percent < 9999);
        let ok = !refused
            && i32::try_from(job)
                .is_ok_and(|j| crate::stats::jobs::change_job(self.svc, self.ctx, who, j));
        if !ok {
            tracing::warn!(
                target: "lua.npc",
                script = %self.script_name,
                job,
                level,
                current,
                "ChangeJob refused"
            );
        }
    }

    /// `InitStat`: STR = STA = DEX = INT = 15 and `remain_gp = (level - 1) * 2` — exactly this formula, not
    /// doc 16's level-up table (KEEP) —, the op `init_stat{remain_gp}` and the derived values recomputed.
    fn init_stat(&mut self, who: PlayerRef) {
        let core = self.svc.stats.handle();
        let Some(m) = self.ctx.worlds.player_mut(who.0) else {
            return;
        };
        m.mv.base.str = BASE_STAT;
        m.mv.base.sta = BASE_STAT;
        m.mv.base.dex = BASE_STAT;
        m.mv.base.int = BASE_STAT;
        let gp = (m.mv.level - 1).wrapping_mul(2);
        if let Some(p) = m.player_mut() {
            p.progression.stat_points = gp;
        }
        stats_changed(m, &core);
        self.push(Op::InitStat(pb::DialogValue {
            value: u32::try_from(gp).unwrap_or(0),
        }));
    }

    /// `InitStr/Sta/Dex/Int`: a stat above 15 goes back to 15 and the difference becomes points; the op is sent
    /// even when nothing changed.
    fn init_one_stat(&mut self, who: PlayerRef, h: Helper) {
        let core = self.svc.stats.handle();
        let Some(m) = self.ctx.worlds.player_mut(who.0) else {
            return;
        };
        let slot = match h {
            Helper::InitStr => &mut m.mv.base.str,
            Helper::InitSta => &mut m.mv.base.sta,
            Helper::InitDex => &mut m.mv.base.dex,
            _ => &mut m.mv.base.int,
        };
        let refund = (*slot - BASE_STAT).max(0);
        if refund > 0 {
            *slot = BASE_STAT;
        }
        let gp = match m.player_mut() {
            Some(p) => {
                p.progression.stat_points = p.progression.stat_points.wrapping_add(refund);
                p.progression.stat_points
            }
            None => return,
        };
        if refund > 0 {
            stats_changed(m, &core);
        }
        let value = pb::DialogValue {
            value: u32::try_from(gp).unwrap_or(0),
        };
        self.push(match h {
            Helper::InitStr => Op::InitStr(value),
            Helper::InitSta => Op::InitSta(value),
            Helper::InitDex => Op::InitDex(value),
            _ => Op::InitInt(value),
        });
    }

    /// `SetLevel(n)` of doc 16 plus the player-data update of doc 21.
    fn set_level(&mut self, who: PlayerRef, n: i64) {
        let Ok(level) = i32::try_from(n) else { return };
        let core = self.svc.stats.handle();
        let before = self.player().map_or(0, |m| m.mv.level);
        let mut out = Out::new();
        if let Some(m) = self.ctx.worlds.player_mut(who.0) {
            progression::set_level(m, &core, level, &mut out);
        }
        out.flush(self.ctx, who.0);
        crate::social::hooks::on_level_changed(
            self.svc,
            self.ctx,
            who,
            u32::try_from(before).unwrap_or(0),
            u32::try_from(level).unwrap_or(0),
        );
    }

    /// `AddGPPoint(n)` (negative allowed).
    fn add_gp(&mut self, who: PlayerRef, n: i64) {
        let Ok(n) = i32::try_from(n) else { return };
        let mut out = Out::new();
        if let Some(m) = self.ctx.worlds.player_mut(who.0) {
            progression::add_gp(m, n, &mut out);
        }
        out.flush(self.ctx, who.0);
    }

    /// `Replace(world, x, y, z)`: only for `x > 0 && z > 0`, `exit` first.
    fn replace(&mut self, who: PlayerRef, world: i64, args: &[HVal]) {
        let (x, y, z) = (float(args, 1), float(args, 2), float(args, 3));
        if x > 0.0 && z > 0.0 {
            self.push(Op::Exit(pb::DialogEmpty {}));
            self.teleport(who, world, Vec3::new(x, y, z));
        }
    }

    /// `ReplaceKey(world, key)`: `exit`, then the revival region named by the key.
    fn replace_key(&mut self, who: PlayerRef, world: i64, key_id: i64) {
        self.push(Op::Exit(pb::DialogEmpty {}));
        let key = self.text(key_id);
        let Ok(w) = u32::try_from(world) else { return };
        match self.ctx.worlds.revival.revival_pos(WorldId(w), &key) {
            Some(p) => self.teleport(who, i64::from(p.world.0), p.pos),
            None => tracing::warn!(
                target: "lua.npc",
                script = %self.script_name,
                world,
                key = %key,
                "ReplaceKey: no revival region with that key"
            ),
        }
    }

    fn teleport(&mut self, who: PlayerRef, world: i64, pos: Vec3) {
        let Ok(w) = u32::try_from(world) else { return };
        if let Err(e) = self.ctx.worlds.replace(
            who.0,
            WorldId(w),
            pos,
            LayerId(0),
            crate::world::ReplaceKind::Normal,
        ) {
            tracing::warn!(
                target: "lua.npc",
                script = %self.script_name,
                world,
                error = %e,
                "the script teleport was refused"
            );
        }
    }

    /// `DropQuestItem`'s ground item: one piece near the NPC for the player.
    pub(super) fn drop_quest_item(&mut self, who: PlayerRef, item_id: i64, prob: i64) -> i64 {
        use flyff_common::rng::GameRng;
        let Ok(id) = u32::try_from(item_id) else {
            return 0;
        };
        if self.data.item(id).is_none() {
            return 0;
        }
        let draw = self.ctx.rng.world(self.world).random_below(1000);
        if i64::from(draw) >= prob {
            return 0;
        }
        let npc_pos = self.ctx.worlds.core(self.npc).map_or(Vec3::ZERO, |c| c.pos);
        let (dx, dz) = {
            let rng = self.ctx.rng.world(self.world);
            (rng.random_f32(2.0) - 1.0, rng.random_f32(2.0) - 1.0)
        };
        let pos = Vec3::new(npc_pos.x + dx, npc_pos.y, npc_pos.z + dz);
        i64::from(self.drop_for(who, id, 1, pos))
    }
}
