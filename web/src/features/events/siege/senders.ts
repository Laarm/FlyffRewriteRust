// The sends of the siege windows and the siege NPC menu (rule C9: `// legacy-send: <SendFn>`). Exactly the 12 functions of WP-CFEAT-41 (`SendGCGetItem` is inactive in V15). The session
// identifies the sender (VR-001), so the leading own-id fields of the legacy packets are gone.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  GetPenyaGuildGuildCombatSchema,
  GetPenyaPlayerGuildCombatSchema,
  InGuildCombat_GcInAppSchema,
  InGuildCombat_GcInWindowSchema,
  InGuildCombatSchema,
  JoinGuildCombatSchema,
  OutGuildCombatSchema,
  PlayerPointGuildCombatSchema,
  RequestStatusSchema,
  SelectMapGuildCombatSchema,
  SelectPlayerGuildCombatSchema,
  TeleGuildCombatSchema,
} from '../../../gen/proto/flyff/v1/guild_siege_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders over one client context. */
export interface SiegeSenders {
  /** `SendGuildCombatWindow()`: ask for the bid window (`MMI_GUILDWAR_APP`, the apply button of the state window). */
  window(): void;
  /** `SendGuildCombatApp( penya )`: the confirmed bid. */
  app(penya: number): void;
  /** `SendGuildCombatCancel()`: withdraw the application. */
  cancel(): void;
  /** `SendGCRequestStatusWindow()`: ask for the list of the applying guilds. */
  requestStatus(): void;
  /** `SendGCSelectPlayerWindow()`: ask for the line-up editor. */
  selectPlayerWindow(): void;
  /** `SendGCSelectPlayer( vecSelectPlayer, uidDefender )`: submit the line-up. */
  selectPlayer(defender: number, ids: readonly number[]): void;
  /** `SendGCSelectMap( map )`: the entry gate 0..3, 99 = random. */
  selectMap(map: number): void;
  /** `SendGCJoin()`: enter the siege map. */
  join(): void;
  /** `SendGCTele()`: leave the siege map for Madrigal. */
  tele(): void;
  /** `SendGCGetPenyaGuild()`: the master claims the prize or the refund. */
  getPenyaGuild(): void;
  /** `SendGCGetPenyaPlayer()`: the best player claims the prize. */
  getPenyaPlayer(): void;
  /** `SendGCPlayerPoint()`: ask for the cross-siege player ranking. */
  playerPoint(): void;
}

export function siegeSenders(send: Send): SiegeSenders {
  return {
    window() {
      // legacy-send: SendGuildCombatWindow
      send({
        case: 'inGuildCombat',
        value: create(InGuildCombatSchema, {
          sub: { case: 'gcInWindow', value: create(InGuildCombat_GcInWindowSchema) },
        }),
      });
    },
    app(penya) {
      // legacy-send: SendGuildCombatApp
      send({
        case: 'inGuildCombat',
        value: create(InGuildCombatSchema, {
          sub: { case: 'gcInApp', value: create(InGuildCombat_GcInAppSchema, { penya }) },
        }),
      });
    },
    cancel() {
      // legacy-send: SendGuildCombatCancel
      send({ case: 'outGuildCombat', value: create(OutGuildCombatSchema) });
    },
    requestStatus() {
      // legacy-send: SendGCRequestStatusWindow
      send({ case: 'requestStatus', value: create(RequestStatusSchema) });
    },
    selectPlayerWindow() {
      // legacy-send: SendGCSelectPlayerWindow
      send({
        case: 'selectPlayerGuildCombat',
        value: create(SelectPlayerGuildCombatSchema, { window: true }),
      });
    },
    selectPlayer(defender, ids) {
      // legacy-send: SendGCSelectPlayer
      send({
        case: 'selectPlayerGuildCombat',
        value: create(SelectPlayerGuildCombatSchema, { window: false, defender, ids: [...ids] }),
      });
    },
    selectMap(map) {
      // legacy-send: SendGCSelectMap
      send({ case: 'selectMapGuildCombat', value: create(SelectMapGuildCombatSchema, { map }) });
    },
    join() {
      // legacy-send: SendGCJoin
      send({ case: 'joinGuildCombat', value: create(JoinGuildCombatSchema) });
    },
    tele() {
      // legacy-send: SendGCTele
      send({ case: 'teleGuildCombat', value: create(TeleGuildCombatSchema) });
    },
    getPenyaGuild() {
      // legacy-send: SendGCGetPenyaGuild
      send({
        case: 'getPenyaGuildGuildCombat',
        value: create(GetPenyaGuildGuildCombatSchema),
      });
    },
    getPenyaPlayer() {
      // legacy-send: SendGCGetPenyaPlayer
      send({
        case: 'getPenyaPlayerGuildCombat',
        value: create(GetPenyaPlayerGuildCombatSchema),
      });
    },
    playerPoint() {
      // legacy-send: SendGCPlayerPoint
      send({
        case: 'playerPointGuildCombat',
        value: create(PlayerPointGuildCombatSchema),
      });
    },
  };
}
