// The action-queue rules as 42's `QueueActions` (WP-CFEAT-32): `checkAddSkill` and the `SkillTaskBar` send (`SendSkillTaskBar`, all five
// queue slots, `data` 2, no text).

import { create } from '@bufbuild/protobuf';
import type { ClientContext } from '../../../app/context';
import {
  type SkillTaskBar,
  SkillTaskBarSchema,
  TaskBarQueueSlotSchema,
} from '../../../gen/proto/flyff/v1/taskbar_pb';
import { SC, type Shortcut } from '../../../ui/dnd/shortcut';
import type { QueueActions } from '../../../ui/taskbar/ports';
import { shortcutToWire } from '../../taskbar/wire';
import { checkAddSkill, QUEUE_SLOTS } from './checkAddSkill';

/**
 * The `SkillTaskBar` message of a queue: ALWAYS five entries, slot 0..4 in order; an empty slot is sent as a `SHORTCUT_NONE` so that
 * the server clears it ("slots not listed keep their old value", ref/07a section 13).
 */
export function skillTaskBarOf(queue: readonly (Shortcut | null)[]): SkillTaskBar {
  return create(SkillTaskBarSchema, {
    slots: Array.from({ length: QUEUE_SLOTS }, (_, i) =>
      create(TaskBarQueueSlotSchema, {
        slot: i,
        shortcut: shortcutToWire(
          queue[i] ?? { kind: SC.NONE, id: 0, type: 0, index: i, userId: 0, data: 2 },
        ),
      }),
    ),
  });
}

/** 42's `QueueActions` over the client's sender. */
export function queueActions(ctx: ClientContext): QueueActions {
  return {
    checkAddSkill,
    send(queue) {
      // legacy-send: SendSkillTaskBar
      ctx.net.sender.send({ case: 'skillTaskBar', value: skillTaskBarOf(queue) });
    },
  };
}
