// Sim events (Plan/40 section 7.6, Plan/31 section 17.6): `take_events()` returns a pointer to `[count, dropped, records...]`
// (8 words per record: `[kind, obj_id, frame_lo, a, b, c, d, e]`, floats bit-cast). `SimEventReader` turns the pointer
// into records without allocating when the buffer is empty; `SimEventRouter` dispatches the records to the consumers
// that subscribed at boot (43 camera, 44 audio, 41 render, 17 combat, the reporters). Kinds are the generated
// `EVENT_KINDS` of `gen/sim/wire.ts`; a consumer that asks for a kind this build does not know is logged once and
// ignored (a kind a later `flyff-sim` adds must not break an older client).

import { EVENT_BUFFER, EVENT_KINDS, type EventName } from '../gen/sim/wire';
import { bitsToFloat, type SimEventRecord } from '../wasm/simTypes';
import type { MemoryLike } from './renderView';

export type { SimEventRecord };

/** The frame slot `frame_lo`: events carry the low 32 bits of the sim frame. */
export { bitsToFloat };

/** `a..e` of an event as numbers: `payload[i]` of the kind says whether it is a float or an id (see `EVENT_KINDS`). */
export function eventFloat(e: SimEventRecord, word: 0 | 1 | 2 | 3 | 4): number {
  return bitsToFloat(e.words[word]);
}

const EMPTY: readonly SimEventRecord[] = Object.freeze([]);

/** What one `take_events` call produced. */
export interface EventBatch {
  readonly events: readonly SimEventRecord[];
  /** Events the sim dropped because its buffer was full (cosmetic ones first). */
  readonly dropped: number;
}

const NAME_BY_ID = new Map<number, EventName>(
  (Object.keys(EVENT_KINDS) as EventName[]).map((n) => [EVENT_KINDS[n].id, n]),
);

/** Reads the event buffer; the header view is cached per (buffer, pointer) so an empty frame allocates nothing. */
export class SimEventReader {
  private header: Uint32Array | null = null;
  private builtOn: ArrayBufferLike | null = null;
  private ptr = -1;

  constructor(private readonly memory: MemoryLike) {}

  read(ptr: number): EventBatch {
    const buffer = this.memory.buffer;
    if (this.header === null || this.builtOn !== buffer || this.ptr !== ptr) {
      this.header = new Uint32Array(buffer, ptr, EVENT_BUFFER.HEADER_WORDS);
      this.builtOn = buffer;
      this.ptr = ptr;
    }
    const count = Math.min(this.header[0] ?? 0, EVENT_BUFFER.CAPACITY);
    const dropped = this.header[1] ?? 0;
    if (count === 0) return { events: EMPTY, dropped };
    const words = new Uint32Array(
      buffer,
      ptr + EVENT_BUFFER.HEADER_WORDS * 4,
      count * EVENT_BUFFER.RECORD_WORDS,
    );
    const events: SimEventRecord[] = new Array<SimEventRecord>(count);
    for (let i = 0; i < count; i += 1) {
      const at = i * EVENT_BUFFER.RECORD_WORDS;
      const kind = words[at] ?? 0;
      events[i] = {
        kind,
        name: NAME_BY_ID.get(kind) ?? '?',
        objId: words[at + 1] ?? 0,
        frame: words[at + 2] ?? 0,
        words: [
          words[at + 3] ?? 0,
          words[at + 4] ?? 0,
          words[at + 5] ?? 0,
          words[at + 6] ?? 0,
          words[at + 7] ?? 0,
        ],
      };
    }
    return { events, dropped };
  }
}

export interface SimEventRouter {
  /**
   * Subscribes `fn` to the given kinds (names of `EVENT_KINDS` or numeric ids); returns the unsubscribe function.
   * A name this build does not know is ignored (logged once).
   */
  on(kinds: readonly (EventName | number)[], fn: (e: SimEventRecord) => void): () => void;
  /** Dispatches a batch (the loop's step 5). A throwing consumer is logged and the rest still run. */
  dispatch(events: readonly unknown[]): void;
  readonly stats: {
    readonly dispatched: number;
    readonly errors: number;
    /** Events with a kind no consumer listens to. */
    readonly unhandled: number;
    readonly dropped: number;
  };
}

export interface SimEventRouterOptions {
  readonly log?: {
    warn(scope: string, message: string): void;
    error(scope: string, message: string): void;
  };
}

export function createSimEventRouter(options: SimEventRouterOptions = {}): SimEventRouter {
  const byKind = new Map<number, Set<(e: SimEventRecord) => void>>();
  const warned = new Set<string>();
  const stats = { dispatched: 0, errors: 0, unhandled: 0, dropped: 0 };
  return {
    on(kinds, fn) {
      const ids: number[] = [];
      for (const k of kinds) {
        const id =
          typeof k === 'number'
            ? k
            : (EVENT_KINDS as Record<string, { id: number } | undefined>)[k]?.id;
        if (id === undefined) {
          if (!warned.has(String(k))) {
            warned.add(String(k));
            options.log?.warn(
              'sim',
              `event kind ${String(k)} is not in this build; the subscription is ignored`,
            );
          }
          continue;
        }
        ids.push(id);
        let set = byKind.get(id);
        if (!set) {
          set = new Set();
          byKind.set(id, set);
        }
        set.add(fn);
      }
      return () => {
        for (const id of ids) byKind.get(id)?.delete(fn);
      };
    },
    dispatch(events) {
      for (const raw of events) {
        const e = raw as SimEventRecord;
        const set = byKind.get(e.kind);
        if (!set || set.size === 0) {
          stats.unhandled += 1;
          continue;
        }
        stats.dispatched += 1;
        for (const fn of [...set]) {
          try {
            fn(e);
          } catch (error) {
            stats.errors += 1;
            if (stats.errors <= 5) {
              options.log?.error(
                'sim',
                `event ${e.name} handler failed: ${error instanceof Error ? error.message : String(error)}`,
              );
            }
          }
        }
      }
    },
    get stats() {
      return { ...stats };
    },
  };
}
