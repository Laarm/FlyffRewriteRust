// The sends of the lord windows (rule C9: `// legacy-send: <SendFn>`). Exactly the five functions of WP-CFEAT-43 (`src:Neuz/DPClient.cpp:18016-18110`). The session identifies the sender (VR-001), so the
// leading own-id fields of the legacy packets are gone.

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  ElectionAddDepositReqSchema,
  ElectionIncVoteReqSchema,
  ElectionSetPledgeReqSchema,
  LEventCreateReqSchema,
  LordSkillUseReqSchema,
} from '../../../gen/proto/flyff/v1/lord_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

/** The senders over one client context. */
export interface LordSenders {
  /** `SendElectionAddDeposit( iDeposit )`: the NEW TOTAL deposit (at least 100 000 000, above the current one). */
  addDeposit(total: number): void;
  /** `SendElectionSetPledge( szPledge )`: the pledge, at most 255 bytes (the server cuts at the same limit). */
  setPledge(pledge: string): void;
  /** `SendElectionIncVote( idPlayer )`: the character id of the candidate. */
  incVote(candidate: number): void;
  /** `SendLEventCreate( iEEvent, iIEvent )`: the chosen rows of the two rate lists. */
  createEvent(eIndex: number, iIndex: number): void;
  /** `SendLordSkillUse( nSkill, szTarget )`: the skill id 0..7 and the name of the target (ignored for a skill on oneself). */
  useSkill(skill: number, targetName: string): void;
}

export function lordSenders(send: Send): LordSenders {
  return {
    addDeposit(total) {
      // legacy-send: SendElectionAddDeposit
      send({
        case: 'electionAddDepositReq',
        value: create(ElectionAddDepositReqSchema, { total: BigInt(Math.trunc(total)) }),
      });
    },
    setPledge(pledge) {
      // legacy-send: SendElectionSetPledge
      send({
        case: 'electionSetPledgeReq',
        value: create(ElectionSetPledgeReqSchema, { pledge }),
      });
    },
    incVote(candidate) {
      // legacy-send: SendElectionIncVote
      send({
        case: 'electionIncVoteReq',
        value: create(ElectionIncVoteReqSchema, { candidate }),
      });
    },
    createEvent(eIndex, iIndex) {
      // legacy-send: SendLEventCreate
      send({
        case: 'lEventCreateReq',
        value: create(LEventCreateReqSchema, { eIndex, iIndex }),
      });
    },
    useSkill(skill, targetName) {
      // legacy-send: SendLordSkillUse
      send({
        case: 'lordSkillUseReq',
        value: create(LordSkillUseReqSchema, { skill, targetName }),
      });
    },
  };
}
