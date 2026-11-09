// `mailStore` (Plan/45 §2.3, §16; WP-CFEAT-23): the own mailbox as the server sent it (`CMailBox`). The reducers are pure functions over an immutable
// state value; the handlers of `mailModule.ts` call them with the decoded messages.
//
//   QueryMailBoxNtf   the whole box                                  (`applyBox`)
//   RemoveMail        0 the mail is gone, 1 the item was taken, 2 the penya were taken, 3 the mail is read   (`applyRemove`)
//   PostMail          a mail arrived while the post window is open (defined, never pushed: Plan/19 §11.2)    (`applyPost`)

import type { StoreApi } from 'zustand/vanilla';
import type { Mail } from '../../gen/proto/flyff/v1/social_types_pb';
import type { StoreRegistry } from '../../stores/registry';
import { itemFromWire, type InvItem } from '../inventory/items';

/** `RemoveMail.kind`. */
export const REMOVE = { MAIL: 0, ITEM: 1, GOLD: 2, READ: 3 } as const;

/** `MAX_MAIL`: at most 50 mails per box (Plan/19 §11.1). */
export const MAX_MAIL = 50;

export interface MailEntry {
  /** `m_nMail`. */
  readonly id: number;
  /** `m_idSender`, 0 = the system. */
  readonly senderId: number;
  readonly item: InvItem | null;
  readonly gold: number;
  /** Unix seconds (`m_tmCreate`). */
  readonly createdAt: number;
  readonly read: boolean;
  readonly title: string;
  readonly text: string;
}

export interface MailState {
  /** The box was received at least once since the join. */
  readonly ready: boolean;
  readonly box: readonly MailEntry[];
  readonly version: number;
}

export function initialMailState(): MailState {
  return { ready: false, box: [], version: 0 };
}

export type MailStore = StoreApi<MailState>;

/** Scope `world`: the box is asked for again by the next post window. */
export function defineMailStore(registry: StoreRegistry): MailStore {
  const existing = registry.get('mail');
  if (existing) return existing as unknown as MailStore;
  return registry.defineStore<MailState>({
    name: 'mail',
    scope: 'world',
    owner: '45',
    initial: initialMailState,
  });
}

/** The entry of a wire mail. */
export function entryOf(m: Mail): MailEntry {
  return {
    id: m.mailId,
    senderId: m.idSender,
    item: m.item ? itemFromWire(m.item) : null,
    gold: m.gold,
    createdAt: Number(m.createdAt),
    read: m.read,
    title: m.title,
    text: m.text,
  };
}

/** `OnMailBox`: the box is replaced (at most `MAX_MAIL` mails). */
export function applyBox(s: MailState, mails: readonly Mail[]): MailState {
  return { ready: true, box: mails.slice(0, MAX_MAIL).map(entryOf), version: s.version + 1 };
}

/** `OnPostMail`: `CMailBox::AddMail`; a mail the box has already is not added twice. */
export function applyPost(s: MailState, mail: Mail | undefined): MailState {
  if (!mail || s.box.some((e) => e.id === mail.mailId)) return s;
  return { ...s, box: [...s.box, entryOf(mail)], version: s.version + 1 };
}

/** `OnRemoveMail` (`CMailBox::RemoveMail`, `RemoveMailItem`, `RemoveMailGold`, `ReadMail`). */
export function applyRemove(s: MailState, id: number, kind: number): MailState {
  const i = s.box.findIndex((e) => e.id === id);
  const e = s.box[i];
  if (i < 0 || !e) return s;
  let next: readonly MailEntry[];
  switch (kind) {
    case REMOVE.MAIL:
      next = s.box.filter((x) => x.id !== id);
      break;
    case REMOVE.ITEM:
      if (e.item === null) return s;
      next = s.box.map((x) => (x.id === id ? { ...x, item: null } : x));
      break;
    case REMOVE.GOLD:
      if (e.gold === 0) return s;
      next = s.box.map((x) => (x.id === id ? { ...x, gold: 0 } : x));
      break;
    case REMOVE.READ:
      if (e.read) return s;
      next = s.box.map((x) => (x.id === id ? { ...x, read: true } : x));
      break;
    default:
      return s;
  }
  return { ...s, box: next, version: s.version + 1 };
}
