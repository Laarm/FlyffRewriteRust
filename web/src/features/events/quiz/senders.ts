// The sends of the quiz (rule C9: `// legacy-send: <SendFn>`): `DPClient.cpp:19040-19050`. The session identifies the sender (VR-001).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import { QuizEntranceSchema, QuizTeleportSchema } from '../../../gen/proto/flyff/v1/event_pb';

type Send = (msg: Parameters<ClientContext['net']['sender']['send']>[0]) => unknown;

export interface QuizSenders {
  /** `SendQuizEventEntrance()`: the Yes of the entrance confirm. */
  entrance(): void;
  /** `SendQuizEventTeleport( zone )`: 100 the quiz zone, 101-104 an answer, 106 the exit. */
  teleport(zone: number): void;
}

export function quizSenders(send: Send): QuizSenders {
  return {
    entrance() {
      // legacy-send: SendQuizEventEntrance
      send({ case: 'quizEntrance', value: create(QuizEntranceSchema, {}) });
    },
    teleport(zone) {
      // legacy-send: SendQuizEventTeleport
      send({ case: 'quizTeleport', value: create(QuizTeleportSchema, { zone }) });
    },
  };
}
