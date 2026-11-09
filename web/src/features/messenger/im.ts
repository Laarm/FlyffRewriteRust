// The instant-message sessions (`CWndMgr::m_mapMessage`, `OpenMessage`, `UpdateMessage`; `src:_Interface/WndManager.cpp:7200-7250`; Plan/45 §8.3, §24.1; WP-CFEAT-39): one
// conversation per partner while his window is open - the lines, the partner's name (a rename changes it) and whether I blocked him. A session is the `ViewSource<ImView>`
// of its window; the window gone, the session goes (the original kept nothing either). The registry is keyed by the lower-cased name (`instanceKey` of `APP_MESSAGE`).

import { imKey } from '../../ui/windows/messenger/imKey';
import type { ImLine, ImView, ViewSource } from '../../ui/windows/messenger/ports';

/** The most lines a conversation keeps (a window is a chat box, not a log). */
export const MAX_IM_LINES = 500;

export class ImSession implements ViewSource<ImView> {
  #partner: string;
  #partnerId: number | null;
  #lines: readonly ImLine[] = [];
  #blocked = false;
  #view: ImView;
  readonly #listeners = new Set<() => void>();

  constructor(partner: string, partnerId: number | null, blocked: boolean) {
    this.#partner = partner;
    this.#partnerId = partnerId;
    this.#blocked = blocked;
    this.#view = this.#make();
  }

  #make(): ImView {
    return { partner: this.#partner, lines: this.#lines, blocked: this.#blocked };
  }

  #publish(): void {
    this.#view = this.#make();
    for (const fn of [...this.#listeners]) fn();
  }

  get partner(): string {
    return this.#partner;
  }
  /** The character id of the partner, `null` while it is not known. */
  get partnerId(): number | null {
    return this.#partnerId;
  }
  get lines(): readonly ImLine[] {
    return this.#lines;
  }
  get blocked(): boolean {
    return this.#blocked;
  }

  get = (): ImView => this.#view;

  subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };

  /** `AddMessage`: one more line at the end. */
  append(line: ImLine): void {
    const next = [...this.#lines, line];
    this.#lines = next.length > MAX_IM_LINES ? next.slice(next.length - MAX_IM_LINES) : next;
    this.#publish();
  }

  setBlocked(blocked: boolean): void {
    if (blocked === this.#blocked) return;
    this.#blocked = blocked;
    this.#publish();
  }

  /** The partner is known by id now. */
  setPartnerId(id: number): void {
    this.#partnerId = id;
  }

  /** `UpdateMessage`: the partner got a new name; the lines already written keep the name they were written with. */
  rename(name: string): void {
    if (name === this.#partner) return;
    this.#partner = name;
    this.#publish();
  }
}

export class ImRegistry {
  readonly #sessions = new Map<string, ImSession>();

  get size(): number {
    return this.#sessions.size;
  }
  get(name: string): ImSession | undefined {
    return this.#sessions.get(imKey(name));
  }
  has(name: string): boolean {
    return this.#sessions.has(imKey(name));
  }
  add(session: ImSession): void {
    this.#sessions.set(imKey(session.partner), session);
  }
  /** Removes the session, but only when the key still points at it. */
  remove(session: ImSession): void {
    const key = imKey(session.partner);
    if (this.#sessions.get(key) === session) this.#sessions.delete(key);
  }
  /** The session of a character id (the windows that are open). */
  byId(id: number): ImSession | undefined {
    for (const s of this.#sessions.values()) if (s.partnerId === id) return s;
    return undefined;
  }
  /** Re-keys the session of `from` under `to`; `false` when there is none (or `to` is taken by another). */
  rename(from: string, to: string): boolean {
    const s = this.get(from);
    if (!s) return false;
    const other = this.get(to);
    if (other && other !== s) return false;
    this.#sessions.delete(imKey(from));
    s.rename(to);
    this.#sessions.set(imKey(to), s);
    return true;
  }
  all(): readonly ImSession[] {
    return [...this.#sessions.values()];
  }
}
