// The sends of the party windows, the player menu and the party finder (rule C9). The ids on the wire are character ids (`m_idPlayer`); the
// session identifies the sender (VR-001), so the leader / own-id fields of the legacy packets are gone.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import { Vec3Schema } from '../../gen/proto/flyff/v1/common_pb';
import { SummonPartyConfirmSchema, SummonPartySchema } from '../../gen/proto/flyff/v1/messenger_pb';
import {
  AddPartyMemberSchema,
  ChanPartyNameSchema,
  ChangeTroupSchema,
  JoinPartyReqSchema,
  MemberRequestCancelSchema,
  MemberRequestSchema,
  PartyAllowJoinReqSchema,
  PartyChangeExpModeReqSchema,
  PartyChangeItemModeReqSchema,
  PartyChangeLeaderSchema,
  PartySkillUseSchema,
  RemovePartyMemberSchema,
  SetNaviPointReqSchema,
} from '../../gen/proto/flyff/v1/party_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders over one client context. */
export interface PartySenders {
  /** `SendPartyMemberRequest(g_pPlayer, member, bTroup)`: invite a player. */
  memberRequest(memberId: number, troupe?: boolean): void;
  /** `SendPartyMemberCancle(leader, member, mode)`: decline an invitation (0 declined, 3 the player refuses invitations). */
  memberRequestCancel(leaderId: number, mode: number): void;
  /** `SendAddPartyMember(leader, ...)`: accept an invitation, or join from the finder list. */
  addPartyMember(leaderId: number): void;
  /** `SendRemovePartyMember(leader, member)`: leave (own id) or kick. */
  removePartyMember(memberId: number): void;
  /** `SendChangeTroup(TRUE, name)`. */
  changeTroup(name: string): void;
  /** `SendChangePartyName(name)`. */
  chanPartyName(name: string): void;
  /** `SendChangeShareItem(mode)`: 0 finder, 1 round robin, 2 leader, 3 random. */
  changeItemMode(mode: number): void;
  /** `SendChangeShareExp(mode)`: 0 level, 1 contribution. */
  changeExpMode(mode: number): void;
  /** `SendPartyChangeLeader(leader, new)`. */
  changeLeader(newLeader: number): void;
  /** `SendPartySkillUse(skill)`. */
  skillUse(skill: number): void;
  /** `JoinParty()`: ask for the list of the recruiting parties. */
  joinPartyReq(): void;
  /** `SendPartyJoinAllow(mode)`. */
  allowJoin(allow: boolean): void;
  /** `SendSetNaviPoint(pos, objidTarget)`. */
  naviPoint(pos: readonly [number, number, number], target: number): void;
  /** `SendSummonParty(dwData)`. */
  summonParty(data: number): void;
  /** `SendSummonPartyConfirm(objid, dwData)`. */
  summonPartyConfirm(leaderObj: number, data: number): void;
}

export function partySenders(send: Send): PartySenders {
  return {
    memberRequest(memberId, troupe = false) {
      // legacy-send: SendPartyMemberRequest
      send({ case: 'memberRequest', value: create(MemberRequestSchema, { memberId, troupe }) });
    },
    memberRequestCancel(leaderId, mode) {
      // legacy-send: SendPartyMemberCancle
      send({
        case: 'memberRequestCancel',
        value: create(MemberRequestCancelSchema, { leaderId, mode }),
      });
    },
    addPartyMember(leaderId) {
      // legacy-send: SendAddPartyMember
      send({ case: 'addPartyMember', value: create(AddPartyMemberSchema, { leaderId }) });
    },
    removePartyMember(memberId) {
      // legacy-send: SendRemovePartyMember
      send({ case: 'removePartyMember', value: create(RemovePartyMemberSchema, { memberId }) });
    },
    changeTroup(name) {
      // legacy-send: SendChangeTroup
      send({ case: 'changeTroup', value: create(ChangeTroupSchema, { name }) });
    },
    chanPartyName(name) {
      // legacy-send: SendChangePartyName
      send({ case: 'chanPartyName', value: create(ChanPartyNameSchema, { name }) });
    },
    changeItemMode(mode) {
      // legacy-send: SendChangeShareItem
      send({
        case: 'partyChangeItemModeReq',
        value: create(PartyChangeItemModeReqSchema, { mode }),
      });
    },
    changeExpMode(mode) {
      // legacy-send: SendChangeShareExp
      send({
        case: 'partyChangeExpModeReq',
        value: create(PartyChangeExpModeReqSchema, { mode }),
      });
    },
    changeLeader(newLeader) {
      // legacy-send: SendPartyChangeLeader
      send({ case: 'partyChangeLeader', value: create(PartyChangeLeaderSchema, { newLeader }) });
    },
    skillUse(skill) {
      // legacy-send: SendPartySkillUse
      send({ case: 'partySkillUse', value: create(PartySkillUseSchema, { skill }) });
    },
    joinPartyReq() {
      // legacy-send: JoinParty
      send({ case: 'joinPartyReq', value: create(JoinPartyReqSchema, {}) });
    },
    allowJoin(allow) {
      // legacy-send: SendPartyJoinAllow
      send({
        case: 'partyAllowJoinReq',
        value: create(PartyAllowJoinReqSchema, { mode: allow ? 1 : 0 }),
      });
    },
    naviPoint(pos, target) {
      // legacy-send: SendSetNaviPoint
      send({
        case: 'setNaviPointReq',
        value: create(SetNaviPointReqSchema, {
          pos: create(Vec3Schema, { x: pos[0], y: pos[1], z: pos[2] }),
          target,
        }),
      });
    },
    summonParty(data) {
      // legacy-send: SendSummonParty
      send({ case: 'summonParty', value: create(SummonPartySchema, { data }) });
    },
    summonPartyConfirm(leaderObj, data) {
      // legacy-send: SendSummonPartyConfirm
      send({
        case: 'summonPartyConfirm',
        value: create(SummonPartyConfirmSchema, { leaderObj, data }),
      });
    },
  };
}
