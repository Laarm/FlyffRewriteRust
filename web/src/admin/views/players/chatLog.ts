// The line buffer of the chat monitor (Plan/25 section 4.9): the last 2,000 lines, a pause that keeps the view still while new lines
// wait, and the view filters. Pure functions; `ChatMonitor.tsx` is the screen.
import { ChatChannel, type ChatLine } from '../../../gen/proto/admin';
import { n64 } from '../../../net/int64';

/** Lines the client keeps (Plan/25 section 4.9). */
export const MAX_LINES = 2000;

/** One line as the monitor shows it. */
export interface ChatEntry {
  /** A running number (the React key). */
  readonly key: number;
  /** Unix milliseconds. */
  readonly ts: number;
  readonly channel: ChatChannel;
  readonly fromId: number;
  readonly from: string;
  readonly to: string;
  readonly text: string;
  /** What was typed before the word filter changed it (P sessions only), empty otherwise. */
  readonly original: string;
  readonly world: number;
}

export function entryOf(line: ChatLine, key: number): ChatEntry {
  return {
    key,
    ts: n64(line.tsMs),
    channel: line.channel,
    fromId: n64(line.fromCharacterId),
    from: line.from,
    to: line.to,
    text: line.text,
    original: line.originalText,
    world: line.world,
  };
}

/** The channels only `view.chat.private` may see. */
export const PRIVATE_CHANNELS: readonly ChatChannel[] = [
  ChatChannel.PARTY,
  ChatChannel.GUILD,
  ChatChannel.WHISPER,
  ChatChannel.MESSENGER,
];

export const PUBLIC_CHANNELS: readonly ChatChannel[] = [ChatChannel.SAY, ChatChannel.SHOUT];

export const isPrivate = (c: ChatChannel): boolean => PRIVATE_CHANNELS.includes(c);

/** What the operator chose to see. */
export interface MonitorFilter {
  readonly channels: ReadonlySet<ChatChannel>;
  /** `0` = all worlds. */
  readonly world: number;
  /** `0` = everybody. */
  readonly character: number;
  readonly text: string;
}

/** The channels a session may select, in display order. */
export function selectable(privateAllowed: boolean): readonly ChatChannel[] {
  return privateAllowed ? [...PUBLIC_CHANNELS, ...PRIVATE_CHANNELS] : PUBLIC_CHANNELS;
}

/** The default selection: everything the session may see. */
export function defaultFilter(privateAllowed: boolean): MonitorFilter {
  return { channels: new Set(selectable(privateAllowed)), world: 0, character: 0, text: '' };
}

/** `true` when the filter includes a private channel (the privacy banner and the audit follow it). */
export function includesPrivate(f: MonitorFilter): boolean {
  return [...f.channels].some(isPrivate);
}

/** Does the entry pass the view filter (the same rules as the server's `ChatFilter`, applied to the buffer)? */
export function matches(f: MonitorFilter, e: ChatEntry): boolean {
  if (!f.channels.has(e.channel)) return false;
  if (f.world !== 0 && e.world !== f.world) return false;
  if (f.character !== 0 && e.fromId !== f.character) return false;
  const text = f.text.trim().toLowerCase();
  return text === '' || e.text.toLowerCase().includes(text);
}

/** The buffer with `incoming` appended, cut to the newest [`MAX_LINES`]. */
export function append(buffer: readonly ChatEntry[], incoming: readonly ChatEntry[]): ChatEntry[] {
  if (incoming.length === 0) return buffer as ChatEntry[];
  const all = buffer.concat(incoming);
  return all.length > MAX_LINES ? all.slice(all.length - MAX_LINES) : all;
}

/** `HH:MM:SS` of a time in the browser's zone. */
export function clock(ms: number): string {
  const d = new Date(ms);
  const two = (n: number): string => String(n).padStart(2, '0');
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

export const CHANNEL_LABEL: Readonly<Partial<Record<ChatChannel, string>>> = {
  [ChatChannel.SAY]: 'Say',
  [ChatChannel.SHOUT]: 'Shout',
  [ChatChannel.PARTY]: 'Party',
  [ChatChannel.GUILD]: 'Guild',
  [ChatChannel.WHISPER]: 'Whisper',
  [ChatChannel.MESSENGER]: 'Messenger',
  [ChatChannel.SYSTEM]: 'System',
};

// ---- the buffer as a store -----------------------------------------------------------------------------------------------------------------

/** What the screen renders: replaced (never mutated) whenever the buffer changed, at most every `FLUSH_MS`. */
export interface ChatSnapshot {
  readonly lines: readonly ChatEntry[];
  /** Lines that arrived while paused. */
  readonly waiting: number;
  readonly paused: boolean;
  /** The subscription was opened and nothing refused it. */
  readonly connected: boolean;
  readonly refusal: string | null;
  /** The server restarted the stream (`Resync`): lines may be missing. */
  readonly restarted: boolean;
  readonly version: number;
}

/** How often the screen is told about new lines. */
export const FLUSH_MS = 250;

/**
 * The line buffer of one monitor, outside React: lines arrive from the socket at any rate, the screen reads a snapshot (through
 * `useSyncExternalStore`) a few times a second. While paused new lines wait in a second buffer and join the view on resume.
 */
export class ChatBuffer {
  #lines: ChatEntry[] = [];
  #waiting: ChatEntry[] = [];
  #paused = false;
  #connected = false;
  #refusal: string | null = null;
  #restarted = false;
  #keys = 0;
  #version = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #snapshot: ChatSnapshot;
  readonly #listeners = new Set<() => void>();

  constructor() {
    this.#snapshot = this.#build();
  }

  #build(): ChatSnapshot {
    return {
      lines: this.#lines,
      waiting: this.#waiting.length,
      paused: this.#paused,
      connected: this.#connected,
      refusal: this.#refusal,
      restarted: this.#restarted,
      version: this.#version,
    };
  }

  #publish(now = false): void {
    const flush = (): void => {
      this.#timer = null;
      this.#version += 1;
      this.#snapshot = this.#build();
      for (const l of [...this.#listeners]) l();
    };
    if (now) {
      if (this.#timer !== null) clearTimeout(this.#timer);
      flush();
    } else if (this.#timer === null) {
      this.#timer = setTimeout(flush, FLUSH_MS);
    }
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  readonly getSnapshot = (): ChatSnapshot => this.#snapshot;

  /** A line from the socket. */
  push(line: ChatLine): void {
    const entry = entryOf(line, (this.#keys += 1));
    this.#connected = true;
    if (this.#paused) this.#waiting.push(entry);
    else this.#lines = append(this.#lines, [entry]);
    this.#publish();
  }

  /** A subscription was (re)opened with a new filter: forget the refusal of the last one. */
  opened(): void {
    this.#refusal = null;
    this.#connected = true;
    this.#publish(true);
  }

  refused(message: string): void {
    this.#refusal = message;
    this.#publish(true);
  }

  resynced(): void {
    this.#restarted = true;
    this.#publish(true);
  }

  dismissRestart(): void {
    this.#restarted = false;
    this.#publish(true);
  }

  pause(): void {
    this.#paused = true;
    this.#publish(true);
  }

  resume(): void {
    this.#lines = append(this.#lines, this.#waiting);
    this.#waiting = [];
    this.#paused = false;
    this.#publish(true);
  }

  clear(): void {
    this.#lines = [];
    this.#waiting = [];
    this.#publish(true);
  }

  /** Ends the pending flush (the screen went away for good). */
  dispose(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.#listeners.clear();
  }
}

// ---- one buffer for the life of the page -----------------------------------------------------------------------------------------

let shared: { readonly identity: string; readonly buffer: ChatBuffer } | null = null;

/**
 * The buffer of the chat monitor of this page: the lines stay while the operator visits another sub-tab and comes back (the feed
 * itself only runs while the Chat tab is open). A different identity (a sign-in as another staff member, a sign-out) starts an empty one,
 * so lines a session was allowed to see never reach another.
 */
export function sharedBuffer(identity: string): ChatBuffer {
  if (shared?.identity !== identity) {
    shared?.buffer.dispose();
    shared = { identity, buffer: new ChatBuffer() };
  }
  return shared.buffer;
}

/** Tests: forgets the shared buffer. */
export function resetSharedBufferForTests(): void {
  shared?.buffer.dispose();
  shared = null;
}
