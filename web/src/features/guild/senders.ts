// The sends of the guild windows, the player menu, the bank and the guild war (rule C9: `// legacy-send: <SendFn>`). 28 C2S messages: all of `guild.proto` (23) and `guild_war.proto`
// (5). The session identifies the sender (VR-001), so the leading own-id fields of the legacy packets are gone; `IgnoreGuildInvite` and `GuildBankMoveItem` are built and
// registered but have no caller (the legacy client never sent them, KEEP).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  AddGuildMemberReqSchema,
  ChgMasterReqSchema,
  CreateGuildCloakSchema,
  DestroyGuildReqSchema,
  GetItemGuildBankReqSchema,
  GuildAuthorityReqSchema,
  GuildBankMoveItemSchema,
  GuildBankWndCloseSchema,
  GuildBankWndReqSchema,
  GuildClassReqSchema,
  GuildContributionReqSchema,
  GuildInviteReqSchema,
  GuildLogoReqSchema,
  GuildLogViewReqSchema,
  GuildMemberLevelReqSchema,
  GuildNicknameReqSchema,
  GuildNoticeReqSchema,
  GuildPenyaReqSchema,
  GuildSetNameReqSchema,
  IgnoreGuildInviteSchema,
  PutItemGuildBankReqSchema,
  QuerySetGuildNameSchema,
  RemoveGuildMemberReqSchema,
} from '../../gen/proto/flyff/v1/guild_pb';
import {
  AcptGuildWarReqSchema,
  AcptTruceSchema,
  DeclGuildWarReqSchema,
  QueryTruceReqSchema,
  SurrenderReqSchema,
} from '../../gen/proto/flyff/v1/guild_war_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders over one client context. */
export interface GuildSenders {
  /** `SendGuildInvite( objId )`: invite a player (needs the INVITATION power). */
  invite(targetObj: number): void;
  /** Never called (the legacy client never sent `IGNORE_GUILD_INVITE`). */
  ignoreInvite(inviter: number): void;
  /** `SendAddGuildMember( idMaster, info )`: accept an invitation. */
  addMember(inviter: number): void;
  /** `SendRemoveGuildMember( idRequester, idPlayer )`: leave (own id) or remove a member. */
  removeMember(idPlayer: number): void;
  /** `SendDestroyGuild( idMaster )`. */
  destroy(): void;
  /** `SendGuildMemberLv( idMaster, idPlayer, nMemberLv )`. */
  memberLevel(idPlayer: number, rank: number): void;
  /** `SendGuildClass( idMaster, idPlayer, nFlag )`: `up` = raise. */
  guildClass(idPlayer: number, up: boolean): void;
  /** `SendGuildNickName( idPlayer, alias )`. */
  nickname(idPlayer: number, alias: string): void;
  /** `SendChgMaster( idOld, idNew )`. */
  chgMaster(newMaster: number): void;
  /** `SendGuildAuthority( idGuild, power[5] )`. */
  authority(power: readonly number[]): void;
  /** `SendGuilPenya( idGuild, rank, penya )`. */
  penya(rank: number, penya: number): void;
  /** `SendGuildSetName( name )`. */
  setName(name: string): void;
  /** `SendGuildLogo( logo )`. */
  logo(logo: number): void;
  /** `SendGuildContribution( cbPxpCount, gold, cbItemFlag )`. */
  contribution(gold: number, itemFlag: boolean): void;
  /** `SendGuildNotice( notice )`. */
  notice(notice: string): void;
  /** `SendCreateGuildCloak()`. */
  createCloak(): void;
  /** `SendQuerySetGuildName( name, scroll )`: the rename with a scroll. */
  querySetName(itemObjId: number, name: string): void;
  /** `SendOpenGuildBankWnd()`. */
  openBank(): void;
  /** `SendCloseGuildBankWnd()`. */
  closeBank(): void;
  /** `SendPutItemGuildBank( objId, count, mode )`: mode 1 = item. */
  putItem(objId: number, count: number): void;
  /** `SendGetItemGuildBank( objId, count, mode )`: mode 0 = penya (`objId` ignored), 1 = item. */
  getItem(objId: number, count: number, mode: 0 | 1): void;
  /** Never called (the legacy client has no way to reorder the bank). */
  moveItem(src: number, dst: number): void;
  /** `SendReqGuildBankLogList( kind )`. */
  logView(kind: number): void;
  /** `SendDeclWar( idMaster, name )`. */
  declWar(targetName: string): void;
  /** `SendAcptWar( idMaster, idDecl )`. */
  acptWar(idDeclGuild: number): void;
  /** `SendSurrender( idPlayer )`. */
  surrender(): void;
  /** `SendQueryTruce( idPlayer )`. */
  queryTruce(): void;
  /** `SendAcptTruce( idPlayer )`. */
  acptTruce(): void;
}

export function guildSenders(send: Send): GuildSenders {
  return {
    invite(targetObj) {
      // legacy-send: SendGuildInvite
      send({ case: 'guildInviteReq', value: create(GuildInviteReqSchema, { targetObj }) });
    },
    ignoreInvite(inviter) {
      send({ case: 'ignoreGuildInvite', value: create(IgnoreGuildInviteSchema, { inviter }) });
    },
    addMember(inviter) {
      // legacy-send: SendAddGuildMember
      send({ case: 'addGuildMemberReq', value: create(AddGuildMemberReqSchema, { inviter }) });
    },
    removeMember(idPlayer) {
      // legacy-send: SendRemoveGuildMember
      send({
        case: 'removeGuildMemberReq',
        value: create(RemoveGuildMemberReqSchema, { idPlayer }),
      });
    },
    destroy() {
      // legacy-send: SendDestroyGuild
      send({ case: 'destroyGuildReq', value: create(DestroyGuildReqSchema, {}) });
    },
    memberLevel(idPlayer, rank) {
      // legacy-send: SendGuildMemberLv
      send({
        case: 'guildMemberLevelReq',
        value: create(GuildMemberLevelReqSchema, { idPlayer, rank }),
      });
    },
    guildClass(idPlayer, up) {
      // legacy-send: SendGuildClass
      send({ case: 'guildClassReq', value: create(GuildClassReqSchema, { up, idPlayer }) });
    },
    nickname(idPlayer, alias) {
      // legacy-send: SendGuildNickName
      send({
        case: 'guildNicknameReq',
        value: create(GuildNicknameReqSchema, { idPlayer, alias }),
      });
    },
    chgMaster(newMaster) {
      // legacy-send: SendChgMaster
      send({ case: 'chgMasterReq', value: create(ChgMasterReqSchema, { newMaster }) });
    },
    authority(power) {
      // legacy-send: SendGuildAuthority
      send({
        case: 'guildAuthorityReq',
        value: create(GuildAuthorityReqSchema, { power: [...power] }),
      });
    },
    penya(rank, penya) {
      // legacy-send: SendGuilPenya
      send({ case: 'guildPenyaReq', value: create(GuildPenyaReqSchema, { rank, penya }) });
    },
    setName(name) {
      // legacy-send: SendGuildSetName
      send({ case: 'guildSetNameReq', value: create(GuildSetNameReqSchema, { name }) });
    },
    logo(logo) {
      // legacy-send: SendGuildLogo
      send({ case: 'guildLogoReq', value: create(GuildLogoReqSchema, { logo }) });
    },
    contribution(gold, itemFlag) {
      // legacy-send: SendGuildContribution
      send({
        case: 'guildContributionReq',
        value: create(GuildContributionReqSchema, { gold, itemFlag }),
      });
    },
    notice(notice) {
      // legacy-send: SendGuildNotice
      send({ case: 'guildNoticeReq', value: create(GuildNoticeReqSchema, { notice }) });
    },
    createCloak() {
      // legacy-send: SendCreateGuildCloak
      send({ case: 'createGuildCloak', value: create(CreateGuildCloakSchema, {}) });
    },
    querySetName(itemObjId, name) {
      // legacy-send: SendQuerySetGuildName
      send({
        case: 'querySetGuildName',
        value: create(QuerySetGuildNameSchema, { itemObjId, name }),
      });
    },
    openBank() {
      // legacy-send: SendOpenGuildBankWnd
      send({ case: 'guildBankWndReq', value: create(GuildBankWndReqSchema, {}) });
    },
    closeBank() {
      // legacy-send: SendCloseGuildBankWnd
      send({ case: 'guildBankWndClose', value: create(GuildBankWndCloseSchema, {}) });
    },
    putItem(objId, count) {
      // legacy-send: SendPutItemGuildBank
      send({
        case: 'putItemGuildBankReq',
        value: create(PutItemGuildBankReqSchema, { objId, count, mode: 1 }),
      });
    },
    getItem(objId, count, mode) {
      // legacy-send: SendGetItemGuildBank
      send({
        case: 'getItemGuildBankReq',
        value: create(GetItemGuildBankReqSchema, { objId, countOrGold: count, mode }),
      });
    },
    moveItem(src, dst) {
      send({ case: 'guildBankMoveItem', value: create(GuildBankMoveItemSchema, { src, dst }) });
    },
    logView(kind) {
      // legacy-send: SendReqGuildBankLogList
      send({ case: 'guildLogViewReq', value: create(GuildLogViewReqSchema, { kind }) });
    },
    declWar(targetName) {
      // legacy-send: SendDeclWar
      send({ case: 'declGuildWarReq', value: create(DeclGuildWarReqSchema, { targetName }) });
    },
    acptWar(idDeclGuild) {
      // legacy-send: SendAcptWar
      send({ case: 'acptGuildWarReq', value: create(AcptGuildWarReqSchema, { idDeclGuild }) });
    },
    surrender() {
      // legacy-send: SendSurrender
      send({ case: 'surrenderReq', value: create(SurrenderReqSchema, {}) });
    },
    queryTruce() {
      // legacy-send: SendQueryTruce
      send({ case: 'queryTruceReq', value: create(QueryTruceReqSchema, {}) });
    },
    acptTruce() {
      // legacy-send: SendAcptTruce
      send({ case: 'acptTruce', value: create(AcptTruceSchema, {}) });
    },
  };
}
