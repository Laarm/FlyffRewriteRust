// Handler registry and dispatch (Plan/40 section 5.4, CA5): maps every `ServerMsg` oneof case to an ordered
// list of handlers in five phases (Pre observers -> Core -> Sim -> Feature -> Fx observers). Several
// modules may handle one case (the original `OnSnapshot` did store updates, mover actions and UI effects in
// one handler). Unknown cases and handler exceptions are counted, never fatal by themselves.
//
// Rules implemented here: H2 (per-handler isolation, escalation to an L2 resync / L3 reload for Core/Sim
// errors), H3 (messages about unknown objects are ignored), SM4 (`inJoin`), observers cannot write stores
// (dev builds), the batch hooks and the drain-end hooks.

import type { ServerBatch } from '../gen/proto/flyff/v1/envelope_pb';
import { n64 } from '../gen/proto/js_safe';
import type { Logger } from '../platform/log';
import { now as defaultNow } from '../platform/time';
import { ObserverWriteError } from '../stores/registry';
import {
  decodeBatch,
  dispatchDecoded,
  SERVER_CASES,
  type ServerCase,
  type ServerHandlers,
  type ServerValue,
} from './codec';
import type { RawFrame } from './inbound';

export type Case = ServerCase;
export type Value<K extends Case> = ServerValue<K>;

/** Handler phases in dispatch order (plain object, not `const enum`: isolatedModules). */
export const Phase = { Pre: 0, Core: 1, Sim: 2, Feature: 3, Fx: 4 } as const;
export type Phase = (typeof Phase)[keyof typeof Phase];

const PHASE_NAMES = ['Pre', 'Core', 'Sim', 'Feature', 'Fx'] as const;

/** Per-message context. The object is reused: it is valid only during the handler call. */
export interface MsgCtx {
  /** `n64(ServerBatch.server_frame)`. */
  readonly serverFrame: number;
  /** Receipt time of the frame (`performance.now()`). */
  readonly recvAt: number;
  /** Monotonic per registry (per session). */
  readonly batchSeq: number;
  /** SM4: true from `WorldReadInfo`/`Replace`/`HelloAck{reattached}` to the end of the batch with `SpawnSelf`. */
  readonly inJoin: boolean;
}

export type MsgHandler<K extends Case> = (objId: number, v: Value<K>, ctx: MsgCtx) => void;

export interface BatchHooks {
  begin?: (ctx: MsgCtx) => void;
  end?: (ctx: MsgCtx) => void;
}

export interface RegistryStats {
  /** Cases this build does not know (a newer server), keyed `tag:<n>` (or `empty`). */
  readonly unknown: Readonly<Record<string, number>>;
  /** Known cases without any handler (expected during milestones). */
  readonly unhandled: Readonly<Record<string, number>>;
  /** Exceptions of handlers, keyed by case (`drainEnd:<owner>` / `begin:<owner>` for hooks). */
  readonly errors: Readonly<Record<string, number>>;
  /** Messages about objects that are not in the entity table (H3). */
  readonly unknownObj: Readonly<Record<string, number>>;
  readonly batches: number;
  readonly messages: number;
  readonly decodeErrors: number;
  /** Store writes attempted by Pre/Fx observers (dev builds). */
  readonly observerViolations: number;
  readonly resyncsRequested: number;
  readonly reloadsRequested: number;
}

export interface HandlerRegistry {
  on<K extends Case>(c: K, phase: Phase, owner: string, h: MsgHandler<K>): void;
  onBatch(owner: string, hooks: BatchHooks): void;
  /** Called once per drain, after the last frame (flush DST ops to `RulesContext`, section 7.10). */
  onDrainEnd(owner: string, fn: () => void): void;
  /** After `app/features.ts` registered everything (boot B5); `on` throws afterwards. */
  freeze(): void;
  readonly frozen: boolean;
  /** Decodes and dispatches one inbound frame (the drain callback). Never throws. */
  dispatchFrame(frame: RawFrame): void;
  /** Runs the drain-end hooks (once per drain, after the last frame). */
  endDrain(): void;
  /** Starts a join (`HelloAck{reattached}`): `inJoin` stays true until the end of the batch with `SpawnSelf`. */
  beginJoin(): void;
  readonly inJoin: boolean;
  readonly stats: RegistryStats;
}

/** Observer guard: the store registry's `runReadOnly` (observers must not write stores). */
export interface ObserverGuard {
  runReadOnly<T>(fn: () => T): T;
}

export interface HandlerRegistryOptions {
  readonly log?: Pick<Logger, 'warn' | 'error'>;
  /** H3 filter source; absent = no filtering. */
  readonly entities?: { has(id: number): boolean };
  /** Cases that may name an object that is not (yet) in the entity table. Default: the `Spawn*` cases. */
  readonly objFilterExempt?: ReadonlySet<string>;
  readonly guard?: ObserverGuard;
  /** Run Pre/Fx handlers under `guard` (default: dev builds). */
  readonly assertObservers?: boolean;
  /** L2: close the socket and run the reconnect policy (section 12.2). */
  readonly requestResync?: (reason: string) => void;
  /** L3: reload the page (section 12.2). */
  readonly requestReload?: (reason: string) => void;
  /** A second Core/Sim error within this window escalates to L3 (default 30 s, H2). */
  readonly resyncWindowMs?: number;
  readonly now?: () => number;
  /** `?debug=net`: receives every decoded batch. */
  readonly inspector?: { onBatch(batch: ServerBatch, ctx: MsgCtx): void };
}

/** Cases that carry an object that is created by the message itself. */
export const SPAWN_CASES: ReadonlySet<string> = new Set([
  'spawnSelf',
  'spawnPlayer',
  'spawnNpc',
  'spawnItem',
  'spawnCtrl',
]);

/**
 * The cases the H3 object filter lets through by default: the spawns, `CreateSfxObj`, whose absolute-position effect is created
 * even when the anchor object is not known (`CDPClient::OnCreateSfxObj`, the `else` branch), and `SetNaviPointNtf`, whose sender may be
 * a party member who is out of view.
 */
const DEFAULT_EXEMPT: ReadonlySet<string> = new Set([
  ...SPAWN_CASES,
  'createSfxObj',
  // the minimap mark of a party member who is out of view (`OnSetNaviPoint` never looked the sender up)
  'setNaviPointNtf',
]);

/** Cases that start a join (SM4). */
const JOIN_START_CASES: ReadonlySet<string> = new Set(['worldReadInfo', 'replace']);

interface Entry {
  readonly phase: Phase;
  readonly owner: string;
  readonly order: number;
  readonly fn: (objId: number, v: unknown, ctx: MsgCtx) => void;
}

function hexPrefix(bytes: Uint8Array, n: number): string {
  let out = '';
  const len = Math.min(n, bytes.byteLength);
  for (let i = 0; i < len; i += 1) out += (bytes[i] ?? 0).toString(16).padStart(2, '0');
  return out;
}

function bump(map: Record<string, number>, key: string): void {
  map[key] = (map[key] ?? 0) + 1;
}

export function createHandlerRegistry(options: HandlerRegistryOptions = {}): HandlerRegistry {
  const now = options.now ?? defaultNow;
  const resyncWindowMs = options.resyncWindowMs ?? 30_000;
  const exempt = options.objFilterExempt ?? DEFAULT_EXEMPT;
  const guard = options.guard;
  const assertObservers = options.assertObservers ?? import.meta.env.DEV;

  const byCase = new Map<string, Entry[]>();
  const batchHooks: { owner: string; hooks: BatchHooks }[] = [];
  const drainHooks: { owner: string; fn: () => void }[] = [];
  let frozen = false;
  let order = 0;
  let compiledDirty = true;
  let dispatchTable: ServerHandlers = {};

  const stats = {
    unknown: {} as Record<string, number>,
    unhandled: {} as Record<string, number>,
    errors: {} as Record<string, number>,
    unknownObj: {} as Record<string, number>,
    batches: 0,
    messages: 0,
    decodeErrors: 0,
    observerViolations: 0,
    resyncsRequested: 0,
    reloadsRequested: 0,
  };

  // current message context (reused, see MsgCtx)
  const ctx = { serverFrame: 0, recvAt: 0, batchSeq: 0, inJoin: false };
  let batchSeq = 0;
  let joinActive = false;
  let joinSawSelf = false;
  let lastEscalationAt: number | null = null;

  const escalate = (reason: string): void => {
    const t = now();
    if (lastEscalationAt !== null && t - lastEscalationAt < resyncWindowMs) {
      stats.reloadsRequested += 1;
      options.log?.error(
        'registry',
        `second Core/Sim failure within ${String(resyncWindowMs)} ms: L3 (${reason})`,
      );
      lastEscalationAt = t;
      options.requestReload?.(reason);
    } else {
      stats.resyncsRequested += 1;
      lastEscalationAt = t;
      options.requestResync?.(reason);
    }
  };

  const compile = (): void => {
    const table: Record<string, (objId: number, v: unknown) => void> = {};
    // every known case gets an entry, so join tracking and the unhandled count work without handlers
    for (const c of SERVER_CASES) {
      const entries = [...(byCase.get(c) ?? [])].sort(
        (a, b) => a.phase - b.phase || a.order - b.order,
      );
      table[c] = (objId, v) => {
        runCase(c, entries, objId, v);
      };
    }
    dispatchTable = table;
    compiledDirty = false;
  };

  const runEntry = (c: string, e: Entry, objId: number, v: unknown): void => {
    try {
      if (guard && assertObservers && (e.phase === Phase.Pre || e.phase === Phase.Fx)) {
        guard.runReadOnly(() => {
          e.fn(objId, v, ctx);
        });
      } else {
        e.fn(objId, v, ctx);
      }
    } catch (error) {
      bump(stats.errors, c);
      if (error instanceof ObserverWriteError) stats.observerViolations += 1;
      const message = error instanceof Error ? error.message : String(error);
      options.log?.error(
        'registry',
        `handler ${e.owner} (${PHASE_NAMES[e.phase]}) failed for ${c}: ${message}`,
      );
      if (e.phase === Phase.Core || e.phase === Phase.Sim) escalate(`handler:${c}:${e.owner}`);
    }
  };

  const runCase = (c: string, entries: readonly Entry[], objId: number, v: unknown): void => {
    stats.messages += 1;
    if (JOIN_START_CASES.has(c)) {
      joinActive = true;
      joinSawSelf = false;
    }
    ctx.inJoin = joinActive;
    if (c === 'spawnSelf') joinSawSelf = true;
    if (objId !== 0 && options.entities && !exempt.has(c) && !options.entities.has(objId)) {
      bump(stats.unknownObj, c);
      return;
    }
    if (entries.length === 0) {
      bump(stats.unhandled, c);
      return;
    }
    for (const e of entries) runEntry(c, e, objId, v);
  };

  const hook = (owner: string, name: string, fn: () => void): void => {
    try {
      fn();
    } catch (error) {
      bump(stats.errors, `${name}:${owner}`);
      const message = error instanceof Error ? error.message : String(error);
      options.log?.error('registry', `${name} hook of ${owner} failed: ${message}`);
      escalate(`${name}:${owner}`);
    }
  };

  const onUnknown = (tag: string): void => {
    if (SERVER_CASES.has(tag)) {
      bump(stats.unhandled, tag);
    } else {
      bump(stats.unknown, tag);
    }
  };

  return {
    on(c, phase, owner, h) {
      if (frozen) throw new Error(`handler registry is frozen: ${owner} cannot register ${c}`);
      let list = byCase.get(c);
      if (!list) {
        list = [];
        byCase.set(c, list);
      }
      order += 1;
      list.push({ phase, owner, order, fn: h as Entry['fn'] });
      compiledDirty = true;
    },
    onBatch(owner, hooks) {
      if (frozen)
        throw new Error(`handler registry is frozen: ${owner} cannot register batch hooks`);
      batchHooks.push({ owner, hooks });
    },
    onDrainEnd(owner, fn) {
      if (frozen)
        throw new Error(`handler registry is frozen: ${owner} cannot register drain hooks`);
      drainHooks.push({ owner, fn });
    },
    freeze() {
      if (compiledDirty) compile();
      frozen = true;
    },
    get frozen() {
      return frozen;
    },
    dispatchFrame(frame) {
      if (compiledDirty) compile();
      let batch: ServerBatch;
      let serverFrame: number;
      try {
        batch = decodeBatch(frame.bytes);
        serverFrame = n64(batch.serverFrame);
      } catch (error) {
        stats.decodeErrors += 1;
        const message = error instanceof Error ? error.message : String(error);
        options.log?.warn(
          'registry',
          `undecodable frame (${String(frame.bytes.byteLength)} bytes, first 64: ${hexPrefix(frame.bytes, 64)}): ${message}`,
        );
        escalate('decode');
        return;
      }
      batchSeq += 1;
      stats.batches += 1;
      ctx.serverFrame = serverFrame;
      ctx.recvAt = frame.recvAt;
      ctx.batchSeq = batchSeq;
      ctx.inJoin = joinActive;
      try {
        options.inspector?.onBatch(batch, ctx);
      } catch {
        // the inspector is a debugging aid: it must never break dispatch
      }
      for (const h of batchHooks) {
        const begin = h.hooks.begin;
        if (begin) {
          hook(h.owner, 'begin', () => {
            begin(ctx);
          });
        }
      }
      dispatchDecoded(batch, dispatchTable, onUnknown);
      ctx.inJoin = joinActive;
      for (const h of batchHooks) {
        const end = h.hooks.end;
        if (end) {
          hook(h.owner, 'end', () => {
            end(ctx);
          });
        }
      }
      if (joinActive && joinSawSelf) {
        joinActive = false;
        joinSawSelf = false;
      }
    },
    endDrain() {
      for (const h of drainHooks) hook(h.owner, 'drainEnd', h.fn);
    },
    beginJoin() {
      joinActive = true;
      joinSawSelf = false;
    },
    get inJoin() {
      return joinActive;
    },
    get stats() {
      return {
        ...stats,
        unknown: { ...stats.unknown },
        unhandled: { ...stats.unhandled },
        errors: { ...stats.errors },
        unknownObj: { ...stats.unknownObj },
      };
    },
  };
}
