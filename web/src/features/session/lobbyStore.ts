// `lobbyStore` (Plan/45 §2.3, WP-CFEAT-03): the character list as the lobby screens show it. Writers: the `CharList` and
// `LobbyError` handlers (phase Feature, after the Core handlers fed the state machine). Scope `session`: the store is
// reset when the machine returns to the login screen.
//
// The legacy `OnPlayerList` rule is a reducer here (`applyCharList`): after every list the selected slot stays unless it is
// blocked (or empty), then the first non-blocked occupied slot is selected.

import type { StoreApi } from 'zustand/vanilla';
import type { CharList, CharSlot } from '../../gen/proto/flyff/v1/lobby_pb';
import type { StoreRegistry } from '../../stores/registry';

export const SLOT_COUNT = 3;

/** One occupied slot of the last `CharList` (the full `CharSlot` without the protobuf types). */
export interface LobbySlot {
  readonly slot: number;
  readonly idPlayer: number;
  readonly name: string;
  readonly blocked: boolean;
  readonly worldId: number;
  readonly moverIndex: number;
  readonly sex: number;
  readonly skinSet: number;
  readonly hairMesh: number;
  /** ARGB as stored. */
  readonly hairColor: number;
  readonly headMesh: number;
  readonly job: number;
  readonly level: number;
  readonly str: number;
  readonly sta: number;
  readonly dex: number;
  readonly int: number;
  /** The visible equipment item ids in list order (placed by `ItemProp.parts` by the preview subject builder). */
  readonly equip: readonly number[];
}

export interface LastLobbyError {
  readonly code: number;
  readonly tid: number;
  /** Bumps per error (the same error twice in a row is still an event). */
  readonly seq: number;
}

export interface DeleteDialogState {
  /** The slot being deleted; `null` = no dialog. */
  readonly slot: number | null;
  readonly busy: boolean;
  /** Bumps to clear the key field after a refusal. */
  readonly clearNonce: number;
}

export interface LobbyState {
  /** Always three entries; `null` = empty. */
  readonly slots: readonly (LobbySlot | null)[];
  readonly selected: number;
  readonly lastError: LastLobbyError | null;
  readonly deleteDialog: DeleteDialogState;
  /** `CharList`s received since the login (the screen controller shows the select window after the first). */
  readonly listVersion: number;
}

export function initialLobbyState(): LobbyState {
  return {
    slots: [null, null, null],
    selected: 0,
    lastError: null,
    deleteDialog: { slot: null, busy: false, clearNonce: 0 },
    listVersion: 0,
  };
}

export type LobbyStore = StoreApi<LobbyState>;

export function defineLobbyStore(registry: StoreRegistry): LobbyStore {
  const existing = registry.get('lobby');
  if (existing) return existing as unknown as LobbyStore;
  return registry.defineStore<LobbyState>({
    name: 'lobby',
    scope: 'session',
    owner: '45',
    initial: initialLobbyState,
  });
}

function toSlot(s: CharSlot): LobbySlot {
  return {
    slot: s.slot,
    idPlayer: s.idPlayer,
    name: s.name,
    blocked: s.blocked,
    worldId: s.worldId,
    moverIndex: s.moverIndex,
    sex: s.sex,
    skinSet: s.skinSet,
    hairMesh: s.hairMesh,
    hairColor: s.hairColor,
    headMesh: s.headMesh,
    job: s.job,
    level: s.level,
    str: s.str,
    sta: s.sta,
    dex: s.dex,
    int: s.int,
    equip: [...s.equipItemIds],
  };
}

/**
 * The slots of a `CharList` (a slot outside 0..2 is ignored; a duplicate slot keeps the first, like the lobby's own rule)
 * and the selection of `OnPlayerList`: unchanged unless that slot is blocked or empty, then the first non-blocked
 * occupied slot (when there is none the selection is kept).
 */
export function applyCharList(
  previous: LobbyState,
  list: Pick<CharList, 'slots'>,
): Pick<LobbyState, 'slots' | 'selected'> {
  const slots: (LobbySlot | null)[] = [null, null, null];
  for (const s of list.slots) {
    if (s.slot < 0 || s.slot >= SLOT_COUNT || slots[s.slot] !== null) continue;
    slots[s.slot] = toSlot(s);
  }
  let selected = previous.selected;
  const current = slots[selected] ?? null;
  if (current === null || current.blocked) {
    const first = slots.findIndex((s) => s !== null && !s.blocked);
    if (first >= 0) selected = first;
  }
  return { slots, selected };
}
