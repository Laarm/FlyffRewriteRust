// The sends of the 1:1 guild combat (rule C9: `// legacy-send: <SendFn>`): the nine functions of WP-CFEAT-45 (`src:Neuz/DPClient.cpp` `SendGC1to1*`, ref/99 App. B spellings - sic
// `SendGC1to1MemberLienUp*`) plus the arena pair `SendHdr( PACKETTYPE_ARENA_ENTER / _EXIT )` (`WndWorld.cpp:6118-6123`, the csv row is `done`, the marker adds the call site).
// The session identifies the sender (VR-001).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  Gc1to1MemberLineUpOpenWndReqSchema,
  Gc1to1MemberLineUpSchema,
  Gc1to1TeleportToNpcSchema,
  Gc1to1TeleportToStageSchema,
  Gc1to1TenderCancelSchema,
  Gc1to1TenderFailedSchema,
  Gc1to1TenderOpenWndReqSchema,
  Gc1to1TenderSchema,
  Gc1to1TenderViewReqSchema,
} from '../../../gen/proto/flyff/v1/guild_siege_pb';
import { ArenaEnterSchema, ArenaExitSchema } from '../../../gen/proto/flyff/v1/npc_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders over one client context. */
export interface Gc1to1Senders {
  /** `SendGC1to1TenderOpenWnd()`: the tender window (menu 60, the application status window's button). */
  tenderOpenWnd(): void;
  /** `SendGC1to1TenderView()`: the ranking (menu 62, the refresh button). */
  tenderView(): void;
  /** `SendGC1to1Tender( penya )`. */
  tender(penya: number): void;
  /** `SendGC1to1TenderCancel()`. */
  tenderCancel(): void;
  /** `SendGC1to1TenderFailed()`: the refund of a guild that was not admitted (menu 70). */
  tenderFailed(): void;
  /** `SendGC1to1MemberLienUpOpenWnd()` (menu 63). */
  memberLineUpOpenWnd(): void;
  /** `SendGC1to1MemberLienUp( ids )`. */
  memberLineUp(ids: readonly number[]): void;
  /** `SendGC1to1TeleportToNPC()`: the info box of the entrance state. */
  teleportToNpc(): void;
  /** `SendGC1to1TeleportToStage()` (menu 64, the join confirm). */
  teleportToStage(): void;
  /** `SendHdr( PACKETTYPE_ARENA_ENTER )` (menu 80). */
  arenaEnter(): void;
  /** `SendHdr( PACKETTYPE_ARENA_EXIT )` (menu 81). */
  arenaExit(): void;
}

export function gc1to1Senders(send: Send): Gc1to1Senders {
  return {
    tenderOpenWnd() {
      // legacy-send: SendGC1to1TenderOpenWnd
      send({
        case: 'gc1to1TenderOpenWndReq',
        value: create(Gc1to1TenderOpenWndReqSchema, {}),
      });
    },
    tenderView() {
      // legacy-send: SendGC1to1TenderView
      send({ case: 'gc1to1TenderViewReq', value: create(Gc1to1TenderViewReqSchema, {}) });
    },
    tender(penya) {
      // legacy-send: SendGC1to1Tender
      send({ case: 'gc1to1Tender', value: create(Gc1to1TenderSchema, { penya }) });
    },
    tenderCancel() {
      // legacy-send: SendGC1to1TenderCancel
      send({ case: 'gc1to1TenderCancel', value: create(Gc1to1TenderCancelSchema, {}) });
    },
    tenderFailed() {
      // legacy-send: SendGC1to1TenderFailed
      send({ case: 'gc1to1TenderFailed', value: create(Gc1to1TenderFailedSchema, {}) });
    },
    memberLineUpOpenWnd() {
      // legacy-send: SendGC1to1MemberLienUpOpenWnd
      send({
        case: 'gc1to1MemberLineUpOpenWndReq',
        value: create(Gc1to1MemberLineUpOpenWndReqSchema, {}),
      });
    },
    memberLineUp(ids) {
      // legacy-send: SendGC1to1MemberLienUp
      send({
        case: 'gc1to1MemberLineUp',
        value: create(Gc1to1MemberLineUpSchema, { ids: [...ids] }),
      });
    },
    teleportToNpc() {
      // legacy-send: SendGC1to1TeleportToNPC
      send({ case: 'gc1to1TeleportToNpc', value: create(Gc1to1TeleportToNpcSchema, {}) });
    },
    teleportToStage() {
      // legacy-send: SendGC1to1TeleportToStage
      send({ case: 'gc1to1TeleportToStage', value: create(Gc1to1TeleportToStageSchema, {}) });
    },
    arenaEnter() {
      // legacy-send: SendHdr
      send({ case: 'arenaEnter', value: create(ArenaEnterSchema, {}) });
    },
    arenaExit() {
      // legacy-send: SendHdr
      send({ case: 'arenaExit', value: create(ArenaExitSchema, {}) });
    },
  };
}
