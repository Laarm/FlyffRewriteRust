//! The life of one quiz: the open continuation (the questions arrived), the step machine of `CQuiz::Process`
//! (`Quiz.cpp:295-539`) and `CloseQuizEvent` (`:660-677`). Plan/22 §15.3.

use super::zones::{self, Geometry, ZoneKind};
use super::{CHANNEL, QuizPhase, QuizService, REMAIN_QUIZ, RunSettings, import};
use crate::chat;
use crate::ctx::{GameCtx, Services};
use crate::gm::text::OutLine;
use crate::mover::{Mover, npc_parts};
use crate::world::fx::{sfx_near, sfx_world};
use flyff_common::defs::obj::{XI_QUIZCORRECTANSWER, XI_SKILL_MER_ONE_SPECIALHIT02};
use flyff_common::defs::text::{
    TID_GAME_QUIZ_CLOSING, TID_GAME_QUIZ_CORRECT_ANSWER_4C, TID_GAME_QUIZ_CORRECT_ANSWER_OX,
    TID_GAME_QUIZ_DROP_OUT, TID_GAME_QUIZ_END_SOLVE_QUESTION, TID_GAME_QUIZ_QUESTION,
    TID_GAME_QUIZ_TELEPORT_QUIZZONE, TID_GAME_QUIZ_WAIT_QUESTION, TID_GAME_QUIZ_WATCHINGZONE_OPEN,
    TID_GAME_QUIZ_WINNERCOUNT,
};
use flyff_common::defs::world::WI_WORLD_MADRIGAL;
use flyff_common::math::Vec3;
use flyff_common::{GameMs, LayerId, WorldId};
use flyff_db::audit::content as audit;
use flyff_db::repo::meta::counter::NEXT_QUIZ_EVENT_ID;
use flyff_db::{DbOp, QuizQuestionRecord, QuizType};
use flyff_proto::pb;
use std::sync::Arc;

/// A refused open (PD-12 or a read error): the line goes to the GM if he is still online and is kept for
/// `gm::state_line` (a console issuer has no session). State unchanged.
pub(super) fn refuse(s: &mut Services, ctx: &mut GameCtx<'_>, line: String) {
    tracing::warn!("{line}");
    let by = s.event.quiz.opened_by;
    s.event.quiz.last_refusal = Some(line.clone());
    if let Some(by) = by
        && ctx.worlds.player(by.0).is_some()
    {
        chat::send_to(ctx, by, OutLine::Text(line).into_message());
    }
}

/// The questions of the type arrived (`OpenQuizEvent` + `SetNPC`): order them never-asked first and oldest first,
/// spawn the host NPCs, start the entrance time and audit `quiz.open`.
pub(super) fn begin(
    s: &mut Services,
    ctx: &mut GameCtx<'_>,
    quiz_type: QuizType,
    rows: Vec<QuizQuestionRecord>,
) {
    let data = Arc::clone(ctx.data);
    let Some(cfg) = data.lua.quiz.as_ref() else {
        refuse(s, ctx, "Quiz: QuizEvent.lua is not loaded".to_owned());
        return;
    };
    let mut rows: Vec<QuizQuestionRecord> = rows
        .into_iter()
        .filter(|q| {
            let fits = q.quiz_type == quiz_type && q.validate().is_ok();
            if !fits {
                tracing::warn!(
                    id = q.id,
                    "quiz: a stored question does not fit the type and is skipped"
                );
            }
            fits
        })
        .collect();
    if rows.is_empty() {
        let label = import::type_label(quiz_type).to_ascii_uppercase();
        refuse(
            s,
            ctx,
            format!(
                "Quiz: no enabled {label} questions in quiz_questions; import them with `flyff quiz import`"
            ),
        );
        return;
    }
    // DECISION (FIX): questions rotate, never-asked first (by id), then the oldest `used_at`
    rows.sort_by_key(|q| (q.used_at.is_some(), q.used_at.map(|t| t.0), q.id));

    let mut npcs = Vec::new();
    let madrigal = WorldId(WI_WORLD_MADRIGAL);
    for n in &cfg.npcs {
        let (Some(prop), Some(def)) = (data.mover(n.mover), data.npc(&n.char_key)) else {
            tracing::warn!(
                key = %n.char_key,
                "quiz: the host NPC is not in the game data: the quiz opens without it"
            );
            continue;
        };
        let pos = Vec3::new(n.pos[0], n.pos[1], n.pos[2]);
        let (core, sim) = npc_parts(ctx.data, prop, pos, 0.0);
        let mover = Mover::new_npc(
            core,
            sim,
            prop,
            Some(def),
            ctx.data,
            ctx.rng.world(madrigal),
        );
        match ctx.worlds.spawn_mover(madrigal, LayerId(0), mover) {
            Ok(id) => npcs.push(id),
            Err(e) => tracing::warn!(error = %e, "quiz: the host NPC could not be placed"),
        }
    }

    let settings = RunSettings {
        auto: cfg.auto_mode,
        quiz_type,
        entrance_ms: u64::from(cfg.entrance_ms),
        wait_ms: u64::from(cfg.wait_ms),
        question_ms: u64::from(cfg.question_ms),
        watching_ms: u64::from(cfg.watching_zone_open_ms),
        close_wait_ms: u64::from(cfg.close_wait_ms),
        prize_item: cfg.prize_item,
        prize_count: cfg.prize_item_num,
    };
    let now = ctx.clock.game_ms;
    let q = &mut s.event.quiz;
    let event_id = q.next_event_id;
    q.next_event_id = event_id.saturating_add(1);
    q.event_id = event_id;
    // FIX: legacy took MAX of a log table; one id per open (the id only appears in the audit lines)
    ctx.persist.op(DbOp::RaiseCounter {
        key: NEXT_QUIZ_EVENT_ID,
        at_least: i64::from(event_id) + 1,
    });
    q.next = GameMs(now.0 + settings.entrance_ms);
    q.settings = Some(settings);
    q.questions = rows.into();
    q.asked_unmarked.clear();
    q.quiz_count = 0;
    q.npcs = npcs;
    q.running = true;
    q.phase = QuizPhase::Open;
    q.notice_due = None;
    q.last_refusal = None;
    let by = q.opened_by.and_then(|p| {
        ctx.worlds
            .player(p.0)
            .and_then(|m| m.player())
            .map(|p| p.ident.character_id)
    });
    let questions = u32::try_from(q.questions.len()).unwrap_or(u32::MAX);
    ctx.audit
        .emit(audit::quiz_open(event_id, quiz_type, questions, by));
}

// ---- the step machine ----------------------------------------------------------------------------------------------

/// One run of `CQuiz::Process` for a running quiz: the body of the phase if its deadline passed (`now > next`, or
/// `force`: the GM's `SetNextTime(0)` step), `CloseWait` decides at once.
pub(super) fn step(s: &mut Services, ctx: &mut GameCtx<'_>, force: bool) {
    let (running, phase, next) = {
        let q = &s.event.quiz;
        (q.running, q.phase, q.next)
    };
    if !running {
        return;
    }
    let due = force || ctx.clock.game_ms > next;
    match phase {
        QuizPhase::Close if due => close(s, ctx),
        QuizPhase::Open if due => entrance_over(s, ctx),
        QuizPhase::Question if due => ask(s, ctx),
        QuizPhase::CorrectAnswer if due => reveal(s, ctx),
        QuizPhase::DropOut if due => drop_out(s, ctx),
        QuizPhase::CloseWait => close_wait(s, ctx),
        QuizPhase::WatchingZoneOpen if due => watching_over(s, ctx),
        _ => {}
    }
}

fn settings(s: &Services) -> Option<RunSettings> {
    s.event.quiz.settings.clone()
}

fn after(ctx: &GameCtx<'_>, ms: u64) -> GameMs {
    GameMs(ctx.clock.game_ms.0.saturating_add(ms))
}

/// Open, deadline passed: no question or nobody in the quiz world closes it, else the first question follows after the
/// wait.
fn entrance_over(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(cfg) = settings(s) else {
        return close(s, ctx);
    };
    if s.event.quiz.questions.is_empty() {
        tracing::error!("quiz: the question list is empty at the end of the entrance time");
        return close(s, ctx);
    }
    if super::players(ctx).is_empty() {
        return close(s, ctx);
    }
    super::say(ctx, TID_GAME_QUIZ_WAIT_QUESTION, &[], 0);
    let q = &mut s.event.quiz;
    q.phase = QuizPhase::Question;
    q.next = after(ctx, cfg.wait_ms);
}

/// Question, deadline passed: the caption, the question text and the countdown to everybody in the quiz layer.
fn ask(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(cfg) = settings(s) else {
        return close(s, ctx);
    };
    let Some(front) = s.event.quiz.questions.front() else {
        return close(s, ctx);
    };
    let (id, text) = (front.id, import::sent_text(front));
    super::say(ctx, TID_GAME_QUIZ_QUESTION, &[], QuizPhase::Question.code());
    if !text.is_empty() {
        super::to_layer(
            ctx,
            pb::QuizQuestion {
                r#type: i32::from(cfg.quiz_type.code()),
                question: text,
                // the countdown the client shows (`m_nQCount`), not the number of the question
                count: i32::try_from(cfg.question_ms / 1000).unwrap_or(i32::MAX),
            },
        );
    }
    let next = after(ctx, cfg.question_ms);
    let q = &mut s.event.quiz;
    q.phase = QuizPhase::CorrectAnswer;
    q.next = next;
    q.quiz_count += 1;
    q.asked_unmarked.push(id);
}

/// CorrectAnswer, deadline passed: the effect on the right answer, "question n has ended" and the answer.
fn reveal(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(cfg) = settings(s) else {
        return close(s, ctx);
    };
    let Some(answer) = s.event.quiz.questions.front().map(|q| q.answer) else {
        return close(s, ctx);
    };
    let geo = Geometry::of(cfg.quiz_type);
    if let Some(pos) = geo.answer_position(answer) {
        sfx_world(ctx, zones::WORLD, XI_QUIZCORRECTANSWER, pos, false);
    }
    let count = s.event.quiz.quiz_count;
    super::say(
        ctx,
        TID_GAME_QUIZ_END_SOLVE_QUESTION,
        &[count.to_string()],
        QuizPhase::CorrectAnswer.code(),
    );
    match cfg.quiz_type {
        QuizType::Ox => {
            let letter = if answer == 1 { "O" } else { "X" };
            super::say(
                ctx,
                TID_GAME_QUIZ_CORRECT_ANSWER_OX,
                &[letter.to_owned()],
                0,
            );
        }
        QuizType::FourChoice => {
            super::say(
                ctx,
                TID_GAME_QUIZ_CORRECT_ANSWER_4C,
                &[answer.to_string()],
                0,
            );
        }
    }
    let next = after(ctx, cfg.wait_ms);
    let q = &mut s.event.quiz;
    q.phase = QuizPhase::DropOut;
    q.next = next;
}

/// DropOut, deadline passed (`DropOutWrongUser`): "drop out" to everybody, the wrong players inside the quiz rect go
/// to the watch zone, the survivors are counted once (FIX: legacy re-tested every player in its winner loop). More than
/// one survivor and more questions go on; otherwise the winners are paid and the quiz waits to close.
fn drop_out(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(cfg) = settings(s) else {
        return close(s, ctx);
    };
    let Some(answer) = s.event.quiz.questions.front().map(|q| q.answer) else {
        return close(s, ctx);
    };
    let geo = Geometry::of(cfg.quiz_type);
    super::say(ctx, TID_GAME_QUIZ_DROP_OUT, &[], QuizPhase::DropOut.code());
    let mut survivors = Vec::new();
    for obj in super::players(ctx) {
        let Some(pos) = super::pos_of(ctx, obj) else {
            continue;
        };
        if geo.in_answer_rect(pos, answer) {
            survivors.push(obj);
        } else if geo.zone_of(pos) == ZoneKind::Quiz {
            let to = geo.teleport_pos(zones::ZONE_WATCHING, ctx.rng.event());
            if let Some(to) = to {
                super::go(ctx, obj, zones::WORLD, to);
            }
        }
    }
    let left = s.event.quiz.questions.len();
    // the legacy answer logs (`tblQuizUserLog` / `AnswerLog`, `m_bSelectLog`) are dropped: a debug line per round
    tracing::debug!(
        event = s.event.quiz.event_id,
        question_no = s.event.quiz.quiz_count,
        answer,
        survivors = survivors.len(),
        left,
        "quiz: round decided"
    );
    s.event.quiz.questions.pop_front();
    if survivors.len() > 1 && left > 1 {
        let next = after(ctx, cfg.wait_ms);
        let q = &mut s.event.quiz;
        q.phase = QuizPhase::Question;
        q.next = next;
        return;
    }
    super::say(
        ctx,
        TID_GAME_QUIZ_WINNERCOUNT,
        &[survivors.len().to_string()],
        QuizPhase::CloseWait.code(),
    );
    super::prize::award(s, ctx, &survivors);
    // `next` stays: the next run decides
    s.event.quiz.phase = QuizPhase::CloseWait;
}

/// CloseWait (no timer): more than `REMAIN_QUIZ` questions left reopen the watch zone, else the closing notice and
/// the closing wait.
fn close_wait(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(cfg) = settings(s) else {
        return close(s, ctx);
    };
    if s.event.quiz.questions.len() > REMAIN_QUIZ {
        super::say(
            ctx,
            TID_GAME_QUIZ_WATCHINGZONE_OPEN,
            &[],
            QuizPhase::WatchingZoneOpen.code(),
        );
        super::say(ctx, TID_GAME_QUIZ_TELEPORT_QUIZZONE, &[], 0);
        let next = after(ctx, cfg.watching_ms);
        let q = &mut s.event.quiz;
        q.phase = QuizPhase::WatchingZoneOpen;
        q.next = next;
        // KEEP: the numbering restarts, and the database recreated its world data (the first notice comes at once)
        q.quiz_count = 0;
        q.notice_due = None;
    } else {
        super::notice_all(ctx, TID_GAME_QUIZ_CLOSING, true, CHANNEL, 0);
        let next = after(ctx, cfg.close_wait_ms);
        let q = &mut s.event.quiz;
        q.phase = QuizPhase::Close;
        q.next = next;
    }
}

/// WatchingZoneOpen, deadline passed: "wait for the question"; somebody inside the quiz rect continues, else the quiz
/// closes.
fn watching_over(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let Some(cfg) = settings(s) else {
        return close(s, ctx);
    };
    let geo = Geometry::of(cfg.quiz_type);
    super::say(ctx, TID_GAME_QUIZ_WAIT_QUESTION, &[], 0);
    let any = super::players(ctx)
        .into_iter()
        .any(|o| super::pos_of(ctx, o).is_some_and(|p| geo.zone_of(p) == ZoneKind::Quiz));
    if any {
        let next = after(ctx, cfg.wait_ms);
        let q = &mut s.event.quiz;
        q.phase = QuizPhase::Question;
        q.next = next;
    } else {
        close(s, ctx);
    }
}

// ---- close ------------------------------------------------------------------------------------------------------------

/// `CloseQuizEvent`: not running, closed, the list and a pending read gone; every host NPC leaves with its effect;
/// the questions asked since the last payout are marked used; everybody in the quiz world goes to Madrigal.
pub(super) fn close(s: &mut Services, ctx: &mut GameCtx<'_>) {
    let q: &mut QuizService = &mut s.event.quiz;
    q.running = false;
    q.phase = QuizPhase::Close;
    q.next = GameMs(0);
    q.quiz_count = 0;
    q.questions.clear();
    q.loading = None;
    q.notice_due = None;
    let npcs = std::mem::take(&mut q.npcs);
    let unmarked = std::mem::take(&mut q.asked_unmarked);
    for npc in npcs {
        if ctx.worlds.core(npc).is_some_and(|c| !c.deleted) {
            sfx_near(ctx, npc, XI_SKILL_MER_ONE_SPECIALHIT02, Vec3::ZERO, false);
            ctx.worlds.delete(npc);
        }
    }
    if !unmarked.is_empty() {
        ctx.persist.op(DbOp::MarkQuizQuestionsUsed {
            ids: unmarked,
            at: ctx.clock.unix_ms.to_unix_time(),
        });
    }
    for obj in super::players(ctx) {
        super::go_out(ctx, obj);
    }
}
