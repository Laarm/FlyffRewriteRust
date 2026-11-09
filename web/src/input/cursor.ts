// World cursor selection (Plan/43 §4.10, ref/21 §8.7; `CWndWorld::OnSetCursor`, `WndWorld.cpp:2571`): which of the 19
// cursors the pointer shows while it is over the world. The pixels and hotspots belong to 42's `CursorManager`
// (`ui/cursor/cursors.ts` is the one id -> file table); this module only decides the id and applies it on change.

import { CUR, type CursorId } from '../ui/cursor/cursors';

/** What the cursor rule needs to know about the highlighted object. */
export interface CursorTarget {
  readonly kind: 'mover' | 'item' | 'ctrl';
  readonly isPlayer: boolean;
  /** `IsPeaceful()` of a mover (an NPC that cannot be attacked). */
  readonly peaceful: boolean;
  /** `CK1_HOUSING` furniture: the default cursor. */
  readonly housing: boolean;
}

export interface CursorInput {
  /** The middle button is held (flying camera drag). */
  readonly middleDown: boolean;
  readonly target: CursorTarget | null;
  /** Own vendor open, a vendor window open, or a blocking mini-game / smelt window open. */
  readonly blocked: boolean;
}

/** The first matching row of the §4.10 table. */
export function selectCursor(i: CursorInput): CursorId {
  if (i.middleDown) return CUR.CAMERA;
  const t = i.target;
  if (t) {
    if (t.kind === 'mover') {
      if (!t.isPlayer && t.peaceful) return CUR.NPC;
      if (!t.peaceful) return CUR.ATTACK;
      return CUR.BASE; // a peaceful player: the window default
    }
    if (t.kind === 'item') return CUR.GETITEM;
    return t.housing ? CUR.BASE : CUR.CONTROL;
  }
  if (i.blocked) return CUR.NO;
  return CUR.BASE;
}

/** Applies a cursor id on change only (`CursorManager.setWorldCursor`). */
export class WorldCursor {
  private current: CursorId | -1 = -1;

  constructor(private readonly set: (id: number) => void) {}

  apply(id: CursorId): void {
    if (id === this.current) return;
    this.current = id;
    this.set(id);
  }

  get id(): number {
    return this.current;
  }

  /** The cursor manager lost its target (a new canvas): the next `apply` pushes the id again. */
  invalidate(): void {
    this.current = -1;
  }
}
