// The action-queue row of the taskbar: the editing mechanics (`CWndTaskBar::SetSkillQueue` / `RemoveSkillQueue`,
// `_Interface/WndTaskBar.cpp:1495-1520, 1833-1916`; Plan/42 §11.8, Plan/17 §6.13; WP-UI-42). The rules of the row (`CheckAddSkill`) and
// the `SkillTaskBar` message belong to 45 (`QueueActions`, WP-CFEAT-32); this class only does what the C++ did around them:
//
//   add(index, skill)    refused while the queue runs or holds five skills; a gap below the queue count clears the queue first (an
//                        error state of the original); `index` is clamped to the count; the skill needs a level and no cool-down
//                        (`TID_GAME_SKILLLNOTUSE`); an index below the count inserts and shifts right after
//                        `checkAddSkill(style, index)`, otherwise it appends after `checkAddSkill(style, count)`; the entry is
//                        `{SKILL, type, index, id, userId 0, data 2}`; one `send` of all five slots
//   remove(index)        refused while the queue runs; shifts left; the skill that was removed is asked again for its place
//                        (`checkAddSkill(removedStyle, index)`) and while that is refused the skill that moved into the place goes
//                        too; one `send`
//
// The model keeps the queue compact (`queueCount()` entries from slot 0 on), so a "gap" is only possible after a damaged snapshot.

import { TID_GAME_SKILLLNOTUSE } from '../../gen/defs/text';
import { SC, type Shortcut } from '../dnd/shortcut';
import { MAX_SLOT_QUEUE } from './layout';
import type { TaskbarModel } from './model';
import type { QueueActions, SkillDropInfo, TaskbarQueuePort } from './ports';

export interface ActionQueueDeps {
  readonly model: TaskbarModel;
  readonly actions: () => QueueActions | undefined;
  /** `g_pPlayer->GetSkill(0, id)` with its prop: the skill in job-skill slot `id`; `null` = unknown. */
  readonly skill: (id: number) => SkillDropInfo | null;
  /** The refusal line: `TID_GAME_SKILLLNOTUSE` with the skill name. */
  readonly say: (tid: number, ...args: string[]) => void;
}

/** The queue slots as the combo styles the rules read (`0` for an empty slot or an unknown skill). */
export function queueStyles(
  queue: readonly (Shortcut | null)[],
  skill: (id: number) => SkillDropInfo | null,
): number[] {
  return queue.map((sc) => (sc === null ? 0 : (skill(sc.id)?.combo ?? 0)));
}

export class ActionQueue implements TaskbarQueuePort {
  constructor(private readonly d: ActionQueueDeps) {}

  private get queue(): readonly (Shortcut | null)[] {
    return this.d.model.get().queue;
  }

  /** `m_nExecute != 0`. */
  private get running(): boolean {
    return this.d.model.get().executing !== 0;
  }

  /** `CheckAddSkill(style, dest)` over the queue as it is. */
  private check(style: number, dest: number): boolean {
    const actions = this.d.actions();
    if (!actions) return false;
    const q = this.queue;
    return actions.checkAddSkill(
      style,
      dest,
      queueStyles(q, this.d.skill),
      q.filter((e) => e !== null).length,
    );
  }

  private send(): void {
    this.d.actions()?.send(this.d.model.get().queue);
  }

  /** `SetSkillQueue(index, type, id, icon)`; `false` = refused (the forbid icon shows). */
  add(index: number, type: number, id: number, icon: string | null): boolean {
    const { model } = this.d;
    if (this.running) return false;
    let queue = [...model.get().queue];
    let count = queue.filter((e) => e !== null).length;
    if (count >= MAX_SLOT_QUEUE) return false;
    // an empty slot below the count is an error state: the queue starts over
    for (let i = 0; i < count; i += 1) {
      if (queue[i] === null || queue[i] === undefined) {
        queue = queue.map(() => null);
        model.setQueue(queue);
        count = 0;
        break;
      }
    }
    const at = Math.min(Math.max(0, Math.trunc(index)), count);
    const skill = this.d.skill(id);
    if (skill === null || skill.level <= 0) return false;
    if ((skill.cooldown ?? 0) > 0) {
      this.d.say(TID_GAME_SKILLLNOTUSE, skill.name ?? '');
      return false;
    }
    if (!this.check(skill.combo, at)) return false;
    // insert (shift right) or append
    const next: (Shortcut | null)[] = [...queue];
    for (let i = count; i > at; i -= 1) next[i] = next[i - 1] ?? null;
    const entry: Shortcut = {
      kind: SC.SKILL,
      id,
      type,
      index: at,
      userId: 0,
      data: 2,
      ...(icon !== null ? { icon } : {}),
    };
    next[at] = entry;
    model.setQueue(next);
    this.send();
    return true;
  }

  /** `RemoveSkillQueue(index, send)`. */
  remove(index: number, send = true): void {
    const { model } = this.d;
    if (this.running) return;
    const i = Math.trunc(index);
    if (i < 0 || i >= MAX_SLOT_QUEUE) return;
    // `goto BACK`: the skill that moved into the place is asked again (bounded by the five slots)
    for (let guard = 0; guard < MAX_SLOT_QUEUE; guard += 1) {
      const q = [...model.get().queue];
      const removed = q[i] ?? null;
      if (removed === null) break;
      const style = this.d.skill(removed.id)?.combo ?? 0;
      for (let k = i; k < MAX_SLOT_QUEUE - 1; k += 1) q[k] = q[k + 1] ?? null;
      q[MAX_SLOT_QUEUE - 1] = null;
      model.setQueue(q);
      if (this.check(style, i)) break;
    }
    if (send) this.send();
  }
}
