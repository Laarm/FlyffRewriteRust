// The client model of the taskbar (Plan/42 §11.2, `taskbar/model.ts`): 18 applet slots, 8 pages of 9 item slots, the
// 5-slot action queue, the action points and the (client-only) page. The server persists the same state
// (`TaskBar` in the join batch, the four slot messages); the client applies every change locally at once, there is no
// echo (§11.6).

import { SC, type Shortcut } from '../dnd/shortcut';
import { ControlState } from '../wnd/observable';
import { MAX_SLOT_APPLET, MAX_SLOT_ITEM, MAX_SLOT_ITEM_COUNT, MAX_SLOT_QUEUE } from './layout';

export interface TaskbarState {
  readonly applets: readonly (Shortcut | null)[];
  /** 8 pages x 9 slots. */
  readonly items: readonly (readonly (Shortcut | null)[])[];
  /** 5 slots, compact (no gaps below `queueCount`). */
  readonly queue: readonly (Shortcut | null)[];
  readonly actionPoints: number;
  /** `0..7`, client-only (not persisted, starts at 0). */
  readonly page: number;
  /** `m_nExecute` (17's runtime writes it): 0 idle, 1 / 2 executing. */
  readonly executing: 0 | 1 | 2;
  /** The queue index being executed (`m_nUsedSkillQueue`; meaningful while `executing` is not 0). */
  readonly queueIndex: number;
}

const emptyRow = (n: number): (Shortcut | null)[] => Array.from({ length: n }, () => null);

export function emptyTaskbarState(): TaskbarState {
  return {
    applets: emptyRow(MAX_SLOT_APPLET),
    items: Array.from({ length: MAX_SLOT_ITEM_COUNT }, () => emptyRow(MAX_SLOT_ITEM)),
    queue: emptyRow(MAX_SLOT_QUEUE),
    actionPoints: 0,
    page: 0,
    executing: 0,
    queueIndex: 0,
  };
}

/** The wire part of one slot of the `TaskBar` message (`Shortcut` of 11 §7.3 with its slot). */
export interface WireSlot {
  readonly index: number;
  readonly shortcut: Shortcut;
}

/** What the `TaskBar` message carries (S->C: join batch, job change, skill reset). */
export interface TaskBarSnapshot {
  readonly applets: readonly WireSlot[];
  readonly items: readonly { page: number; slot: WireSlot }[];
  readonly queue: readonly WireSlot[];
  readonly actionPoints: number;
}

/** `CanQueue`-style filter: applet-row SKILL entries whose skill is not a combo style are emptied locally (not sent). */
export type SkillComboCheck = (type: number, id: number) => boolean;

/** The observable taskbar state (a `world`-scoped store of 40). */
export class TaskbarModel {
  readonly state = new ControlState<TaskbarState>(emptyTaskbarState());

  get(): TaskbarState {
    return this.state.get();
  }

  /** `InitTaskBar` + load: replaces the whole state; every entry's `index` becomes its slot. */
  replaceAll(snap: TaskBarSnapshot, comboOk?: SkillComboCheck): void {
    const next = emptyTaskbarState();
    const applets = [...next.applets];
    for (const s of snap.applets) {
      if (s.index < 0 || s.index >= MAX_SLOT_APPLET) continue;
      const sc = s.shortcut;
      // applet-row SKILL entries that are not combo-style skills are emptied locally (KEEP, not sent)
      if (sc.kind === SC.SKILL && comboOk && !comboOk(sc.type, sc.id)) continue;
      applets[s.index] = { ...sc, index: s.index };
    }
    const items = next.items.map((p) => [...p]);
    for (const { page, slot } of snap.items) {
      if (page < 0 || page >= MAX_SLOT_ITEM_COUNT) continue;
      if (slot.index < 0 || slot.index >= MAX_SLOT_ITEM) continue;
      const row = items[page];
      if (row) row[slot.index] = { ...slot.shortcut, index: slot.index };
    }
    const queue = [...next.queue];
    for (const s of snap.queue) {
      if (s.index < 0 || s.index >= MAX_SLOT_QUEUE) continue;
      queue[s.index] = { ...s.shortcut, index: s.index };
    }
    this.state.set({
      ...next,
      applets,
      items,
      queue,
      actionPoints: snap.actionPoints,
      page: this.get().page,
    });
  }

  /** `queueCount`: the number of queue entries. */
  queueCount(): number {
    return this.get().queue.filter((q) => q !== null).length;
  }

  setApplet(slot: number, sc: Shortcut | null): void {
    if (slot < 0 || slot >= MAX_SLOT_APPLET) return;
    const applets = [...this.get().applets];
    applets[slot] = sc ? { ...sc, index: slot } : null;
    this.state.set({ ...this.get(), applets });
  }

  setItem(page: number, slot: number, sc: Shortcut | null): void {
    if (page < 0 || page >= MAX_SLOT_ITEM_COUNT || slot < 0 || slot >= MAX_SLOT_ITEM) return;
    const items = this.get().items.map((p) => [...p]);
    const row = items[page];
    if (row) row[slot] = sc ? { ...sc, index: slot } : null;
    this.state.set({ ...this.get(), items });
  }

  setQueue(queue: readonly (Shortcut | null)[]): void {
    const next = emptyRow(MAX_SLOT_QUEUE);
    queue.slice(0, MAX_SLOT_QUEUE).forEach((q, i) => {
      next[i] = q ? { ...q, index: i } : null;
    });
    this.state.set({ ...this.get(), queue: next });
  }

  setActionPoints(ap: number): void {
    if (ap !== this.get().actionPoints) this.state.set({ ...this.get(), actionPoints: ap });
  }

  setExecuting(e: 0 | 1 | 2, queueIndex = this.get().queueIndex): void {
    const s = this.get();
    if (e !== s.executing || queueIndex !== s.queueIndex) {
      this.state.set({ ...s, executing: e, queueIndex });
    }
  }

  /** `0..7`; wraps (`0 <-> 7`, §11.3). */
  setPage(n: number): void {
    const page =
      ((Math.trunc(n) % MAX_SLOT_ITEM_COUNT) + MAX_SLOT_ITEM_COUNT) % MAX_SLOT_ITEM_COUNT;
    if (page !== this.get().page) this.state.set({ ...this.get(), page });
  }

  /** The item shortcut of slot `i` on the current page. */
  itemAt(i: number): Shortcut | null {
    const s = this.get();
    return s.items[s.page]?.[i] ?? null;
  }

  /** The state of a slot of any row. */
  slot(row: 'applet' | 'item' | 'queue', slot: number): Shortcut | null {
    const s = this.get();
    if (row === 'applet') return s.applets[slot] ?? null;
    if (row === 'queue') return s.queue[slot] ?? null;
    return s.items[s.page]?.[slot] ?? null;
  }
}
