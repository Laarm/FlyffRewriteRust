// The sends of the couple, campus, honor and cheer features (rule C9; ref/99 App. B). The session identifies the sender (VR-001), so the own-id fields of the legacy packets are gone.
// `SendQueryPlayerData` (the campus invitation also asks for the target) is the player-data cache's own send.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../app/context';
import {
  CampusAcceptSchema,
  CampusInviteReqSchema,
  CampusRefuseSchema,
  CampusRemoveMemberSchema,
  CheeringSchema,
  CoupleReqSchema,
  DecoupleSchema,
  HonorChangeReqSchema,
  HonorListReqSchema,
  ProposeSchema,
  RefuseSchema,
} from '../../gen/proto/flyff/v1/relation_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

export interface RelationSenders {
  /** `SendPropose( name )`: propose to the player with that name. */
  propose(targetName: string): void;
  /** `SendRefuse()`: decline the pending proposal. */
  refuse(): void;
  /** `SendCouple()`: accept the pending proposal (the packet carries nothing; VR-044). */
  couple(): void;
  /** `SendDecouple()`. */
  decouple(): void;
  /** `SendInviteCampusMember( idTarget )`. */
  inviteCampusMember(target: number): void;
  /** `SendAcceptCampusMember( idRequester )`. */
  acceptCampusMember(requester: number): void;
  /** `SendRefuseCampusMember( idRequester )`. */
  refuseCampusMember(requester: number): void;
  /** `SendRemoveCampusMember( idTarget )`. */
  removeCampusMember(target: number): void;
  /** `SendReqHonorList()`. */
  reqHonorList(): void;
  /** `SendReqHonorTitleChange( nChange )`: a title id, `-1` for none. */
  reqHonorTitleChange(id: number): void;
  /** `SendCheering( objid )`: cheer the player with that object id. */
  cheering(targetObj: number): void;
}

export function relationSenders(send: Send): RelationSenders {
  return {
    propose(targetName) {
      // legacy-send: SendPropose
      send({ case: 'propose', value: create(ProposeSchema, { targetName }) });
    },
    refuse() {
      // legacy-send: SendRefuse
      send({ case: 'refuse', value: create(RefuseSchema, {}) });
    },
    couple() {
      // legacy-send: SendCouple
      send({ case: 'coupleReq', value: create(CoupleReqSchema, {}) });
    },
    decouple() {
      // legacy-send: SendDecouple
      send({ case: 'decouple', value: create(DecoupleSchema, {}) });
    },
    inviteCampusMember(target) {
      // legacy-send: SendInviteCampusMember
      send({ case: 'campusInviteReq', value: create(CampusInviteReqSchema, { target }) });
    },
    acceptCampusMember(requester) {
      // legacy-send: SendAcceptCampusMember
      send({ case: 'campusAccept', value: create(CampusAcceptSchema, { requester }) });
    },
    refuseCampusMember(requester) {
      // legacy-send: SendRefuseCampusMember
      send({ case: 'campusRefuse', value: create(CampusRefuseSchema, { requester }) });
    },
    removeCampusMember(target) {
      // legacy-send: SendRemoveCampusMember
      send({ case: 'campusRemoveMember', value: create(CampusRemoveMemberSchema, { target }) });
    },
    reqHonorList() {
      // legacy-send: SendReqHonorList
      send({ case: 'honorListReq', value: create(HonorListReqSchema, {}) });
    },
    reqHonorTitleChange(id) {
      // legacy-send: SendReqHonorTitleChange
      send({ case: 'honorChangeReq', value: create(HonorChangeReqSchema, { id }) });
    },
    cheering(targetObj) {
      // legacy-send: SendCheering
      send({ case: 'cheering', value: create(CheeringSchema, { targetObj }) });
    },
  };
}
