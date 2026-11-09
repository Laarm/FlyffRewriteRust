// Messages between the main thread and the decode workers (Plan/40 section 8.2, Plan/30 section 14.3). This
// file is the single source of the job kinds; it holds types only (plus one const) so both tsconfig projects
// (app and worker) can include it. `DecodeKind` is Plan/30's and is imported, never re-listed.

import type { DecodeKind, FormatsInfo } from '../wasm/decoders';

export type { DecodeKind, FormatsInfo };

/** The kinds of the pool (Plan/40 8.2): every `decode_*` export plus the three non-wasm-decoder jobs. */
export type JobKind = DecodeKind | 'bitmap' | 'json' | 'chunk_check';

/** Jobs that only exist for the boot placeholder and liveness checks. */
export type LivenessKind = 'ping' | 'wasm-hello';

/** Kinds only accepted when the pool was created with `testHooks` (never in production). */
export type TestJobKind = 'trap' | 'mem_grow' | 'busy';

/** What a worker can be asked to do. */
export type WorkerJobKind = JobKind | LivenessKind | TestJobKind;

export interface WorkerCaps {
  /** Enables the {@link TestJobKind}s. */
  readonly testHooks: boolean;
}

/** `{ mode }` of the `trap` test job. */
export interface TrapOptions {
  /** `unreachable`: a real wasm trap; `throw`: an uncaught exception (worker `error` event). */
  readonly mode: 'unreachable' | 'throw';
}

export type WorkerRequest =
  | { readonly type: 'init'; readonly module: WebAssembly.Module; readonly caps: WorkerCaps }
  | {
      readonly type: 'job';
      readonly id: number;
      readonly kind: WorkerJobKind;
      /** Transferred in (detached on the sender). */
      readonly bytes: ArrayBuffer | null;
      readonly opts?: unknown;
    };

/** Result fields common to success and failure. */
interface ResultBase {
  readonly type: 'result';
  readonly id: number;
  /** `wasm.memory.buffer.byteLength` after the job (recycling decision, Plan/40 8.5). */
  readonly mem: number;
  /** Milliseconds the worker spent on the job. */
  readonly ms: number;
}

export type WorkerResponse =
  | { readonly type: 'ready'; readonly wasmVersion: string; readonly info: FormatsInfo }
  | { readonly type: 'fatal'; readonly error: string }
  | (ResultBase & {
      readonly ok: true;
      /** The meta of a decode, `{ bundle, schema, data }` of `json`, a number for `ping`, ... */
      readonly meta: unknown;
      /** Transferred out. */
      readonly blob: ArrayBuffer | null;
      /** `bitmap` jobs: transferred out. */
      readonly bitmap?: ImageBitmap;
    })
  | (ResultBase & {
      readonly ok: false;
      readonly error: string;
      /** `o3d`, `dds`, ... when the message names a format. */
      readonly format?: string;
      /** The wasm instance trapped: the worker is dead. */
      readonly trap?: boolean;
      /** The kind exists in the protocol but this worker build does not answer it yet. */
      readonly unsupported?: boolean;
    });

/** Result of a `json` job: the envelope of a data bundle (Plan/14 10). */
export interface JsonEnvelope {
  readonly bundle: string;
  readonly schema: number;
  readonly data: unknown;
}
