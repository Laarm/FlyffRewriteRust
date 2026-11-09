// The acting GM character (Plan/25 section 1.5): some shortcuts move or use the admin's own in-game character ("Summon to me",
// "Go to", "Jump to me"). A staff member's acting character is the in-world character of the staff account (the server tells in
// `GET /admin/api/session`, `acting_character`). An owner picks one of the online characters with authority L or higher; the
// choice is remembered in `localStorage`. The id travels as `as_character` with each such action; the server checks it.
import { create } from 'zustand';
import { authorityAtLeast } from '../gen/caps';
import type { ActingCharacter, SessionInfo } from '../gen/dto';
import { rest } from '../api/rest';
import { safeStorage } from '../../platform/storage';

export const ACTING_STORAGE_KEY = 'flyff.admin.acting.v1';

/** One online character that may act (`GET /admin/api/sessions`, `in_world`). */
export interface ActingCandidate extends ActingCharacter {
  readonly authority: string;
}

interface ActingState {
  /** The owner's remembered pick. */
  readonly chosen: ActingCharacter | null;
  readonly candidates: readonly ActingCandidate[];
  readonly loading: boolean;
  readonly choose: (c: ActingCharacter | null) => void;
  readonly loadCandidates: () => Promise<void>;
}

function readChosen(): ActingCharacter | null {
  const raw = safeStorage.local.get(ACTING_STORAGE_KEY);
  if (raw === null) return null;
  try {
    const v = JSON.parse(raw) as Partial<ActingCharacter>;
    return typeof v.id === 'number' && typeof v.name === 'string'
      ? { id: v.id, name: v.name }
      : null;
  } catch {
    return null;
  }
}

interface OnlineRow {
  readonly character_id: number;
  readonly name: string;
  readonly authority: string;
}

/** The online rows that may act: authority L or higher. */
export function actingCandidates(rows: readonly OnlineRow[]): readonly ActingCandidate[] {
  return rows
    .filter((r) => authorityAtLeast(r.authority, 'L'))
    .map((r) => ({ id: r.character_id, name: r.name, authority: r.authority }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export const useActing = create<ActingState>()((set) => ({
  chosen: readChosen(),
  candidates: [],
  loading: false,
  choose(c) {
    if (c === null) safeStorage.local.remove(ACTING_STORAGE_KEY);
    else safeStorage.local.set(ACTING_STORAGE_KEY, JSON.stringify(c));
    set({ chosen: c });
  },
  async loadCandidates() {
    set({ loading: true });
    try {
      const body = await rest.get<{ in_world: OnlineRow[] }>('/admin/api/sessions', {
        query: { limit: 500 },
      });
      set({ candidates: actingCandidates(body.in_world), loading: false });
    } catch {
      set({ loading: false });
    }
  },
}));

/**
 * The acting character for this session: a staff member's own in-world character; an owner's pick while it is still online
 * (`null` otherwise: the actions that need one are disabled with "log in a GM character in the game").
 */
export function actingFor(
  info: SessionInfo | null,
  chosen: ActingCharacter | null,
  candidates: readonly ActingCandidate[],
): ActingCharacter | null {
  if (!info) return null;
  if (info.kind === 'staff') return info.acting_character;
  if (chosen && candidates.some((c) => c.id === chosen.id)) return chosen;
  return null;
}
