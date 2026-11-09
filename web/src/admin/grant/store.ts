// The state of one grant UI (the tab, or one open dialog): the catalogue filter, the item being built, the basket, the recipients, the
// delivery and the reason. A fresh store is made for every dialog (`createGrantStore`); the tab keeps one for the whole session so
// that switching tabs never loses a basket. All logic is plain functions on the state, so it is tested without a DOM.
import { createContext, useContext } from 'react';
import { createStore, type StoreApi } from 'zustand/vanilla';
import { useStore } from 'zustand';
import type {
  DeliveryMode,
  GrantReport,
  GrantRequest,
  ItemSpec,
  Kit,
  Recipients,
} from '../gen/dto';
import { NO_FILTER, type CatalogFilter } from './catalog';
import { blankSpec, sameSpec } from './spec';

/** A chosen recipient. */
export interface Recipient {
  readonly id: number;
  readonly name: string;
  readonly online: boolean;
}

/** A group in place of the single characters (the wire's `Recipients` is exclusive). */
export type GroupChoice =
  | { readonly kind: 'all_online'; readonly includeStaff: boolean }
  | { readonly kind: 'world'; readonly world: number }
  /** Every member of a party / a guild (WP-ADMIN-23); the label is what the recipient field shows. */
  | { readonly kind: 'party'; readonly party: number; readonly label: string }
  | { readonly kind: 'guild'; readonly guild: number; readonly label: string };

export type ModeChoice = 'auto' | 'direct' | 'pending' | 'mail';

/** One line of the basket. */
export interface BasketLine {
  readonly id: number;
  readonly spec: ItemSpec;
}

/** Where a ground drop lands (the ground-drop mode). */
export interface GroundTarget {
  readonly world: number;
  readonly layer: number;
  readonly x: number;
  readonly y: number;
  readonly z: number;
  /** The character with loot priority; `null` = anyone. */
  readonly owner: Recipient | null;
}

export type Target = 'recipients' | 'ground';

export interface GrantState {
  readonly filter: CatalogFilter;
  readonly view: 'grid' | 'list';
  /** The item being built. */
  readonly spec: ItemSpec | null;
  /** The basket line `spec` was loaded from (an edit replaces it), or `null`. */
  readonly editing: number | null;
  readonly lines: readonly BasketLine[];
  readonly gold: number;
  readonly recipients: readonly Recipient[];
  readonly group: GroupChoice | null;
  readonly mode: ModeChoice;
  readonly mailSubject: string;
  readonly mailText: string;
  readonly preset: string | null;
  readonly note: string;
  /** The typed confirmation. */
  readonly confirm: string;
  /** What must be typed, once the server asked for a confirmation. */
  readonly confirmFor: string | null;
  readonly target: Target;
  readonly ground: GroundTarget;
  readonly nextId: number;
  readonly busy: boolean;
  readonly error: string | null;
  readonly report: GrantReport | null;
  /** A message after a ground drop. */
  readonly dropped: string | null;
}

export interface GrantActions {
  setFilter: (patch: Partial<CatalogFilter>) => void;
  setView: (view: 'grid' | 'list') => void;
  /** Starts building `itemId` (a fresh spec). */
  pick: (itemId: number) => void;
  patchSpec: (patch: Partial<ItemSpec>) => void;
  setSpec: (spec: ItemSpec | null) => void;
  /** Adds `spec` to the basket (a line equal in everything but the count grows by its count, up to `maxCount`). */
  addSpec: (spec: ItemSpec, maxCount?: number) => void;
  editLine: (id: number) => void;
  removeLine: (id: number) => void;
  setLineCount: (id: number, count: number) => void;
  setGold: (gold: number) => void;
  addRecipient: (r: Recipient) => void;
  removeRecipient: (id: number) => void;
  setGroup: (group: GroupChoice | null) => void;
  setMode: (mode: ModeChoice) => void;
  setMail: (subject: string, text: string) => void;
  setReason: (preset: string | null, note: string) => void;
  setConfirm: (text: string) => void;
  askConfirm: (expected: string) => void;
  setTarget: (target: Target) => void;
  setGround: (patch: Partial<GroundTarget>) => void;
  /** Adds the items and the penya of a kit to the basket. */
  loadKit: (kit: Kit) => void;
  setBusy: (busy: boolean) => void;
  setError: (message: string | null) => void;
  setReport: (report: GrantReport | null) => void;
  setDropped: (message: string | null) => void;
  /** After a successful grant: an empty basket (the recipients, delivery and reason stay for the next one). */
  clearBasket: () => void;
}

export type GrantStore = GrantState & GrantActions;

/** What a dialog opens with. */
export interface GrantInit {
  readonly recipients?: readonly Recipient[];
  readonly item?: number;
  readonly kit?: Kit;
  readonly ground?: Partial<GroundTarget>;
  readonly spec?: ItemSpec;
  readonly mode?: ModeChoice;
}

export const DEFAULT_GROUND: GroundTarget = { world: 1, layer: 0, x: 0, y: 0, z: 0, owner: null };

export function initialState(init: GrantInit = {}): GrantState {
  const spec = init.spec ?? (init.item === undefined ? null : blankSpec(init.item));
  const lines: BasketLine[] = [];
  let gold = 0;
  let nextId = 1;
  if (init.kit) {
    for (const item of init.kit.items) lines.push({ id: nextId++, spec: item });
    gold = init.kit.gold;
  }
  return {
    filter: NO_FILTER,
    view: 'grid',
    spec,
    editing: null,
    lines,
    gold,
    recipients: init.recipients ?? [],
    group: null,
    mode: init.mode ?? 'auto',
    mailSubject: '',
    mailText: '',
    preset: null,
    note: '',
    confirm: '',
    confirmFor: null,
    target: init.ground ? 'ground' : 'recipients',
    ground: { ...DEFAULT_GROUND, ...init.ground },
    nextId,
    busy: false,
    error: null,
    report: null,
    dropped: null,
  };
}

/** Makes a store. */
export function createGrantStore(init: GrantInit = {}): StoreApi<GrantStore> {
  return createStore<GrantStore>()((set) => ({
    ...initialState(init),
    setFilter: (patch) => {
      set((s) => ({ filter: { ...s.filter, ...patch } }));
    },
    setView: (view) => {
      set({ view });
    },
    pick: (itemId) => {
      set({ spec: blankSpec(itemId), editing: null });
    },
    patchSpec: (patch) => {
      set((s) => (s.spec ? { spec: { ...s.spec, ...patch } } : {}));
    },
    setSpec: (spec) => {
      set({ spec, editing: null });
    },
    addSpec: (spec, maxCount) => {
      set((s) => {
        if (s.editing !== null) {
          return {
            lines: s.lines.map((l) => (l.id === s.editing ? { id: l.id, spec } : l)),
            editing: null,
            spec: null,
          };
        }
        const hit = s.lines.find(
          (l) =>
            sameSpec({ ...l.spec, count: 1 }, { ...spec, count: 1 }) &&
            (maxCount === undefined || l.spec.count + spec.count <= maxCount),
        );
        if (hit) {
          return {
            lines: s.lines.map((l) =>
              l.id === hit.id
                ? { id: l.id, spec: { ...l.spec, count: l.spec.count + spec.count } }
                : l,
            ),
          };
        }
        return { lines: [...s.lines, { id: s.nextId, spec }], nextId: s.nextId + 1 };
      });
    },
    editLine: (id) => {
      set((s) => {
        const line = s.lines.find((l) => l.id === id);
        return line ? { spec: line.spec, editing: id } : {};
      });
    },
    removeLine: (id) => {
      set((s) => ({
        lines: s.lines.filter((l) => l.id !== id),
        ...(s.editing === id ? { editing: null, spec: null } : {}),
      }));
    },
    setLineCount: (id, count) => {
      set((s) => ({
        lines: s.lines.map((l) => (l.id === id ? { id, spec: { ...l.spec, count } } : l)),
      }));
    },
    setGold: (gold) => {
      set({ gold });
    },
    addRecipient: (r) => {
      set((s) =>
        s.recipients.some((x) => x.id === r.id)
          ? {}
          : { recipients: [...s.recipients, r], group: null },
      );
    },
    removeRecipient: (id) => {
      set((s) => ({ recipients: s.recipients.filter((r) => r.id !== id) }));
    },
    setGroup: (group) => {
      set({ group });
    },
    setMode: (mode) => {
      set({ mode });
    },
    setMail: (mailSubject, mailText) => {
      set({ mailSubject, mailText });
    },
    setReason: (preset, note) => {
      set({ preset, note });
    },
    setConfirm: (confirm) => {
      set({ confirm });
    },
    askConfirm: (confirmFor) => {
      set({ confirmFor, confirm: '' });
    },
    setTarget: (target) => {
      set({ target });
    },
    setGround: (patch) => {
      set((s) => ({ ground: { ...s.ground, ...patch } }));
    },
    loadKit: (kit) => {
      set((s) => {
        let nextId = s.nextId;
        const added = kit.items.map((spec) => ({ id: nextId++, spec }));
        return { lines: [...s.lines, ...added], nextId, gold: s.gold + kit.gold };
      });
    },
    setBusy: (busy) => {
      set({ busy });
    },
    setError: (error) => {
      set({ error });
    },
    setReport: (report) => {
      set({ report });
    },
    setDropped: (dropped) => {
      set({ dropped });
    },
    clearBasket: () => {
      set({ lines: [], gold: 0, spec: null, editing: null, confirm: '', confirmFor: null });
    },
  }));
}

// ---- derived values -------------------------------------------------------------------------------------------------------------

/** The `Recipients` of the wire for the state. */
export function recipientsOf(s: Pick<GrantState, 'recipients' | 'group'>): Recipients {
  if (s.group?.kind === 'all_online')
    return { all_online: { include_staff: s.group.includeStaff } };
  if (s.group?.kind === 'world') return { world: s.group.world };
  if (s.group?.kind === 'party') return { party: s.group.party };
  if (s.group?.kind === 'guild') return { guild: s.group.guild };
  return { characters: s.recipients.map((r) => r.id) };
}

/** The delivery of the wire for the state. */
export function deliveryOf(s: Pick<GrantState, 'mode' | 'mailSubject' | 'mailText'>): DeliveryMode {
  if (s.mode === 'mail')
    return { mail: { subject: s.mailSubject.trim(), text: s.mailText.trim() } };
  return s.mode;
}

/** The request of `POST /admin/api/grants/v2` for the state. */
export function requestOf(s: GrantState): GrantRequest {
  return {
    recipients: recipientsOf(s),
    items: s.lines.map((l) => l.spec),
    gold: s.gold,
    mode: deliveryOf(s),
  };
}

/** Why the grant cannot be sent yet (`null` = it can). The server validates everything again. */
export function blockedReason(s: GrantState): string | null {
  const noRecipients = s.target === 'recipients' && s.group === null && s.recipients.length === 0;
  if (noRecipients) return 'recipients';
  if (s.lines.length === 0 && s.gold <= 0) return 'basket';
  if (s.preset === null && s.note.trim() === '') return 'reason';
  if (s.mode === 'mail' && s.mailSubject.trim() === '') return 'mail';
  if (s.target === 'ground' && s.lines.length === 0) return 'basket';
  return null;
}

/** The pieces of items and the penya of the basket (the confirmation facts). */
export function basketTotals(s: Pick<GrantState, 'lines' | 'gold'>): {
  pieces: number;
  gold: number;
  lines: number;
} {
  return {
    pieces: s.lines.reduce((sum, l) => sum + l.spec.count, 0),
    gold: s.gold,
    lines: s.lines.length,
  };
}

// ---- the React side -------------------------------------------------------------------------------------------------------------

const GrantContext = createContext<StoreApi<GrantStore> | null>(null);

export const GrantProvider = GrantContext.Provider;

/** The store of the surrounding grant UI. */
export function useGrantStoreApi(): StoreApi<GrantStore> {
  const api = useContext(GrantContext);
  if (!api) throw new Error('the grant UI needs a GrantProvider');
  return api;
}

/** Selects from the store of the surrounding grant UI. */
export function useGrant<T>(selector: (s: GrantStore) => T): T {
  return useStore(useGrantStoreApi(), selector);
}
