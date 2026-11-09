//! `DialogHost`: the game side of the ScriptLib helper API (Plan/20 §6, §7.3, §7.5).
//!
//! One host serves one invocation: a `ScriptDlg` request (a player talks to an NPC) or one `#auto` tick of an
//! NPC (no player). It collects the [`pb::DialogOp`]s of the response in call order and executes the other
//! helper effects (texts, items, gold, quests, teleports) through the owners' APIs. [`flyff_lua::NpcRuntime::call`]
//! calls [`ScriptHost::call`] for every `d:Name(...)` of the running NPC function; `Run` and the quest flows call
//! back into the runtime (`run_dialog`), nested up to 16 deep.
//!
//! Helper groups and where they live: context and output (this file), player queries and mutations
//! (`helpers.rs`), quest helpers and the quest flows' operations (`quest_ops.rs`), party and guild
//! (`SocialView`, `social.rs`).

use super::NpcService;
use crate::chat::ingress::notice;
use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::mover::Mover;
use crate::quest::effects::{self, EndResult};
use crate::quest::flow::{self, FlowHost, PredicateMode, QuestPredicates};
use crate::quest::{PlayerSubject, QUEST_CREGUILD, QuestBook};
use flyff_common::defs::text::TID_GAME_LACKSPACE;
use flyff_common::{LayerId, ObjId, WorldId};
use flyff_data::GameData;
use flyff_lua::npc::{CtxReq, HelperStatus};
use flyff_lua::{HVal, Helper, HostError, NpcRuntime, NpcScriptId, ScriptHost};
use flyff_proto::pb;
use flyff_proto::pb::dialog_op::Op;
use std::sync::Arc;

/// The per-invocation `GetVal()` / `GetQuestId()` pair (`nGlobal1` / `nGlobal2`; nested runs pass their own).
#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    value: i64,
    quest: i64,
}

/// What a finished invocation produced.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The dialog operations in call order.
    pub ops: Vec<pb::DialogOp>,
}

/// The helper host of one invocation (see the module documentation).
pub struct DialogHost<'a, 'b> {
    pub(super) svc: &'a mut Services,
    pub(super) ctx: &'a mut GameCtx<'b>,
    pub(super) rt: NpcRuntime,
    /// The player; `None` in the `#auto` context.
    pub(super) who: Option<PlayerRef>,
    pub(super) npc: ObjId,
    pub(super) world: WorldId,
    pub(super) layer: LayerId,
    /// The character key of the NPC, lower case.
    pub(super) npc_key: String,
    /// The character key of the NPC as stored (`m_szCharacterKey`, case kept): the dialog-condition quests compare
    /// it with `strcmp` (`AddCondKey`, WP-NPC-15).
    pub(super) npc_key_exact: String,
    pub(super) script: NpcScriptId,
    pub(super) script_name: String,
    frames: Vec<Frame>,
    pub(super) ops: Vec<pb::DialogOp>,
    pub(super) preds: Arc<dyn QuestPredicates>,
    pub(super) data: Arc<GameData>,
}

impl<'a, 'b> DialogHost<'a, 'b> {
    /// A host for `npc` running `script`; `who` is the talking player (`None` for `#auto`). `None` when the NPC
    /// is not a live NPC mover.
    pub fn new(
        svc: &'a mut Services,
        ctx: &'a mut GameCtx<'b>,
        rt: NpcRuntime,
        who: Option<PlayerRef>,
        npc: ObjId,
        script: NpcScriptId,
    ) -> Option<DialogHost<'a, 'b>> {
        let m = ctx.worlds.mover(npc)?;
        let exact = m.npc()?.character_key.clone();
        let key = exact.to_ascii_lowercase();
        let (world, layer) = (m.core.world, m.core.layer);
        let script_name = rt.script_name(script)?.to_owned();
        let preds = Arc::clone(&svc.npc.predicates);
        let data = Arc::clone(ctx.data);
        Some(DialogHost {
            svc,
            ctx,
            rt,
            who,
            npc,
            world,
            layer,
            npc_key: key,
            npc_key_exact: exact,
            script,
            script_name,
            frames: Vec::new(),
            ops: Vec::new(),
            preds,
            data,
        })
    }

    /// The text of a `WorldDialog.txt` line (`""` out of range).
    pub(super) fn text(&self, id: i64) -> String {
        u32::try_from(id)
            .map(|i| self.data.dialog_text.get(i).to_owned())
            .unwrap_or_default()
    }

    pub(super) fn push(&mut self, op: Op) {
        self.ops.push(pb::DialogOp { op: Some(op) });
    }

    fn frame(&self) -> Frame {
        self.frames.last().copied().unwrap_or_default()
    }

    /// The player mover.
    pub(super) fn player(&self) -> Option<&Mover> {
        self.ctx.player(self.who?)
    }

    /// The operations collected so far, consuming the host.
    pub fn take_outcome(self) -> Outcome {
        Outcome { ops: self.ops }
    }

    /// Takes the operations collected so far, keeping the host.
    pub fn take_ops(&mut self) -> Vec<pb::DialogOp> {
        std::mem::take(&mut self.ops)
    }

    /// `run_dialog(key, value, quest)` (Plan/20 §7.3): the predefined quest keys first, then the script function
    /// of the key.
    pub fn run_dialog(&mut self, key: &str, value: i64, quest: i64) -> Result<(), HostError> {
        self.frames.push(Frame { value, quest });
        let r = self.run_dialog_inner(key, value, quest);
        self.frames.pop();
        r
    }

    fn run_dialog_inner(&mut self, key: &str, value: i64, quest: i64) -> Result<(), HostError> {
        let q = u32::try_from(quest).unwrap_or(0);
        match key {
            "QUEST_END" => return flow::quest_end(self, q, false),
            "QUEST_BEGIN" => return flow::quest_begin(self, q),
            "QUEST_BEGIN_YES" => return flow::quest_begin_yes(self, q),
            "QUEST_BEGIN_NO" => return flow::quest_begin_no(self, q),
            "QUEST_END_COMPLETE" => return flow::quest_end_complete(self, value, q),
            "QUEST_END_FAIL" => return flow::quest_end(self, q, true),
            // registered but unhandled in the original: falls through to the script lookup (no function)
            _ => {}
        }
        let rt = self.rt.clone();
        let Some(line) = rt.dialog_keys().line_of(key) else {
            return Ok(());
        };
        if !rt.has_key(self.script, line) {
            return Ok(());
        }
        let script = self.script;
        rt.call(script, line, self).map_err(HostError::from)
    }

    /// `has_key(script, keyText)` (Plan/20 §6.4): the key is a dialog key and the script has a function for it.
    fn has_key(&self, key: &str) -> bool {
        self.rt
            .dialog_keys()
            .line_of(key)
            .is_some_and(|l| self.rt.has_key(self.script, l))
    }

    /// Notes a refused key once per script and key (the original wrote `Error("key error ...")`).
    fn refused(&mut self, helper: &str, key: &str) {
        let token = (self.script_name.clone(), key.to_owned());
        if self.svc.npc.refused_logged.insert(token) {
            tracing::debug!(
                target: "lua.npc",
                script = %self.script_name,
                helper,
                key,
                "key refused: the script has no function for it"
            );
        }
    }

    // ---- the helper bodies of the dialog output group (§6.4) ---------------------------------------------------

    fn add_key(&mut self, args: &[HVal]) {
        let word = self.text(int(args, 0));
        let key = if args.len() >= 2 {
            let k = self.text(int(args, 1));
            if k.is_empty() { word.clone() } else { k }
        } else {
            word.clone()
        };
        if !self.has_key(&key) {
            self.refused("AddKey", &key);
            return;
        }
        // the third argument (param) is forced to 0 by the original
        self.push(Op::AddKey(pb::DialogKey {
            word,
            key,
            param: 0,
            quest_id: 0,
        }));
    }

    fn add_cond_key(&mut self, args: &[HVal]) -> i64 {
        let word = self.text(int(args, 0));
        let k = self.text(int(args, 1));
        let key = if k.is_empty() { word.clone() } else { k };
        if !self.has_key(&key) {
            self.refused("AddCondKey", &key);
            return 1; // the original reports "added" for a refused key
        }
        // the first active quest with `SetEndCondDialog(npc, key)`: both strings byte for byte (`strcmp`), so the
        // NPC key is the one as stored (FIX: the lower-cased key could never match `MaFl_Dior`)
        let preds = Arc::clone(&self.preds);
        let npc_key = self.npc_key_exact.clone();
        let matched = FlowHost::book(self).and_then(|b| preds.cond_dialog_quest(b, &npc_key, &key));
        match matched {
            Some(_) => {
                self.push(Op::AddKey(pb::DialogKey {
                    word,
                    key,
                    param: 0,
                    quest_id: 0,
                }));
                1
            }
            None => 0,
        }
    }

    fn add_answer(&mut self, args: &[HVal]) {
        let word = self.text(int(args, 0));
        let (key, param) = if args.len() >= 3 {
            let k = self.text(int(args, 1));
            (if k.is_empty() { word.clone() } else { k }, int(args, 2))
        } else {
            (word.clone(), 0)
        };
        if !self.has_key(&key) {
            self.refused("AddAnswer", &key);
            return;
        }
        self.push(Op::AddAnswer(pb::DialogKey {
            word,
            key,
            param: u32::try_from(param).unwrap_or(0),
            quest_id: 0,
        }));
    }

    fn add_quest_key(&mut self, args: &[HVal]) {
        let q = u32::try_from(int(args, 0)).unwrap_or(0);
        let k = self.text(int(args, 1));
        let title = self.preds.title(q);
        if !self.preds.exists(q) {
            return; // no quest record: no title to show
        }
        let key = if k.is_empty() { title.clone() } else { k };
        if !self.has_key(&key) {
            self.refused("AddQuestKey", &key);
            return;
        }
        self.push(Op::CurrQuest(pb::DialogKey {
            word: title,
            key,
            param: 0,
            quest_id: q,
        }));
    }

    fn remove_key(&mut self, args: &[HVal]) {
        let key = self.text(int(args, 0));
        if self.has_key(&key) {
            self.push(Op::RemoveKey(pb::DialogText {
                text: key,
                quest_id: 0,
            }));
        }
    }

    fn speak(&mut self, args: &[HVal]) {
        let Ok(obj) = u32::try_from(int(args, 0)) else {
            return;
        };
        let speaker = ObjId(obj);
        if self.ctx.worlds.mover(speaker).is_none() {
            return;
        }
        let text = self.text(int(args, 1));
        crate::chat::route::npc_say(self.svc, self.ctx, speaker, &text);
    }

    fn print_system_message(&mut self, args: &[HVal]) {
        if let (Some(who), Ok(tid)) = (self.who, u32::try_from(int(args, 0))) {
            notice::defined(self.ctx, who, tid, Vec::new());
        }
    }

    fn play_sound(&mut self, args: &[HVal]) {
        let file = self.text(int(args, 0));
        if let Some(who) = self.who {
            crate::chat::send_to(
                self.ctx,
                who,
                pb::PlaySound {
                    sound: Some(pb::play_sound::Sound::File(file)),
                },
            );
        }
    }

    fn query_set_player_name(&mut self) -> i64 {
        let allowed = self.player().is_some_and(|m| {
            m.player().is_some_and(|p| p.ident.authority.is_gm())
                || m.mv.mode & flyff_common::gm::ModeFlags::QUERYSETPLAYERNAME.0 != 0
        });
        if allowed {
            self.push(Op::QuerySetPlayerName(pb::DialogEmpty {}));
            1
        } else {
            0
        }
    }

    fn get_local_event_state(&self, id: i64) -> i64 {
        let Ok(id) = u16::try_from(id) else { return 0 };
        if usize::from(id) >= flyff_common::config::MAX_EVENT {
            return 0;
        }
        if let Some(v) = self.ctx.live.event_flags.get(&id) {
            return i64::from(*v);
        }
        self.ctx
            .live
            .events
            .initial_flags()
            .iter()
            .find(|(e, _)| e.id() == id)
            .map_or(0, |(_, v)| i64::from(*v))
    }

    fn warn_no_player(&mut self, h: Helper) {
        if self
            .svc
            .npc
            .no_player_logged
            .insert(self.script_name.clone())
        {
            tracing::warn!(
                target: "lua.npc",
                script = %self.script_name,
                helper = h.name(),
                "a helper that needs a player was called from #auto: it does nothing and returns 0"
            );
        }
    }
}

/// Argument `i` as an integer (the binding converted it per `ArgKind`).
pub(super) fn int(args: &[HVal], i: usize) -> i64 {
    match args.get(i) {
        Some(HVal::Int(v)) => *v,
        Some(HVal::Float(f)) => *f as i64,
        Some(HVal::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// Argument `i` as a float.
pub(super) fn float(args: &[HVal], i: usize) -> f32 {
    match args.get(i) {
        Some(HVal::Float(f)) => *f as f32,
        Some(HVal::Int(v)) => *v as f32,
        _ => 0.0,
    }
}

impl ScriptHost for DialogHost<'_, '_> {
    fn call(&mut self, h: Helper, args: &[HVal]) -> Result<HVal, HostError> {
        let sig = h.sig();
        if sig.ctx == CtxReq::Player && self.who.is_none() {
            self.warn_no_player(h);
            return Ok(if sig.returns_value {
                HVal::Int(0)
            } else {
                HVal::Nil
            });
        }
        let npc_id = i64::from(self.npc.0);
        let player_id = self.who.map_or(npc_id, |w| i64::from(w.0.0));
        use Helper as H;
        let r: i64 = match h {
            // ---- context and environment (6.3)
            H::GetVal => self.frame().value,
            H::GetQuestId => self.frame().quest,
            H::GetSrcId | H::PcId | H::Src | H::PC => player_id,
            H::GetDstId | H::NpcId | H::Dst | H::NPC => npc_id,
            H::GetLang => 1,
            H::GetParam1 | H::GetParam2 | H::GetParam3 | H::GetParam4 => 0,
            H::Random => {
                let n = int(args, 0);
                if n > 0 {
                    let n = u32::try_from(n).unwrap_or(u32::MAX);
                    use flyff_common::rng::GameRng;
                    i64::from(self.ctx.rng.world(self.world).random_below(n))
                } else {
                    0
                }
            }
            H::GetDay => i64::from(self.ctx.worlds.game_time.day),
            H::GetHour => i64::from(self.ctx.worlds.game_time.hour),
            H::GetMin => i64::from(self.ctx.worlds.game_time.min),
            H::GetLocalEventState => self.get_local_event_state(int(args, 0)),
            H::IsWormonServer => i64::from(
                self.ctx
                    .live
                    .event_state(flyff_common::config::EventId::Wormon),
            ),
            H::Trace => {
                let text = self.text(int(args, 0));
                tracing::debug!(target: "lua.npc", script = %self.script_name, "{text}");
                0
            }
            H::Run => {
                let key = self.text(int(args, 0));
                self.run_dialog(&key, int(args, 1), 0)?;
                0
            }
            // ---- dialog output (6.4)
            H::Say | H::AddSay => {
                let text = self.text(int(args, 0));
                self.push(Op::Say(pb::DialogText { text, quest_id: 0 }));
                0
            }
            H::Speak => {
                self.speak(args);
                0
            }
            H::AddKey => {
                self.add_key(args);
                0
            }
            H::AddCondKey => self.add_cond_key(args),
            H::AddAnswer => {
                self.add_answer(args);
                0
            }
            H::AddQuestKey => {
                self.add_quest_key(args);
                0
            }
            H::RemoveKey => {
                self.remove_key(args);
                0
            }
            H::RemoveAllKey => {
                self.push(Op::RemoveAllKey(pb::DialogEmpty {}));
                0
            }
            H::SayQuest => {
                let q = u32::try_from(int(args, 0)).unwrap_or(0);
                let slot = usize::try_from(int(args, 1)).unwrap_or(usize::MAX);
                FlowHost::say_quest(self, q, slot);
                0
            }
            H::Exit => {
                self.push(Op::Exit(pb::DialogEmpty {}));
                0
            }
            H::PrintSystemMessage => {
                self.print_system_message(args);
                0
            }
            H::PlaySound => {
                self.play_sound(args);
                0
            }
            H::QuerySetPlayerName => self.query_set_player_name(),
            // ---- quest helpers (6.5), player helpers (6.6), party and guild (6.7)
            H::LaunchQuest
            | H::GetQuestState
            | H::IsSetQuest
            | H::SetQuest
            | H::SetQuestState
            | H::BeginQuest
            | H::EndQuest
            | H::RemoveQuest
            | H::IsGuildQuest
            | H::GetGuildQuestState
            | H::MonHuntStart
            | H::MonHuntStartParty
            | H::DropQuestItem
            | H::ExpQuestEffect => self.quest_helper(h, args)?,
            H::GetPlayerLvl
            | H::GetPlayerJob
            | H::GetPlayerSex
            | H::GetPlayerGold
            | H::GetPlayerExpPercent
            | H::GetEmptyInventoryNum
            | H::GetItemNum
            | H::CreateItem
            | H::RemoveItem
            | H::RemoveAllItem
            | H::EquipItem
            | H::AddGold
            | H::RemoveGold
            | H::AddExp
            | H::InitStat
            | H::InitStr
            | H::InitSta
            | H::InitDex
            | H::InitInt
            | H::ChangeJob
            | H::SetLevel
            | H::AddGPPoint
            | H::Replace
            | H::ReplaceKey => self.player_helper(h, args),
            H::IsParty
            | H::IsPartyMaster
            | H::GetPartyNum
            | H::GetPartyLevel
            | H::IsPartyGuild
            | H::IsGuild
            | H::IsGuildMaster
            | H::CreateGuild
            | H::DestroyGuild => self.social_helper(h),
            // ---- no-ops and legacy stubs (6.8)
            H::SetScriptTimer | H::SayName | H::SpeakName | H::EndSay | H::SetMark | H::GoMark => 0,
            H::IsPlayerSkillPoint => 0,
            H::GetGValue_
            | H::SetGValue
            | H::AddGValue
            | H::CreateMover
            | H::CreateChar
            | H::RemoveMover
            | H::SetTimer
            | H::IsTimeOut
            | H::RandomIndex => {
                debug_assert_eq!(sig.status, HelperStatus::Stub);
                1
            }
        };
        Ok(if sig.returns_value {
            HVal::Int(r)
        } else {
            HVal::Nil
        })
    }
}

impl FlowHost for DialogHost<'_, '_> {
    fn npc_key(&self) -> String {
        self.npc_key.clone()
    }

    fn predicates(&self) -> Arc<dyn QuestPredicates> {
        Arc::clone(&self.preds)
    }

    fn book(&mut self) -> Option<&mut QuestBook> {
        let who = self.who?;
        let m = self.ctx.player_mut(who)?;
        m.player_mut().map(|p| &mut p.quests)
    }

    fn can_begin(&mut self, q: u32, mode: PredicateMode) -> bool {
        let Some(who) = self.who else { return false };
        let check = {
            let Some(s) = PlayerSubject::new(&*self.svc, &*self.ctx, who.0) else {
                return false;
            };
            self.preds.can_begin(&s, q, mode)
        };
        if check.lack_space {
            notice::defined(self.ctx, who, TID_GAME_LACKSPACE, Vec::new());
        }
        check.pass
    }

    fn is_next_level(&mut self, q: u32) -> bool {
        let Some(who) = self.who else { return false };
        PlayerSubject::new(&*self.svc, &*self.ctx, who.0)
            .is_some_and(|s| self.preds.is_next_level(&s, q))
    }

    fn can_end(&mut self, q: u32) -> bool {
        let Some(who) = self.who else { return false };
        let ok = PlayerSubject::new(&*self.svc, &*self.ctx, who.0)
            .is_some_and(|s| self.preds.can_end(&s, q));
        if ok && let Some(book) = self.book() {
            // KEEP: the freeze of a running timer, also while a list is built and when the completion then fails
            book.freeze_timer(q);
        }
        ok
    }

    fn remove_all_key(&mut self) {
        self.push(Op::RemoveAllKey(pb::DialogEmpty {}));
    }

    fn run_key(&mut self, key: &str, value: i64, quest: i64) -> Result<(), HostError> {
        self.run_dialog(key, value, quest)
    }

    fn say_quest(&mut self, quest: u32, slot: usize) {
        if let Some(text) = self.preds.dialog_line(quest, slot)
            && !text.is_empty()
        {
            self.push(Op::Say(pb::DialogText {
                text,
                quest_id: quest,
            }));
        }
    }

    fn add_answer(&mut self, word: &str, key: &str, param: u32, quest: u32) {
        self.push(Op::AddAnswer(pb::DialogKey {
            word: word.to_owned(),
            key: key.to_owned(),
            param,
            quest_id: quest,
        }));
    }

    fn new_quest(&mut self, title: &str, key: &str, param: u32, quest: u32) {
        self.push(Op::NewQuest(pb::DialogKey {
            word: title.to_owned(),
            key: key.to_owned(),
            param,
            quest_id: quest,
        }));
    }

    fn curr_quest(&mut self, title: &str, key: &str, param: u32, quest: u32) {
        self.push(Op::CurrQuest(pb::DialogKey {
            word: title.to_owned(),
            key: key.to_owned(),
            param,
            quest_id: quest,
        }));
    }

    fn run_quest(&mut self, q: u32) {
        if let Some(who) = self.who {
            effects::run_quest(self.svc, self.ctx, who, self.npc, q);
        }
    }

    fn end_quest(&mut self, q: u32, check_cond: bool) -> bool {
        let Some(who) = self.who else { return false };
        match effects::end_quest(self.svc, self.ctx, who, q, check_cond) {
            EndResult::Done => true,
            EndResult::NoSpace => {
                // the full bag: the text was sent, the dialog closes
                self.push(Op::Exit(pb::DialogEmpty {}));
                false
            }
            EndResult::NotDone | EndResult::NoQuest => false,
        }
    }
}

/// `true` when the PL-149 shim applies: `QUEST_CREGUILD` with `guild.create_requires_quest = false`.
pub(super) fn cre_guild_shim(svc: &NpcService, q: i64) -> bool {
    q == i64::from(QUEST_CREGUILD) && !svc.guild_create_requires_quest
}
