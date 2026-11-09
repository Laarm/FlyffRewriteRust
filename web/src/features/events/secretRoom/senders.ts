// The sends of the secret-room windows and the secret-room NPC menu (rule C9: `// legacy-send: <SendFn>`). Nine of the eleven functions of WP-CFEAT-52 (`SendTaxRate` is `../tax/senders.ts`,
// `SendTeleportToHeavenTower` is `../heavenTower`). The session identifies the sender (VR-001).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  SecretRoomEntranceSchema,
  SecretRoomLineUpMemberSchema,
  SecretRoomLineUpOpenWndReqSchema,
  SecretRoomTeleportToDungeonSchema,
  SecretRoomTeleportToNpcSchema,
  SecretRoomTenderCancelReturnSchema,
  SecretRoomTenderOpenWndReqSchema,
  SecretRoomTenderSchema,
  SecretRoomTenderViewReqSchema,
} from '../../../gen/proto/flyff/v1/secret_room_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders of the secret room over one client context. */
export interface SecretRoomSenders {
  /** `SendSecretRoomTenderOpenWnd()`: ask for the bid window (menu 84, the apply button of the status window). */
  tenderOpenWnd(): void;
  /** `SendSecretRoomTender( penya )`: the confirmed bid. */
  tender(penya: number): void;
  /** `SendSecretRoomTenderCancelReturn()`: withdraw the bid (80 percent back). */
  tenderCancelReturn(): void;
  /** `SendSecretRoomTenderView()`: ask for the bid ranking (menu 85, the refresh button). */
  tenderView(): void;
  /** `SendSecretRoomLineUpOpenWnd()`: ask for the line-up window (menu 86). */
  lineUpOpenWnd(): void;
  /** `SendSecretRoomLineUpMember( ids )`: submit the line-up. */
  lineUpMember(ids: readonly number[]): void;
  /** `SendSecretRoomEntrance()`: enter the secret room (menu 87). */
  entrance(): void;
  /** `SendSecretRoomTeleportToNPC()`: go to the revival position of the side (the info box). */
  teleportToNpc(): void;
  /** `SendTeleportToSecretRoomDungeon()`: the owner guild enters the hunting ground (menu 100). */
  teleportToDungeon(): void;
}

export function secretRoomSenders(send: Send): SecretRoomSenders {
  return {
    tenderOpenWnd() {
      // legacy-send: SendSecretRoomTenderOpenWnd
      send({
        case: 'secretRoomTenderOpenWndReq',
        value: create(SecretRoomTenderOpenWndReqSchema),
      });
    },
    tender(penya) {
      // legacy-send: SendSecretRoomTender
      send({ case: 'secretRoomTender', value: create(SecretRoomTenderSchema, { penya }) });
    },
    tenderCancelReturn() {
      // legacy-send: SendSecretRoomTenderCancelReturn
      send({
        case: 'secretRoomTenderCancelReturn',
        value: create(SecretRoomTenderCancelReturnSchema),
      });
    },
    tenderView() {
      // legacy-send: SendSecretRoomTenderView
      send({ case: 'secretRoomTenderViewReq', value: create(SecretRoomTenderViewReqSchema) });
    },
    lineUpOpenWnd() {
      // legacy-send: SendSecretRoomLineUpOpenWnd
      send({
        case: 'secretRoomLineUpOpenWndReq',
        value: create(SecretRoomLineUpOpenWndReqSchema),
      });
    },
    lineUpMember(ids) {
      // legacy-send: SendSecretRoomLineUpMember
      send({
        case: 'secretRoomLineUpMember',
        value: create(SecretRoomLineUpMemberSchema, { ids: [...ids] }),
      });
    },
    entrance() {
      // legacy-send: SendSecretRoomEntrance
      send({ case: 'secretRoomEntrance', value: create(SecretRoomEntranceSchema) });
    },
    teleportToNpc() {
      // legacy-send: SendSecretRoomTeleportToNPC
      send({ case: 'secretRoomTeleportToNpc', value: create(SecretRoomTeleportToNpcSchema) });
    },
    teleportToDungeon() {
      // legacy-send: SendTeleportToSecretRoomDungeon
      send({
        case: 'secretRoomTeleportToDungeon',
        value: create(SecretRoomTeleportToDungeonSchema),
      });
    },
  };
}
