//! The plug points of the game thread's input dispatch (Plan/10 §4.6, 11 §3.7, WP-OPS-06).
//!
//! `GameInput` handling is a skeleton wired to three seams so the lobby, world and feature tasks plug in
//! without touching the dispatcher:
//!
//! * [`crate::session::SessionRules`] — join, re-attach view, the leave rule, `finish_leave`, character create
//!   and delete (docs 12 and 15),
//! * [`ClientRouter`] — in-world client messages. The default router calls the generated
//!   `flyff_proto::dispatch_client` on a [`ClientCall`]; a feature adds its `on_<message>` override to
//!   `ClientCall` (one method per message, default `unhandled`),
//! * [`AdminHandler`] — every `AdminCmd` the runtime does not execute itself (docs 20, 23).
//!
//! [`Handlers`] bundles the three; `GameInit.handlers` installs a replacement.

use crate::ctx::{GameCtx, PlayerRef, Services};
use crate::input::{AdminActor, AdminCmd, AdminOutput, ClientMsg};
use crate::session::{SessionRules, StubRules};
use flyff_common::SessionId;
use flyff_proto::{ClientHandlers, dispatch_client};

/// Routes one decoded in-world client message of a bound, untainted session. Runs under the session's `Player`
/// panic scope: pushes are staged and dropped on a panic, the session is poisoned.
pub trait ClientRouter: Send + 'static {
    /// Handles `msg` sent by `who` (the avatar of `session`).
    fn route(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        who: PlayerRef,
        session: SessionId,
        msg: ClientMsg,
    );
}

/// The default router: the generated dispatcher over [`ClientCall`].
#[derive(Debug, Default)]
pub struct DefaultRouter;

impl ClientRouter for DefaultRouter {
    fn route(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        who: PlayerRef,
        session: SessionId,
        msg: ClientMsg,
    ) {
        let mut call = ClientCall { svc, ctx, who };
        dispatch_client(&mut call, session, msg);
    }
}

/// The generated `ClientHandlers` bound to the game state of one call. Feature WPs add their
/// `fn on_<message>(&mut self, sid: SessionId, m: pb::<Message>)` overrides here (each forwarding to its module
/// with `self.svc`, `self.ctx` and `self.who`); everything else is counted as unhandled.
pub struct ClientCall<'a, 'b> {
    /// The services.
    pub svc: &'a mut Services,
    /// The game context.
    pub ctx: &'a mut GameCtx<'b>,
    /// The acting avatar.
    pub who: PlayerRef,
}

impl ClientHandlers for ClientCall<'_, '_> {
    type Sid = SessionId;

    // ---- movement (doc 18 §4) ----

    fn on_player_move_state(&mut self, sid: SessionId, m: flyff_proto::pb::PlayerMoveState) {
        crate::movement::player::on_player_move_state(self.ctx, self.who, sid, m);
    }

    fn on_dest_pos_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DestPosReq) {
        crate::movement::player::on_dest_pos_req(self.ctx, self.who, m);
    }

    fn on_set_dest_obj_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SetDestObjReq) {
        crate::movement::player::on_set_dest_obj_req(self.ctx, self.who, m);
    }

    fn on_motion_req(&mut self, sid: SessionId, m: flyff_proto::pb::MotionReq) {
        crate::movement::player::on_motion_req(self.ctx, self.who, sid, m);
    }

    // ---- ctrls (doc 15 §14.3) ----

    fn on_ctrl_cool_time_cancel(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::CtrlCoolTimeCancel,
    ) {
        crate::world::ctrl::ctrl_cool_time_cancel(self.ctx, self.who);
    }

    // ---- combat (doc 17 §3, §9) ----

    fn on_melee_attack_req(&mut self, _sid: SessionId, m: flyff_proto::pb::MeleeAttackReq) {
        crate::combat::request::on_melee_attack(self.svc, self.ctx, self.who, m);
    }

    fn on_magic_attack_req(&mut self, _sid: SessionId, m: flyff_proto::pb::MagicAttackReq) {
        crate::combat::request::on_magic_attack(self.svc, self.ctx, self.who, m);
    }

    fn on_range_attack_req(&mut self, _sid: SessionId, m: flyff_proto::pb::RangeAttackReq) {
        crate::combat::request::on_range_attack(self.svc, self.ctx, self.who, m);
    }

    fn on_set_target_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SetTargetReq) {
        crate::combat::target::on_set_target(self.svc, self.ctx, self.who, m);
    }

    fn on_revival_req(&mut self, _sid: SessionId, _m: flyff_proto::pb::RevivalReq) {
        crate::combat::death::on_revival_req(self.svc, self.ctx, self.who);
    }

    fn on_revival_to_lodestar_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::RevivalToLodestarReq,
    ) {
        crate::combat::death::on_revival_to_lodestar_req(self.svc, self.ctx, self.who);
    }

    // ---- duels (doc 17 §8.6-§8.7) ----

    fn on_duel_request_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DuelRequestReq) {
        crate::combat::duel::on_duel_request(self.svc, self.ctx, self.who, m);
    }

    fn on_duel_yes(&mut self, _sid: SessionId, m: flyff_proto::pb::DuelYes) {
        crate::combat::duel::on_duel_yes(self.svc, self.ctx, self.who, m);
    }

    fn on_duel_no_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DuelNoReq) {
        crate::combat::duel::on_duel_no(self.svc, self.ctx, self.who, m);
    }

    fn on_duel_party_request_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::DuelPartyRequestReq,
    ) {
        crate::combat::duel::on_duel_party_request(self.svc, self.ctx, self.who, m);
    }

    fn on_duel_party_yes(&mut self, _sid: SessionId, m: flyff_proto::pb::DuelPartyYes) {
        crate::combat::duel::on_duel_party_yes(self.svc, self.ctx, self.who, m);
    }

    fn on_duel_party_no_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DuelPartyNoReq) {
        crate::combat::duel::on_duel_party_no(self.svc, self.ctx, self.who, m);
    }

    // ---- skills (doc 17 §6, §9.4) ----

    fn on_use_skill_req(&mut self, _sid: SessionId, m: flyff_proto::pb::UseSkillReq) {
        crate::skill::use_skill::on_use_skill_req(self.svc, self.ctx, self.who, m);
    }

    fn on_do_use_skill_point_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::DoUseSkillPointReq,
    ) {
        crate::skill::points::on_do_use_skill_point(self.svc, self.ctx, self.who, m);
    }

    fn on_legend_skill_up_start(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::LegendSkillUpStart,
    ) {
        crate::skill::points::on_legend_skill_up_start(self.svc, self.ctx, self.who, m);
    }

    fn on_tele_skill(&mut self, _sid: SessionId, m: flyff_proto::pb::TeleSkill) {
        crate::skill::tele::on_tele_skill(self.svc, self.ctx, self.who, m);
    }

    fn on_end_skill_queue_req(&mut self, _sid: SessionId, _m: flyff_proto::pb::EndSkillQueueReq) {
        crate::skill::queue::on_end_skill_queue_req(self.svc, self.ctx, self.who);
    }

    fn on_send_to_server_ap(&mut self, _sid: SessionId, m: flyff_proto::pb::SendToServerAp) {
        crate::skill::queue::on_send_to_server_ap(self.ctx, self.who, m);
    }

    fn on_resurrection_ok(&mut self, _sid: SessionId, _m: flyff_proto::pb::ResurrectionOk) {
        crate::skill::resurrect::on_resurrection_ok(self.svc, self.ctx, self.who);
    }

    fn on_resurrection_cancel(&mut self, _sid: SessionId, _m: flyff_proto::pb::ResurrectionCancel) {
        crate::skill::resurrect::on_resurrection_cancel(self.ctx, self.who);
    }

    // ---- chat (doc 21 §10, doc 23 §6) ----

    fn on_chat_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ChatReq) {
        crate::chat::route::on_chat_req(self.svc, self.ctx, self.who, m);
    }

    // ---- parties (doc 21 §4, WP-SOCIAL-03..07) ----

    fn on_member_request(&mut self, _sid: SessionId, m: flyff_proto::pb::MemberRequest) {
        crate::social::party::handlers::on_member_request(self.svc, self.ctx, self.who, m);
    }

    fn on_member_request_cancel(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::MemberRequestCancel,
    ) {
        crate::social::party::handlers::on_member_request_cancel(self.svc, self.ctx, self.who, m);
    }

    fn on_add_party_member(&mut self, _sid: SessionId, m: flyff_proto::pb::AddPartyMember) {
        crate::social::party::handlers::on_add_party_member(self.svc, self.ctx, self.who, m);
    }

    fn on_remove_party_member(&mut self, _sid: SessionId, m: flyff_proto::pb::RemovePartyMember) {
        crate::social::party::handlers::on_remove_party_member(self.svc, self.ctx, self.who, m);
    }

    fn on_party_change_leader(&mut self, _sid: SessionId, m: flyff_proto::pb::PartyChangeLeader) {
        crate::social::party::handlers::on_party_change_leader(self.svc, self.ctx, self.who, m);
    }

    fn on_change_troup(&mut self, _sid: SessionId, m: flyff_proto::pb::ChangeTroup) {
        crate::social::party::names::on_change_troup(self.svc, self.ctx, self.who, m);
    }

    fn on_chan_party_name(&mut self, _sid: SessionId, m: flyff_proto::pb::ChanPartyName) {
        crate::social::party::names::on_chan_party_name(self.svc, self.ctx, self.who, m);
    }

    fn on_party_change_item_mode_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::PartyChangeItemModeReq,
    ) {
        crate::social::party::names::on_item_mode(self.svc, self.ctx, self.who, m);
    }

    fn on_party_change_exp_mode_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::PartyChangeExpModeReq,
    ) {
        crate::social::party::names::on_exp_mode(self.svc, self.ctx, self.who, m);
    }

    fn on_party_skill_use(&mut self, _sid: SessionId, m: flyff_proto::pb::PartySkillUse) {
        crate::social::party::skills::on_party_skill_use(self.svc, self.ctx, self.who, m);
    }

    fn on_join_party_req(&mut self, _sid: SessionId, m: flyff_proto::pb::JoinPartyReq) {
        crate::social::party::finder::on_join_party_req(self.svc, self.ctx, self.who, m);
    }

    fn on_party_allow_join_req(&mut self, _sid: SessionId, m: flyff_proto::pb::PartyAllowJoinReq) {
        crate::social::party::finder::on_party_allow_join(self.svc, self.ctx, self.who, m);
    }

    fn on_set_navi_point_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SetNaviPointReq) {
        crate::social::party::finder::on_set_navi_point(self.svc, self.ctx, self.who, m);
    }

    fn on_query_player_data_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::QueryPlayerDataReq,
    ) {
        crate::social::player_data::on_query_player_data(self.svc, self.ctx, self.who, m);
    }

    fn on_query_player_data2(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryPlayerData2) {
        crate::social::player_data::on_query_player_data2(self.svc, self.ctx, self.who, m);
    }

    fn on_query_set_player_name_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::QuerySetPlayerNameReq,
    ) {
        crate::social::player_data::on_query_set_player_name(self.svc, self.ctx, self.who, m);
    }

    fn on_cheering(&mut self, _sid: SessionId, m: flyff_proto::pb::Cheering) {
        crate::social::cheer::on_cheering(self.svc, self.ctx, self.who, m);
    }

    fn on_summon_party(&mut self, _sid: SessionId, m: flyff_proto::pb::SummonParty) {
        crate::social::party::summon::on_summon_party(self.svc, self.ctx, self.who, m);
    }

    fn on_summon_party_confirm(&mut self, _sid: SessionId, m: flyff_proto::pb::SummonPartyConfirm) {
        crate::social::party::summon::on_summon_party_confirm(self.svc, self.ctx, self.who, m);
    }

    // ---- messenger (doc 21 §9, WP-SOCIAL-19) ----

    fn on_add_friend_request_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::AddFriendRequestReq,
    ) {
        crate::social::messenger::requests::on_add_friend_request(self.svc, self.ctx, self.who, m);
    }

    fn on_add_friend_name_request_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::AddFriendNameRequestReq,
    ) {
        crate::social::messenger::requests::on_add_friend_name_request(
            self.svc, self.ctx, self.who, m,
        );
    }

    fn on_add_friend_cancel_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::AddFriendCancelReq,
    ) {
        crate::social::messenger::requests::on_add_friend_cancel(self.svc, self.ctx, self.who, m);
    }

    fn on_add_friend_req(&mut self, _sid: SessionId, m: flyff_proto::pb::AddFriendReq) {
        crate::social::messenger::requests::on_add_friend(self.svc, self.ctx, self.who, m);
    }

    fn on_remove_friend_req(&mut self, _sid: SessionId, m: flyff_proto::pb::RemoveFriendReq) {
        crate::social::messenger::requests::on_remove_friend(self.svc, self.ctx, self.who, m);
    }

    fn on_friend_intercept_state_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::FriendInterceptStateReq,
    ) {
        crate::social::messenger::state::on_friend_intercept_state(self.svc, self.ctx, self.who, m);
    }

    fn on_set_friend_state_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SetFriendStateReq) {
        crate::social::messenger::state::on_set_friend_state(self.svc, self.ctx, self.who, m);
    }

    fn on_get_friend_state_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GetFriendStateReq) {
        crate::social::messenger::state::on_get_friend_state(self.svc, self.ctx, self.who, m);
    }

    fn on_summon_friend(&mut self, _sid: SessionId, m: flyff_proto::pb::SummonFriend) {
        crate::social::messenger::summon::on_summon_friend(self.svc, self.ctx, self.who, m);
    }

    fn on_summon_friend_confirm(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::SummonFriendConfirm,
    ) {
        crate::social::messenger::summon::on_summon_friend_confirm(self.svc, self.ctx, self.who, m);
    }

    fn on_summon_friend_cancel(&mut self, _sid: SessionId, m: flyff_proto::pb::SummonFriendCancel) {
        crate::social::messenger::summon::on_summon_friend_cancel(self.svc, self.ctx, self.who, m);
    }

    // ---- block notices and offline notes (doc 21 §9.4, §9.6, WP-SOCIAL-20) ----

    fn on_block_req(&mut self, _sid: SessionId, m: flyff_proto::pb::BlockReq) {
        crate::social::messenger::block::on_block_req(self.svc, self.ctx, self.who, m);
    }

    fn on_send_tag(&mut self, _sid: SessionId, m: flyff_proto::pb::SendTag) {
        crate::social::messenger::notes::on_send_tag(self.svc, self.ctx, self.who, m);
    }

    // ---- guilds (doc 21 §5, WP-SOCIAL-09..12) ----

    fn on_guild_invite_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildInviteReq) {
        crate::social::guild::handlers::on_guild_invite(self.svc, self.ctx, self.who, m);
    }

    fn on_ignore_guild_invite(&mut self, _sid: SessionId, m: flyff_proto::pb::IgnoreGuildInvite) {
        crate::social::guild::handlers::on_ignore_guild_invite(self.svc, self.ctx, self.who, m);
    }

    fn on_add_guild_member_req(&mut self, _sid: SessionId, m: flyff_proto::pb::AddGuildMemberReq) {
        crate::social::guild::handlers::on_add_guild_member(self.svc, self.ctx, self.who, m);
    }

    fn on_remove_guild_member_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::RemoveGuildMemberReq,
    ) {
        crate::social::guild::handlers::on_remove_guild_member(self.svc, self.ctx, self.who, m);
    }

    fn on_destroy_guild_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DestroyGuildReq) {
        crate::social::guild::create::on_destroy_guild(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_member_level_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::GuildMemberLevelReq,
    ) {
        crate::social::guild::handlers::on_guild_member_level(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_class_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildClassReq) {
        crate::social::guild::handlers::on_guild_class(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_nickname_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildNicknameReq) {
        crate::social::guild::handlers::on_guild_nickname(self.svc, self.ctx, self.who, m);
    }

    fn on_chg_master_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ChgMasterReq) {
        crate::social::guild::handlers::on_chg_master(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_authority_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildAuthorityReq) {
        crate::social::guild::handlers::on_guild_authority(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_penya_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildPenyaReq) {
        crate::social::guild::handlers::on_guild_penya(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_set_name_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildSetNameReq) {
        crate::social::guild::create::on_guild_set_name(self.svc, self.ctx, self.who, m);
    }

    fn on_query_set_guild_name(&mut self, _sid: SessionId, m: flyff_proto::pb::QuerySetGuildName) {
        crate::social::guild::create::on_query_set_guild_name(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_logo_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildLogoReq) {
        crate::social::guild::contribution::on_guild_logo(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_contribution_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::GuildContributionReq,
    ) {
        crate::social::guild::contribution::on_guild_contribution(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_notice_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildNoticeReq) {
        crate::social::guild::contribution::on_guild_notice(self.svc, self.ctx, self.who, m);
    }

    // ---- guild bank (doc 21 §5.8, §5.9, WP-SOCIAL-13) ----

    fn on_guild_bank_wnd_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildBankWndReq) {
        crate::social::guild::bank::on_guild_bank_wnd_req(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_bank_wnd_close(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildBankWndClose) {
        crate::social::guild::bank::on_guild_bank_wnd_close(self.svc, self.ctx, self.who, m);
    }

    fn on_put_item_guild_bank_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::PutItemGuildBankReq,
    ) {
        crate::social::guild::bank::on_put_item_guild_bank(self.svc, self.ctx, self.who, m);
    }

    fn on_get_item_guild_bank_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::GetItemGuildBankReq,
    ) {
        crate::social::guild::bank::on_get_item_guild_bank(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_bank_move_item(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildBankMoveItem) {
        crate::social::guild::bank::on_guild_bank_move_item(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_log_view_req(&mut self, _sid: SessionId, m: flyff_proto::pb::GuildLogViewReq) {
        crate::social::guild::bank::on_guild_log_view(self.svc, self.ctx, self.who, m);
    }

    fn on_create_guild_cloak(&mut self, _sid: SessionId, m: flyff_proto::pb::CreateGuildCloak) {
        crate::social::guild::bank::on_create_guild_cloak(self.svc, self.ctx, self.who, m);
    }

    // ---- guild war (doc 21 §6, WP-SOCIAL-15) ----

    fn on_decl_guild_war_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DeclGuildWarReq) {
        crate::social::guild::war::handlers::on_decl_guild_war(self.svc, self.ctx, self.who, m);
    }

    fn on_acpt_guild_war_req(&mut self, _sid: SessionId, m: flyff_proto::pb::AcptGuildWarReq) {
        crate::social::guild::war::handlers::on_acpt_guild_war(self.svc, self.ctx, self.who, m);
    }

    fn on_surrender_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SurrenderReq) {
        crate::social::guild::war::handlers::on_surrender(self.svc, self.ctx, self.who, m);
    }

    fn on_query_truce_req(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryTruceReq) {
        crate::social::guild::war::handlers::on_query_truce(self.svc, self.ctx, self.who, m);
    }

    fn on_acpt_truce(&mut self, _sid: SessionId, m: flyff_proto::pb::AcptTruce) {
        crate::social::guild::war::handlers::on_acpt_truce(self.svc, self.ctx, self.who, m);
    }

    // ---- guild siege (doc 21 §7.3, WP-SOCIAL-16) ----

    fn on_in_guild_combat(&mut self, _sid: SessionId, m: flyff_proto::pb::InGuildCombat) {
        crate::social::siege::bids::on_in_guild_combat(self.svc, self.ctx, self.who, m);
    }

    fn on_out_guild_combat(&mut self, _sid: SessionId, m: flyff_proto::pb::OutGuildCombat) {
        crate::social::siege::bids::on_out_guild_combat(self.svc, self.ctx, self.who, m);
    }

    fn on_request_status(&mut self, _sid: SessionId, m: flyff_proto::pb::RequestStatus) {
        crate::social::siege::bids::on_request_status(self.svc, self.ctx, self.who, m);
    }

    // ---- guild siege fight (doc 21 §7.4-§7.6, WP-SOCIAL-17, -18) ----

    fn on_select_player_guild_combat(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::SelectPlayerGuildCombat,
    ) {
        crate::social::siege::lineup::select_player(self.svc, self.ctx, self.who, m);
    }

    fn on_join_guild_combat(&mut self, _sid: SessionId, _m: flyff_proto::pb::JoinGuildCombat) {
        crate::social::siege::lineup::join_guild_combat(self.svc, self.ctx, self.who);
    }

    fn on_select_map_guild_combat(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::SelectMapGuildCombat,
    ) {
        crate::social::siege::lineup::select_map(self.svc, self.ctx, self.who, m.map);
    }

    fn on_tele_guild_combat(&mut self, _sid: SessionId, _m: flyff_proto::pb::TeleGuildCombat) {
        crate::social::siege::lineup::tele_out(self.svc, self.ctx, self.who);
    }

    fn on_get_penya_guild_guild_combat(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::GetPenyaGuildGuildCombat,
    ) {
        crate::social::siege::result::claim_guild(self.svc, self.ctx, self.who);
    }

    fn on_get_penya_player_guild_combat(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::GetPenyaPlayerGuildCombat,
    ) {
        crate::social::siege::result::claim_player(self.svc, self.ctx, self.who);
    }

    fn on_player_point_guild_combat(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::PlayerPointGuildCombat,
    ) {
        crate::social::siege::result::player_point(self.svc, self.ctx, self.who);
    }

    // ---- guild combat 1:1 (doc 21 §8, WP-SOCIAL-25, -26) ----

    fn on_gc1to1_tender_open_wnd_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1TenderOpenWndReq,
    ) {
        crate::social::gc1to1::tender::on_open_wnd_req(self.svc, self.ctx, self.who);
    }

    fn on_gc1to1_tender_view_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1TenderViewReq,
    ) {
        crate::social::gc1to1::tender::on_view_req(self.svc, self.ctx, self.who);
    }

    fn on_gc1to1_tender(&mut self, _sid: SessionId, m: flyff_proto::pb::Gc1to1Tender) {
        crate::social::gc1to1::tender::on_tender(self.svc, self.ctx, self.who, m.penya);
    }

    fn on_gc1to1_tender_cancel(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1TenderCancel,
    ) {
        crate::social::gc1to1::tender::on_cancel(self.svc, self.ctx, self.who);
    }

    fn on_gc1to1_tender_failed(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1TenderFailed,
    ) {
        crate::social::gc1to1::tender::on_failed(self.svc, self.ctx, self.who);
    }

    fn on_gc1to1_member_line_up_open_wnd_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1MemberLineUpOpenWndReq,
    ) {
        crate::social::gc1to1::lineup::on_open_wnd_req(self.svc, self.ctx, self.who);
    }

    fn on_gc1to1_member_line_up(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::Gc1to1MemberLineUp,
    ) {
        crate::social::gc1to1::lineup::on_member_line_up(self.svc, self.ctx, self.who, m.ids);
    }

    fn on_gc1to1_teleport_to_npc(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1TeleportToNpc,
    ) {
        crate::social::gc1to1::arena::on_teleport_to_npc(self.svc, self.ctx, self.who);
    }

    fn on_gc1to1_teleport_to_stage(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::Gc1to1TeleportToStage,
    ) {
        crate::social::gc1to1::arena::on_teleport_to_stage(self.svc, self.ctx, self.who);
    }

    // ---- stat points and the tutorial state (doc 16 §7.2, §15) ----

    fn on_inc_stat_level(&mut self, _sid: SessionId, m: flyff_proto::pb::IncStatLevel) {
        crate::stats::handlers::on_inc_stat_level(self.svc, self.ctx, self.who, m);
    }

    fn on_modify_status(&mut self, _sid: SessionId, m: flyff_proto::pb::ModifyStatus) {
        crate::stats::handlers::on_modify_status(self.svc, self.ctx, self.who, m);
    }

    fn on_tutorial_state(&mut self, _sid: SessionId, m: flyff_proto::pb::TutorialState) {
        crate::stats::handlers::on_tutorial_state(self.svc, self.ctx, self.who, m);
    }

    // ---- fashion and appearance switches (doc 16 §13.7) ----

    fn on_hide_coat_req(&mut self, _sid: SessionId, m: flyff_proto::pb::HideCoatReq) {
        crate::stats::handlers::on_hide_coat(self.svc, self.ctx, self.who, m);
    }

    fn on_option_enable_render_mask(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::OptionEnableRenderMask,
    ) {
        crate::stats::handlers::on_option_enable_render_mask(self.svc, self.ctx, self.who, m);
    }

    fn on_query_equip_setting(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryEquipSetting) {
        crate::stats::handlers::on_query_equip_setting(self.svc, self.ctx, self.who, m);
    }

    // ---- the fast job change and the PK / PvP option toggles (doc 16 §11.4, §15) ----

    fn on_update_job(&mut self, _sid: SessionId, m: flyff_proto::pb::UpdateJob) {
        crate::stats::handlers::on_update_job(self.svc, self.ctx, self.who, m);
    }

    fn on_change_mode(&mut self, _sid: SessionId, m: flyff_proto::pb::ChangeMode) {
        crate::stats::handlers::on_change_mode(self.svc, self.ctx, self.who, m);
    }

    // ---- EXP stop (doc 16 §8.5) ----

    fn on_exp_stop(&mut self, _sid: SessionId, m: flyff_proto::pb::ExpStop) {
        crate::stats::handlers::on_exp_stop(self.svc, self.ctx, self.who, m);
    }

    // ---- items (doc 19 §5.1, §5.13, §5.14) ----

    fn on_move_item_req(&mut self, _sid: SessionId, m: flyff_proto::pb::MoveItemReq) {
        crate::item::handlers::on_move_item(self.svc, self.ctx, self.who, m);
    }

    fn on_do_equip_req(&mut self, _sid: SessionId, m: flyff_proto::pb::DoEquipReq) {
        crate::item::handlers::on_do_equip(self.svc, self.ctx, self.who, m);
    }

    fn on_remove_inven_item(&mut self, _sid: SessionId, m: flyff_proto::pb::RemoveInvenItem) {
        crate::item::handlers::on_remove_inven_item(self.svc, self.ctx, self.who, m);
    }

    fn on_drop_item(&mut self, _sid: SessionId, m: flyff_proto::pb::DropItem) {
        crate::item::handlers::on_drop_item(self.svc, self.ctx, self.who, m);
    }

    fn on_do_use_item(&mut self, _sid: SessionId, m: flyff_proto::pb::DoUseItem) {
        let _ = crate::item::usage::on_do_use_item(self.svc, self.ctx, self.who, m);
    }

    fn on_set_lodelight(&mut self, _sid: SessionId, m: flyff_proto::pb::SetLodelight) {
        let _ = crate::combat::lodelight::on_set_lodelight(self.svc, self.ctx, self.who, m);
    }

    // ---- upgrades: refine, element, accessory, collector, safe smelt, stones, random options (doc 19 §12) ----

    fn on_enchant(&mut self, _sid: SessionId, m: flyff_proto::pb::Enchant) {
        let _ = crate::item::upgrade::handlers::on_enchant(self.svc, self.ctx, self.who, m);
    }

    fn on_smelt_safety_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SmeltSafetyReq) {
        let _ = crate::item::upgrade::handlers::on_smelt_safety(self.svc, self.ctx, self.who, m);
    }

    fn on_remove_attribute_req(&mut self, _sid: SessionId, m: flyff_proto::pb::RemoveAttributeReq) {
        let _ =
            crate::item::upgrade::handlers::on_remove_attribute(self.svc, self.ctx, self.who, m);
    }

    fn on_change_attribute(&mut self, _sid: SessionId, m: flyff_proto::pb::ChangeAttribute) {
        let _ =
            crate::item::upgrade::handlers::on_change_attribute(self.svc, self.ctx, self.who, m);
    }

    fn on_commercial_elem_req(&mut self, _sid: SessionId, m: flyff_proto::pb::CommercialElemReq) {
        let _ = crate::item::upgrade::handlers::on_commercial_elem(self.svc, self.ctx, self.who, m);
    }

    fn on_random_scroll(&mut self, _sid: SessionId, m: flyff_proto::pb::RandomScroll) {
        let _ = crate::item::upgrade::handlers::on_random_scroll(self.svc, self.ctx, self.who, m);
    }

    fn on_do_use_item_target(&mut self, _sid: SessionId, m: flyff_proto::pb::DoUseItemTarget) {
        let _ = crate::item::upgrade::handlers::on_use_target(self.svc, self.ctx, self.who, m);
    }

    fn on_awakening(&mut self, _sid: SessionId, m: flyff_proto::pb::Awakening) {
        let _ = crate::item::upgrade::handlers::on_awakening(self.svc, self.ctx, self.who, m);
    }

    fn on_blessedness_cancel(&mut self, _sid: SessionId, m: flyff_proto::pb::BlessednessCancel) {
        let _ =
            crate::item::upgrade::handlers::on_blessedness_cancel(self.svc, self.ctx, self.who, m);
    }

    fn on_item_select_awakening_value_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::ItemSelectAwakeningValueReq,
    ) {
        let _ = crate::item::upgrade::handlers::on_select_value(self.svc, self.ctx, self.who, m);
    }

    fn on_random_option_retry(&mut self, _sid: SessionId, m: flyff_proto::pb::RandomOptionRetry) {
        let _ = crate::item::upgrade::handlers::on_retry(self.svc, self.ctx, self.who, m);
    }

    fn on_remove_item_level_down(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::RemoveItemLevelDown,
    ) {
        let _ =
            crate::item::upgrade::handlers::on_remove_level_down(self.svc, self.ctx, self.who, m);
    }

    fn on_pickup_pet_awakening_cancel(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::PickupPetAwakeningCancel,
    ) {
        let _ = crate::item::upgrade::handlers::on_eat_pet_cancel(self.svc, self.ctx, self.who, m);
    }

    // ---- system pets: summon, feeding, tamer items, resurrection, egg transform (doc 19 §13, WP-ITEM-16, -17) ----

    fn on_pet_release_req(&mut self, _sid: SessionId, _m: flyff_proto::pb::PetReleaseReq) {
        crate::item::pets::system::on_pet_release_req(self.svc, self.ctx, self.who);
    }

    fn on_clear_pet_name(&mut self, _sid: SessionId, _m: flyff_proto::pb::ClearPetName) {
        crate::item::pets::system::on_clear_pet_name(self.svc, self.ctx, self.who);
    }

    fn on_do_use_item_input(&mut self, _sid: SessionId, m: flyff_proto::pb::DoUseItemInput) {
        let _ = crate::item::pets::system::on_do_use_item_input(self.svc, self.ctx, self.who, m);
    }

    fn on_use_pet_feed(&mut self, _sid: SessionId, m: flyff_proto::pb::UsePetFeed) {
        crate::item::pets::feed::on_use_pet_feed(self.svc, self.ctx, self.who, m);
    }

    fn on_make_pet_feed(&mut self, _sid: SessionId, m: flyff_proto::pb::MakePetFeed) {
        crate::item::pets::feed::on_make_pet_feed(self.svc, self.ctx, self.who, m);
    }

    fn on_pet_tamer_mistake(&mut self, _sid: SessionId, m: flyff_proto::pb::PetTamerMistake) {
        crate::item::pets::feed::on_pet_tamer_mistake(self.svc, self.ctx, self.who, m);
    }

    fn on_pet_tamer_miracle(&mut self, _sid: SessionId, m: flyff_proto::pb::PetTamerMiracle) {
        crate::item::pets::feed::on_pet_tamer_miracle(self.svc, self.ctx, self.who, m);
    }

    fn on_que_pet_resurrection(&mut self, _sid: SessionId, m: flyff_proto::pb::QuePetResurrection) {
        crate::item::pets::feed::on_que_pet_resurrection(self.svc, self.ctx, self.who, m);
    }

    fn on_transform_item(&mut self, _sid: SessionId, m: flyff_proto::pb::TransformItem) {
        crate::item::pets::feed::on_transform_item(self.svc, self.ctx, self.who, m);
    }

    // ---- eat pets, vis pets and the pet loot filter (doc 19 §13.5, §6.8, WP-ITEM-18) ----

    fn on_pet_filter(&mut self, _sid: SessionId, m: flyff_proto::pb::PetFilter) {
        crate::item::pets::filter::on_pet_filter(self.svc, self.ctx, self.who, m);
    }

    fn on_vis_pet_remove_vis(&mut self, _sid: SessionId, m: flyff_proto::pb::VisPetRemoveVis) {
        crate::item::pets::eat::hooks::on_vis_pet_remove_vis(self.svc, self.ctx, self.who, m);
    }

    fn on_vis_pet_swap_vis(&mut self, _sid: SessionId, m: flyff_proto::pb::VisPetSwapVis) {
        crate::item::pets::eat::hooks::on_vis_pet_swap_vis(self.svc, self.ctx, self.who, m);
    }

    fn on_state_mode_req(&mut self, _sid: SessionId, m: flyff_proto::pb::StateModeReq) {
        crate::item::usage::on_state_mode(self.svc, self.ctx, self.who, m);
    }

    fn on_do_escape(&mut self, _sid: SessionId, _m: flyff_proto::pb::DoEscape) {
        crate::item::usage::blink::on_do_escape(self.svc, self.ctx, self.who);
    }

    fn on_return_scroll_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ReturnScrollReq) {
        crate::item::usage::blink::on_return_scroll(self.svc, self.ctx, self.who, m);
    }

    // ---- pockets, the inventory sort, the equipment inspection (doc 19 §3.4, §3.5, §5.13.5, WP-ITEM-15) ----

    fn on_move_item_pocket(&mut self, _sid: SessionId, m: flyff_proto::pb::MoveItemPocket) {
        crate::item::pockets::on_move_item_pocket(self.svc, self.ctx, self.who, m);
    }

    fn on_avail_pocket(&mut self, _sid: SessionId, m: flyff_proto::pb::AvailPocket) {
        crate::item::pockets::on_avail_pocket(self.svc, self.ctx, self.who, m);
    }

    fn on_sort_inventory(&mut self, _sid: SessionId, m: flyff_proto::pb::SortInventory) {
        crate::item::pockets::on_sort_inventory(self.svc, self.ctx, self.who, m);
    }

    fn on_query_equip_req(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryEquipReq) {
        crate::item::pockets::on_query_equip(self.svc, self.ctx, self.who, m);
    }

    // ---- collecting (doc 19 §14, WP-ITEM-19) ----

    fn on_query_start_collecting(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::QueryStartCollecting,
    ) {
        crate::item::collect::on_start(self.svc, self.ctx, self.who, m);
    }

    fn on_query_stop_collecting(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::QueryStopCollecting,
    ) {
        crate::item::collect::on_stop(self.svc, self.ctx, self.who, m);
    }

    // ---- ultimate weapons, piercing, item transy, angels and monster scrolls (doc 19 §5.10, §5.12, §12.5-§12.8) ----

    fn on_ultimate_make_item(&mut self, _sid: SessionId, m: flyff_proto::pb::UltimateMakeItem) {
        let _ = crate::item::ultimate::on_make_item(self.svc, self.ctx, self.who, m);
    }

    fn on_ultimate_make_gem(&mut self, _sid: SessionId, m: flyff_proto::pb::UltimateMakeGem) {
        let _ = crate::item::ultimate::on_make_gem(self.svc, self.ctx, self.who, m);
    }

    fn on_ultimate_trans_weapon(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::UltimateTransWeapon,
    ) {
        let _ = crate::item::ultimate::on_trans_weapon(self.svc, self.ctx, self.who, m);
    }

    fn on_ultimate_enchant_weapon(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::UltimateEnchantWeapon,
    ) {
        let _ = crate::item::ultimate::on_enchant_weapon(self.svc, self.ctx, self.who, m);
    }

    fn on_ultimate_set_gem(&mut self, _sid: SessionId, m: flyff_proto::pb::UltimateSetGem) {
        let _ = crate::item::ultimate::on_set_gem(self.svc, self.ctx, self.who, m);
    }

    fn on_ultimate_remove_gem(&mut self, _sid: SessionId, m: flyff_proto::pb::UltimateRemoveGem) {
        let _ = crate::item::ultimate::on_remove_gem(self.svc, self.ctx, self.who, m);
    }

    fn on_piercing_size(&mut self, _sid: SessionId, m: flyff_proto::pb::PiercingSize) {
        let _ = crate::item::piercing::on_piercing_size(self.svc, self.ctx, self.who, m);
    }

    fn on_piercing_req(&mut self, _sid: SessionId, m: flyff_proto::pb::PiercingReq) {
        let _ = crate::item::piercing::on_piercing(self.svc, self.ctx, self.who, m);
    }

    fn on_piercing_remove(&mut self, _sid: SessionId, m: flyff_proto::pb::PiercingRemove) {
        let _ = crate::item::piercing::on_piercing_remove(self.svc, self.ctx, self.who, m);
    }

    fn on_item_transy(&mut self, _sid: SessionId, m: flyff_proto::pb::ItemTransy) {
        let _ = crate::item::piercing::transy::on_item_transy(self.svc, self.ctx, self.who, m);
    }

    fn on_create_angel(&mut self, _sid: SessionId, m: flyff_proto::pb::CreateAngel) {
        let _ = crate::item::summon::angel::on_create_angel(self.svc, self.ctx, self.who, m);
    }

    fn on_create_monster(&mut self, _sid: SessionId, m: flyff_proto::pb::CreateMonster) {
        let _ = crate::item::summon::create_monster::on_request(self.svc, self.ctx, self.who, m);
    }

    // ---- buff-dependent item effects (doc 19 §5.12, §13.4, WP-ITEM-20) ----

    fn on_angel_buff(&mut self, _sid: SessionId, m: flyff_proto::pb::AngelBuff) {
        crate::item::buffitems::angel::on_angel_buff(self.svc, self.ctx, self.who, m);
    }

    fn on_feed_pocket_inactive(&mut self, _sid: SessionId, m: flyff_proto::pb::FeedPocketInactive) {
        crate::item::buffitems::pets::on_feed_pocket_inactive(self.svc, self.ctx, self.who, m);
    }

    // ---- NPC shops, quick sell and the beauty shop (doc 19 §7, WP-ECON-01, WP-ECON-02) ----

    fn on_open_shop_wnd_req(&mut self, _sid: SessionId, m: flyff_proto::pb::OpenShopWndReq) {
        crate::econ::shop::on_open(self.svc, self.ctx, self.who, m);
    }

    fn on_close_shop_wnd(&mut self, _sid: SessionId, m: flyff_proto::pb::CloseShopWnd) {
        crate::econ::shop::on_close(self.svc, self.ctx, self.who, m);
    }

    fn on_buy_item(&mut self, _sid: SessionId, m: flyff_proto::pb::BuyItem) {
        crate::econ::shop::on_buy(self.svc, self.ctx, self.who, m);
    }

    fn on_buy_chip_item(&mut self, _sid: SessionId, m: flyff_proto::pb::BuyChipItem) {
        crate::econ::shop::on_buy_chip(self.svc, self.ctx, self.who, m);
    }

    fn on_sell_item(&mut self, _sid: SessionId, m: flyff_proto::pb::SellItem) {
        crate::econ::shop::on_sell(self.svc, self.ctx, self.who, m);
    }

    fn on_quick_sell(&mut self, _sid: SessionId, m: flyff_proto::pb::QuickSell) {
        crate::econ::quick_sell::on_quick_sell(self.svc, self.ctx, self.who, m);
    }

    fn on_set_hair_req(&mut self, _sid: SessionId, m: flyff_proto::pb::SetHairReq) {
        crate::econ::beauty::on_set_hair(self.svc, self.ctx, self.who, m);
    }

    fn on_change_face_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ChangeFaceReq) {
        crate::econ::beauty::on_change_face(self.svc, self.ctx, self.who, m);
    }

    // ---- player trade, the personal vendor and its chat rooms (doc 19 §8, §9, WP-ECON-03, -04, -11) ----

    fn on_confirm_trade_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ConfirmTradeReq) {
        crate::econ::trade::on_confirm_trade_req(self.svc, self.ctx, self.who, m);
    }

    fn on_confirm_trade_cancel_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::ConfirmTradeCancelReq,
    ) {
        crate::econ::trade::on_confirm_trade_cancel_req(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_req(&mut self, _sid: SessionId, m: flyff_proto::pb::TradeReq) {
        crate::econ::trade::on_trade_req(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_put_req(&mut self, _sid: SessionId, m: flyff_proto::pb::TradePutReq) {
        crate::econ::trade::on_trade_put_req(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_pull_req(&mut self, _sid: SessionId, m: flyff_proto::pb::TradePullReq) {
        crate::econ::trade::on_trade_pull_req(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_put_gold_req(&mut self, _sid: SessionId, m: flyff_proto::pb::TradePutGoldReq) {
        crate::econ::trade::on_trade_put_gold_req(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_ok_req(&mut self, _sid: SessionId, m: flyff_proto::pb::TradeOkReq) {
        crate::econ::trade::on_trade_ok_req(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_confirm(&mut self, _sid: SessionId, m: flyff_proto::pb::TradeConfirm) {
        crate::econ::trade::on_trade_confirm(self.svc, self.ctx, self.who, m);
    }

    fn on_trade_cancel_req(&mut self, _sid: SessionId, m: flyff_proto::pb::TradeCancelReq) {
        crate::econ::trade::on_trade_cancel_req(self.svc, self.ctx, self.who, m);
    }

    fn on_p_vendor_open_req(&mut self, _sid: SessionId, m: flyff_proto::pb::PVendorOpenReq) {
        crate::econ::vendor::on_open(self.svc, self.ctx, self.who, m);
    }

    fn on_p_vendor_close_req(&mut self, _sid: SessionId, m: flyff_proto::pb::PVendorCloseReq) {
        crate::econ::vendor::on_close(self.svc, self.ctx, self.who, m);
    }

    fn on_register_p_vendor_item_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::RegisterPVendorItemReq,
    ) {
        crate::econ::vendor::on_register(self.svc, self.ctx, self.who, m);
    }

    fn on_unregister_p_vendor_item_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::UnregisterPVendorItemReq,
    ) {
        crate::econ::vendor::on_unregister(self.svc, self.ctx, self.who, m);
    }

    fn on_query_p_vendor_item(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryPVendorItem) {
        crate::econ::vendor::on_query(self.svc, self.ctx, self.who, m);
    }

    fn on_buy_p_vendor_item(&mut self, _sid: SessionId, m: flyff_proto::pb::BuyPVendorItem) {
        crate::econ::vendor::on_buy(self.svc, self.ctx, self.who, m);
    }

    fn on_enter_chatting(&mut self, _sid: SessionId, m: flyff_proto::pb::EnterChatting) {
        crate::chat::vendor_room::on_enter(self.svc, self.ctx, self.who, m);
    }

    fn on_chatting_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ChattingReq) {
        crate::chat::vendor_room::on_chatting(self.svc, self.ctx, self.who, m);
    }

    fn on_open_chatting_room(&mut self, _sid: SessionId, m: flyff_proto::pb::OpenChattingRoom) {
        crate::chat::vendor_room::on_open_room(self.svc, self.ctx, self.who, m);
    }

    fn on_close_chatting_room(&mut self, _sid: SessionId, m: flyff_proto::pb::CloseChattingRoom) {
        crate::chat::vendor_room::on_close_room(self.svc, self.ctx, self.who, m);
    }

    // ---- the bank, mail and the exchange scripts (doc 19 §10, §11, §16, WP-ECON-05, -06, -07) ----

    fn on_open_bank_wnd(&mut self, _sid: SessionId, m: flyff_proto::pb::OpenBankWnd) {
        crate::econ::bank::on_open(self.svc, self.ctx, self.who, m);
    }

    fn on_close_bank_wnd(&mut self, _sid: SessionId, m: flyff_proto::pb::CloseBankWnd) {
        crate::econ::bank::on_close(self.svc, self.ctx, self.who, m);
    }

    fn on_confirm_bank(&mut self, _sid: SessionId, m: flyff_proto::pb::ConfirmBank) {
        crate::econ::bank::on_confirm(self.svc, self.ctx, self.who, m);
    }

    fn on_change_bank_pass_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ChangeBankPassReq) {
        crate::econ::bank::on_change_pin(self.svc, self.ctx, self.who, m);
    }

    fn on_put_item_back(&mut self, _sid: SessionId, m: flyff_proto::pb::PutItemBack) {
        crate::econ::bank::on_put_item(self.svc, self.ctx, self.who, m);
    }

    fn on_get_item_back(&mut self, _sid: SessionId, m: flyff_proto::pb::GetItemBack) {
        crate::econ::bank::on_get_item(self.svc, self.ctx, self.who, m);
    }

    fn on_put_gold_back(&mut self, _sid: SessionId, m: flyff_proto::pb::PutGoldBack) {
        crate::econ::bank::on_put_gold(self.svc, self.ctx, self.who, m);
    }

    fn on_get_gold_back(&mut self, _sid: SessionId, m: flyff_proto::pb::GetGoldBack) {
        crate::econ::bank::on_get_gold(self.svc, self.ctx, self.who, m);
    }

    fn on_put_back_to_bank(&mut self, _sid: SessionId, m: flyff_proto::pb::PutBackToBank) {
        crate::econ::bank::on_bank_to_bank(self.svc, self.ctx, self.who, m);
    }

    fn on_query_post_mail(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryPostMail) {
        crate::econ::mail::on_post(self.svc, self.ctx, self.who, m);
    }

    fn on_query_remove_mail(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryRemoveMail) {
        crate::econ::mail::on_remove(self.svc, self.ctx, self.who, m);
    }

    fn on_query_get_mail_item(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryGetMailItem) {
        crate::econ::mail::on_take_item(self.svc, self.ctx, self.who, m);
    }

    fn on_query_get_mail_gold(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryGetMailGold) {
        crate::econ::mail::on_take_gold(self.svc, self.ctx, self.who, m);
    }

    fn on_read_mail(&mut self, _sid: SessionId, m: flyff_proto::pb::ReadMail) {
        crate::econ::mail::on_read(self.svc, self.ctx, self.who, m);
    }

    fn on_query_mail_box_req(&mut self, _sid: SessionId, m: flyff_proto::pb::QueryMailBoxReq) {
        crate::econ::mail::on_query_box(self.svc, self.ctx, self.who, m);
    }

    fn on_exchange_req(&mut self, _sid: SessionId, m: flyff_proto::pb::ExchangeReq) {
        crate::econ::exchange::on_exchange(self.svc, self.ctx, self.who, m);
    }

    // ---- NPC dialog and NPC services (doc 20 §7.2, §8.5, §8.6) ----

    fn on_script_dlg(&mut self, _sid: SessionId, m: flyff_proto::pb::ScriptDlg) {
        crate::npc::on_script_dlg(self.svc, self.ctx, self.who, m);
    }

    fn on_npc_buff(&mut self, _sid: SessionId, m: flyff_proto::pb::NpcBuff) {
        crate::npc::on_npc_buff(self.svc, self.ctx, self.who, m);
    }

    fn on_teleporter(&mut self, _sid: SessionId, m: flyff_proto::pb::Teleporter) {
        crate::npc::on_teleporter(self.svc, self.ctx, self.who, m);
    }

    fn on_tele16(&mut self, _sid: SessionId, m: flyff_proto::pb::Tele16) {
        crate::npc::on_tele16(self.svc, self.ctx, self.who, m);
    }

    // ---- arena (doc 22 §17.1, WP-EVENT-21) ----

    fn on_arena_enter(&mut self, _sid: SessionId, m: flyff_proto::pb::ArenaEnter) {
        crate::event::misc::on_arena_enter(self.svc, self.ctx, self.who, m);
    }

    fn on_arena_exit(&mut self, _sid: SessionId, m: flyff_proto::pb::ArenaExit) {
        crate::event::misc::on_arena_exit(self.svc, self.ctx, self.who, m);
    }

    // ---- quests (doc 20 §9.10, WP-NPC-12) ----

    fn on_remove_quest_req(&mut self, _sid: SessionId, m: flyff_proto::pb::RemoveQuestReq) {
        crate::quest::handlers::on_remove_quest_req(self.svc, self.ctx, self.who, m);
    }

    fn on_quest_check(&mut self, _sid: SessionId, m: flyff_proto::pb::QuestCheck) {
        crate::quest::handlers::on_quest_check(self.svc, self.ctx, self.who, m);
    }

    // ---- quest effects (doc 20 §9.9, WP-NPC-15) ----

    fn on_quest_helper_req_npc_pos(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::QuestHelperReqNpcPos,
    ) {
        crate::quest::locator::on_quest_helper_req_npc_pos(self.svc, self.ctx, self.who, m);
    }

    // ---- the taskbar (doc 42 §11.6, WP-UI-18) ----

    fn on_add_applet_task_bar(&mut self, _sid: SessionId, m: flyff_proto::pb::AddAppletTaskBar) {
        crate::taskbar::on_add_applet(self.ctx, self.who, m);
    }

    fn on_remove_applet_task_bar(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::RemoveAppletTaskBar,
    ) {
        crate::taskbar::on_remove_applet(self.ctx, self.who, m);
    }

    fn on_add_item_task_bar(&mut self, _sid: SessionId, m: flyff_proto::pb::AddItemTaskBar) {
        crate::taskbar::on_add_item(self.ctx, self.who, m);
    }

    fn on_remove_item_task_bar(&mut self, _sid: SessionId, m: flyff_proto::pb::RemoveItemTaskBar) {
        crate::taskbar::on_remove_item(self.ctx, self.who, m);
    }

    fn on_skill_task_bar(&mut self, _sid: SessionId, m: flyff_proto::pb::SkillTaskBar) {
        crate::taskbar::on_skill_taskbar(self.ctx, self.who, m);
    }

    // ---- couple, campus, honor titles (doc 21 §11-§13, WP-SOCIAL-21..23) ----

    fn on_propose(&mut self, _sid: SessionId, m: flyff_proto::pb::Propose) {
        crate::social::couple::flow::on_propose(self.svc, self.ctx, self.who, m);
    }

    fn on_refuse(&mut self, _sid: SessionId, m: flyff_proto::pb::Refuse) {
        crate::social::couple::flow::on_refuse(self.svc, self.ctx, self.who, m);
    }

    fn on_couple_req(&mut self, _sid: SessionId, m: flyff_proto::pb::CoupleReq) {
        crate::social::couple::flow::on_couple_req(self.svc, self.ctx, self.who, m);
    }

    fn on_decouple(&mut self, _sid: SessionId, m: flyff_proto::pb::Decouple) {
        crate::social::couple::flow::on_decouple(self.svc, self.ctx, self.who, m);
    }

    fn on_campus_invite_req(&mut self, _sid: SessionId, m: flyff_proto::pb::CampusInviteReq) {
        crate::social::campus::invite::on_invite_req(self.svc, self.ctx, self.who, m);
    }

    fn on_campus_accept(&mut self, _sid: SessionId, m: flyff_proto::pb::CampusAccept) {
        crate::social::campus::invite::on_accept(self.svc, self.ctx, self.who, m);
    }

    fn on_campus_refuse(&mut self, _sid: SessionId, m: flyff_proto::pb::CampusRefuse) {
        crate::social::campus::invite::on_refuse(self.svc, self.ctx, self.who, m);
    }

    fn on_campus_remove_member(&mut self, _sid: SessionId, m: flyff_proto::pb::CampusRemoveMember) {
        crate::social::campus::invite::on_remove_member(self.svc, self.ctx, self.who, m);
    }

    fn on_honor_list_req(&mut self, _sid: SessionId, m: flyff_proto::pb::HonorListReq) {
        crate::social::honor::on_list_req(self.svc, self.ctx, self.who, m);
    }

    fn on_honor_change_req(&mut self, _sid: SessionId, m: flyff_proto::pb::HonorChangeReq) {
        crate::social::honor::on_change_req(self.svc, self.ctx, self.who, m);
    }

    // ---- NPC event mini-games (doc 22 §16, WP-EVENT-20) ----

    fn on_kawibawibo_start(&mut self, _sid: SessionId, m: flyff_proto::pb::KawibawiboStart) {
        crate::event::minigame::handlers::on_kawibawibo_start(self.svc, self.ctx, self.who, m);
    }

    fn on_kawibawibo_get_item(&mut self, _sid: SessionId, m: flyff_proto::pb::KawibawiboGetItem) {
        crate::event::minigame::handlers::on_kawibawibo_get_item(self.svc, self.ctx, self.who, m);
    }

    fn on_reassemble_open_wnd(&mut self, _sid: SessionId, m: flyff_proto::pb::ReassembleOpenWnd) {
        crate::event::minigame::handlers::on_reassemble_open_wnd(self.svc, self.ctx, self.who, m);
    }

    fn on_reassemble_start(&mut self, _sid: SessionId, m: flyff_proto::pb::ReassembleStart) {
        crate::event::minigame::handlers::on_reassemble_start(self.svc, self.ctx, self.who, m);
    }

    fn on_alphabet_open_wnd(&mut self, _sid: SessionId, m: flyff_proto::pb::AlphabetOpenWnd) {
        crate::event::minigame::handlers::on_alphabet_open_wnd(self.svc, self.ctx, self.who, m);
    }

    fn on_alphabet_start(&mut self, _sid: SessionId, m: flyff_proto::pb::AlphabetStart) {
        crate::event::minigame::handlers::on_alphabet_start(self.svc, self.ctx, self.who, m);
    }

    fn on_five_system_open_wnd(&mut self, _sid: SessionId, m: flyff_proto::pb::FiveSystemOpenWnd) {
        crate::event::minigame::handlers::on_five_system_open_wnd(self.svc, self.ctx, self.who, m);
    }

    fn on_five_system_destroy_wnd(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::FiveSystemDestroyWnd,
    ) {
        crate::event::minigame::handlers::on_five_system_destroy_wnd(
            self.svc, self.ctx, self.who, m,
        );
    }

    fn on_five_system_bet(&mut self, _sid: SessionId, m: flyff_proto::pb::FiveSystemBet) {
        crate::event::minigame::handlers::on_five_system_bet(self.svc, self.ctx, self.who, m);
    }

    fn on_five_system_start(&mut self, _sid: SessionId, m: flyff_proto::pb::FiveSystemStart) {
        crate::event::minigame::handlers::on_five_system_start(self.svc, self.ctx, self.who, m);
    }

    // ---- lord election, lord events and lord skills (doc 22 §9, WP-EVENT-10/11) ----

    fn on_election_add_deposit_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::ElectionAddDepositReq,
    ) {
        crate::event::lord::handlers::on_election_add_deposit(self.svc, self.ctx, self.who, m);
    }

    fn on_election_set_pledge_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::ElectionSetPledgeReq,
    ) {
        crate::event::lord::handlers::on_election_set_pledge(self.svc, self.ctx, self.who, m);
    }

    fn on_election_inc_vote_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::ElectionIncVoteReq,
    ) {
        crate::event::lord::handlers::on_election_inc_vote(self.svc, self.ctx, self.who, m);
    }

    fn on_l_event_create_req(&mut self, _sid: SessionId, m: flyff_proto::pb::LEventCreateReq) {
        crate::event::lord::handlers::on_l_event_create(self.svc, self.ctx, self.who, m);
    }

    fn on_lord_skill_use_req(&mut self, _sid: SessionId, m: flyff_proto::pb::LordSkillUseReq) {
        crate::event::lord::handlers::on_lord_skill_use(self.svc, self.ctx, self.who, m);
    }

    // ---- tax and the Heaven Tower (doc 22 §10, WP-EVENT-12) ----

    fn on_tax_set_tax_rate(&mut self, _sid: SessionId, m: flyff_proto::pb::TaxSetTaxRate) {
        crate::event::tax::rate::on_tax_set_tax_rate(self.svc, self.ctx, self.who, m);
    }

    fn on_heaven_tower_teleport(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::HeavenTowerTeleport,
    ) {
        crate::event::tax::heaven::on_heaven_tower_teleport(self.svc, self.ctx, self.who, m);
    }

    // ---- quiz (doc 22 §15, WP-EVENT-19) ----

    fn on_quiz_entrance(&mut self, _sid: SessionId, _m: flyff_proto::pb::QuizEntrance) {
        crate::event::quiz::on_entrance(self.svc, self.ctx, self.who);
    }

    fn on_quiz_teleport(&mut self, _sid: SessionId, m: flyff_proto::pb::QuizTeleport) {
        crate::event::quiz::on_teleport(self.svc, self.ctx, self.who, m.zone);
    }

    // ---- rainbow race (doc 22 §14, task rainbow-race) ----

    fn on_rainbow_race_prev_ranking_open_wnd_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::RainbowRacePrevRankingOpenWndReq,
    ) {
        crate::event::rainbow::handlers::on_prev_ranking_open_wnd(self.svc, self.ctx, self.who);
    }

    fn on_rainbow_race_application_open_wnd_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::RainbowRaceApplicationOpenWndReq,
    ) {
        crate::event::rainbow::handlers::on_application_open_wnd(self.svc, self.ctx, self.who);
    }

    fn on_rainbow_race_application(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::RainbowRaceApplication,
    ) {
        crate::event::rainbow::handlers::on_application(self.svc, self.ctx, self.who);
    }

    fn on_rainbow_race_mini_game_packet(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::RainbowRaceMiniGamePacket,
    ) {
        crate::event::rainbow::handlers::on_mini_game_packet(self.svc, self.ctx, self.who, m);
    }

    fn on_rainbow_race_req_finish(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::RainbowRaceReqFinish,
    ) {
        crate::event::rainbow::handlers::on_req_finish(self.svc, self.ctx, self.who);
    }

    // ---- housing (doc 22 §13, WP-EVENT-16) ----

    fn on_housing_setup_furniture_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::HousingSetupFurnitureReq,
    ) {
        crate::event::housing::on_setup_furniture(self.svc, self.ctx, self.who, m);
    }

    fn on_housing_visit_room(&mut self, _sid: SessionId, m: flyff_proto::pb::HousingVisitRoom) {
        crate::event::housing::on_visit_room(self.svc, self.ctx, self.who, m);
    }

    fn on_housing_set_visit_allow_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::HousingSetVisitAllowReq,
    ) {
        crate::event::housing::on_set_visit_allow(self.svc, self.ctx, self.who, m);
    }

    fn on_housing_req_visitable_list(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::HousingReqVisitableList,
    ) {
        crate::event::housing::on_req_visitable_list(self.svc, self.ctx, self.who);
    }

    fn on_housing_go_out(&mut self, _sid: SessionId, _m: flyff_proto::pb::HousingGoOut) {
        crate::event::housing::on_go_out(self.svc, self.ctx, self.who);
    }

    // ---- guild house (doc 22 §12, WP-EVENT-14) ----

    fn on_guild_house_buy(&mut self, _sid: SessionId, _m: flyff_proto::pb::GuildHouseBuy) {
        crate::event::guild_house::on_buy(self.svc, self.ctx, self.who);
    }

    fn on_guild_house_packet_req(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::GuildHousePacketReq,
    ) {
        crate::event::guild_house::on_packet_req(self.svc, self.ctx, self.who, m);
    }

    fn on_guild_house_enter(&mut self, _sid: SessionId, _m: flyff_proto::pb::GuildHouseEnter) {
        crate::event::guild_house::on_enter(self.svc, self.ctx, self.who);
    }

    fn on_guild_house_go_out(&mut self, _sid: SessionId, _m: flyff_proto::pb::GuildHouseGoOut) {
        crate::event::guild_house::on_go_out(self.svc, self.ctx, self.who);
    }

    // ---- secret room (doc 22 §11, WP-EVENT-13) ----

    fn on_secret_room_tender(&mut self, _sid: SessionId, m: flyff_proto::pb::SecretRoomTender) {
        crate::event::secret_room::tender::on_tender(self.svc, self.ctx, self.who, m.penya);
    }

    fn on_secret_room_line_up_member(
        &mut self,
        _sid: SessionId,
        m: flyff_proto::pb::SecretRoomLineUpMember,
    ) {
        crate::event::secret_room::lineup::on_line_up_member(self.svc, self.ctx, self.who, m.ids);
    }

    fn on_secret_room_tender_open_wnd_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomTenderOpenWndReq,
    ) {
        crate::event::secret_room::tender::on_tender_open_wnd(self.svc, self.ctx, self.who);
    }

    fn on_secret_room_line_up_open_wnd_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomLineUpOpenWndReq,
    ) {
        crate::event::secret_room::lineup::on_line_up_open_wnd(self.svc, self.ctx, self.who);
    }

    fn on_secret_room_entrance(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomEntrance,
    ) {
        crate::event::secret_room::war::on_entrance(self.svc, self.ctx, self.who);
    }

    fn on_secret_room_teleport_to_npc(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomTeleportToNpc,
    ) {
        crate::event::secret_room::rules::on_teleport_to_npc(self.svc, self.ctx, self.who);
    }

    fn on_secret_room_tender_view_req(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomTenderViewReq,
    ) {
        crate::event::secret_room::tender::on_tender_view(self.svc, self.ctx, self.who);
    }

    fn on_secret_room_tender_cancel_return(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomTenderCancelReturn,
    ) {
        crate::event::secret_room::tender::on_cancel_return(self.svc, self.ctx, self.who);
    }

    fn on_secret_room_teleport_to_dungeon(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::SecretRoomTeleportToDungeon,
    ) {
        crate::event::secret_room::rules::on_teleport_to_dungeon(self.svc, self.ctx, self.who);
    }

    // ---- leaving (doc 12 §12.3-§12.4) ----

    fn on_req_leave(&mut self, _sid: SessionId, _m: flyff_proto::pb::ReqLeave) {
        crate::session::on_req_leave(self.ctx, self.who);
    }

    fn on_return_to_char_select(
        &mut self,
        _sid: SessionId,
        _m: flyff_proto::pb::ReturnToCharSelect,
    ) {
        crate::session::on_return_to_char_select(self.ctx, self.who);
    }

    fn unhandled(&mut self, sid: SessionId, name: &'static str) {
        *self.ctx.counters.unhandled.entry(name).or_default() += 1;
        tracing::debug!(
            session_id = sid.0,
            message = name,
            "client message without a handler"
        );
    }
}

/// Executes the `AdminCmd`s the runtime does not own (GM variants of doc 23, script reloads of doc 20,
/// `SaveAll`/`SetRuntime`/... of doc 10's later WPs). `actor` is passed exactly as it arrived in
/// `GameInput::Admin`.
pub trait AdminHandler: Send + 'static {
    /// Runs `cmd` and returns the answer (within the same tick).
    fn run(
        &mut self,
        svc: &mut Services,
        ctx: &mut GameCtx<'_>,
        cmd: AdminCmd,
        actor: AdminActor,
    ) -> AdminOutput;

    /// `true` when this handler executes `cmd` itself even though the runtime has a built-in implementation of
    /// it (`crate::admin_ops`: `ListOnline`, `Kick`, `Broadcast`, the rates and event flags, `SetRuntime`,
    /// `UnsetRuntime`). Doc 23's GM handler returns `true` for the variants whose semantics it owns (23 §8.2); the
    /// default is `false`. Commands the runtime does not implement always go to [`AdminHandler::run`].
    fn handles(&self, _cmd: &AdminCmd) -> bool {
        false
    }
}

/// The default admin handler: refuses everything the runtime does not execute itself.
#[derive(Debug, Default)]
pub struct NoAdmin;

impl AdminHandler for NoAdmin {
    fn run(
        &mut self,
        _svc: &mut Services,
        _ctx: &mut GameCtx<'_>,
        cmd: AdminCmd,
        _actor: AdminActor,
    ) -> AdminOutput {
        AdminOutput::Error(format!(
            "{} is not available yet",
            crate::dispatch::admin_name(&cmd)
        ))
    }
}

/// The plug points (see the module docs). `Default` installs the M1 stubs.
pub struct Handlers {
    /// Join, leave rule, `finish_leave`, characters.
    pub rules: Box<dyn SessionRules>,
    /// In-world client messages.
    pub client: Box<dyn ClientRouter>,
    /// Admin commands outside the runtime's own.
    pub admin: Box<dyn AdminHandler>,
    /// The per-mover bodies of the world phases (AI, `step_frame`, event handlers, tick tails; Plan/15 §5.3).
    /// The movement pipeline of doc 18 is [`crate::movement::MovementHooks`].
    pub world: Box<dyn crate::world::WorldHooks>,
    /// The tables and settings of the movement pipeline (motion table, model bounds, keyframe rate), read once
    /// by `Game::new` into `Worlds.movement`; also used by the client handlers of `PlayerMoveState`.
    pub movement: crate::movement::MoveInit,
    /// The running events' descriptions, shared with the per-player 1 s timer of the world phase (Plan/22 §4.12): the
    /// server gives the same board to `event::EventSecondHooks` (installed in `StatsHooks::with_second`) and to
    /// the game, which publishes to it. A fresh board is disconnected from any hook.
    pub event_desc: crate::event::DescBoard,
    /// The side channel of the account changes the GM commands make (`/ban`, `/setauth`): the admin console ends the
    /// admin sessions of the account (Plan/25 §1.8). Disconnected by default.
    pub account_events: crate::input::AccountEventSink,
    // ---- gm-events-quests (doc 23 §4.6, WP-GM-25) ----
    /// What the GM commands ask of the server (`/loadscript`, `/reload config`): the server's forwarder
    /// (`admin/gm_reload.rs`) drains it. Disconnected by default.
    pub server_requests: crate::gm::ServerRequestSink,
    // ---- instances (doc 22 §8, task instances) ----
    /// The notes the doc-22 world hooks (`event::hooks::EventWorldHooks`: the instance portal trigger and the world or
    /// layer change of a player) leave for the event services; the game keeps the other end and drains it after the
    /// world phase (`event::hooks::drain_world_notes`). A fresh value is disconnected from any hook.
    pub event_world: crate::event::hooks::WorldNotes,
    // ---- bosses (doc 18 §10.4, task boss-ai-core, WP-AI-18) ----
    /// The rigs of the bosses (the server wraps `ServerDerived.rigs`), read once by `Game::new` into
    /// `Worlds.movement`: the ClockWorks hits need the event points and bones of their model. `None` = no pose data.
    pub boss_poses: Option<std::sync::Arc<dyn crate::ai::boss::pose::BossPoseSource>>,
}

impl Default for Handlers {
    fn default() -> Self {
        Self {
            rules: Box::new(StubRules::new()),
            client: Box::new(DefaultRouter),
            admin: Box::new(NoAdmin),
            world: Box::new(crate::world::phases::NoHooks),
            movement: crate::movement::MoveInit::default(),
            event_desc: crate::event::DescBoard::default(),
            account_events: crate::input::AccountEventSink::default(),
            server_requests: crate::gm::ServerRequestSink::default(),
            event_world: crate::event::hooks::WorldNotes::default(),
            boss_poses: None,
        }
    }
}

impl std::fmt::Debug for Handlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handlers(..)")
    }
}
