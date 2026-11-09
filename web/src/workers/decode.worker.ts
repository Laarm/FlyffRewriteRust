// Decode worker entry (Plan/40 section 8, Plan/30 section 14). One job at a time: the pool never sends a job
// to a busy worker. Receives the compiled wasm module from the main thread, instantiates it with `initSync` and
// answers jobs with `{ type: 'result', id, ok, ... }`; blobs and bitmaps are transferred, never copied.

import { initSync, hello, version } from '../wasm/pkg/flyff_wasm';
import {
  checkChunk,
  classifyWasmError,
  DECODE_KINDS,
  errorFormat,
  formatsInfo,
  runDecode,
  type DecodeKind,
} from '../wasm/decoders';
import type {
  JsonEnvelope,
  TestJobKind,
  WorkerCaps,
  WorkerRequest,
  WorkerResponse,
} from './protocol';

let memory: WebAssembly.Memory | null = null;
let caps: WorkerCaps = { testHooks: false };

function memoryBytes(): number {
  return memory?.buffer.byteLength ?? 0;
}

function post(message: WorkerResponse, transfer: Transferable[] = []): void {
  self.postMessage(message, transfer);
}

function isDecodeKind(kind: string): kind is DecodeKind {
  return (DECODE_KINDS as readonly string[]).includes(kind);
}

/** A 1-function wasm module whose only export executes `unreachable` (the `trap` test job). */
const UNREACHABLE_MODULE = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 7, 5, 1, 1, 116, 0, 0, 10, 5, 1, 3, 0,
  0, 11,
]);

function busyWait(ms: number): void {
  const end = performance.now() + ms;
  while (performance.now() < end) {
    // spin: the `busy` test job keeps a worker occupied for a known time
  }
}

type JobRequest = Extract<WorkerRequest, { type: 'job' }>;

function ok(
  id: number,
  t0: number,
  meta: unknown,
  blob: ArrayBuffer | null,
  extra?: { bitmap: ImageBitmap },
): void {
  const base = {
    type: 'result',
    id,
    ok: true,
    meta,
    blob,
    mem: memoryBytes(),
    ms: performance.now() - t0,
  } as const;
  const transfer: Transferable[] = [];
  if (blob) transfer.push(blob);
  if (extra) transfer.push(extra.bitmap);
  post(extra ? { ...base, bitmap: extra.bitmap } : base, transfer);
}

function fail(
  id: number,
  t0: number,
  error: string,
  flags: { format?: string; trap?: boolean; unsupported?: boolean } = {},
): void {
  post({
    type: 'result',
    id,
    ok: false,
    error,
    ...(flags.format === undefined ? {} : { format: flags.format }),
    ...(flags.trap ? { trap: true } : {}),
    ...(flags.unsupported ? { unsupported: true } : {}),
    mem: memoryBytes(),
    ms: performance.now() - t0,
  });
}

async function runJob(job: JobRequest): Promise<void> {
  const t0 = performance.now();
  const { id, kind } = job;
  try {
    if (isDecodeKind(kind)) {
      if (!job.bytes) {
        fail(id, t0, `${kind}: the job carries no bytes`);
        return;
      }
      const { meta, blob } = runDecode(kind, new Uint8Array(job.bytes), job.opts as never);
      ok(id, t0, meta, blob ? (blob.buffer as ArrayBuffer) : null);
      return;
    }
    switch (kind) {
      case 'bitmap': {
        if (!job.bytes) {
          fail(id, t0, 'bitmap: the job carries no bytes');
          return;
        }
        const bitmap = await createImageBitmap(new Blob([job.bytes]), {
          premultiplyAlpha: 'none',
          colorSpaceConversion: 'none',
        });
        ok(id, t0, { width: bitmap.width, height: bitmap.height }, null, { bitmap });
        return;
      }
      case 'json': {
        if (!job.bytes) {
          fail(id, t0, 'json: the job carries no bytes');
          return;
        }
        const parsed = JSON.parse(
          new TextDecoder().decode(job.bytes),
        ) as Partial<JsonEnvelope> | null;
        if (
          parsed === null ||
          typeof parsed !== 'object' ||
          typeof parsed.bundle !== 'string' ||
          typeof parsed.schema !== 'number'
        ) {
          fail(id, t0, 'json: not a bundle envelope {bundle, schema, data}', { format: 'json' });
          return;
        }
        ok(id, t0, { bundle: parsed.bundle, schema: parsed.schema, data: parsed.data }, null);
        return;
      }
      case 'chunk_check': {
        if (!job.bytes) {
          fail(id, t0, 'chunk_check: the job carries no bytes');
          return;
        }
        // validation only (Plan/31 17.2): the output is the same bytes as a fresh transferable copy
        const checked = checkChunk(new Uint8Array(job.bytes));
        ok(id, t0, { bytes: checked.byteLength }, checked.buffer as ArrayBuffer);
        return;
      }
      case 'ping': {
        const payload = (job.opts as { payload?: number } | undefined)?.payload ?? 0;
        ok(id, t0, payload + 1, null);
        return;
      }
      case 'wasm-hello': {
        const name = (job.opts as { name?: string } | undefined)?.name ?? '';
        ok(id, t0, hello(name), null);
        return;
      }
      case 'trap':
      case 'mem_grow':
      case 'busy':
        if (!caps.testHooks) {
          fail(id, t0, `${kind}: test jobs are disabled`, { unsupported: true });
          return;
        }
        runTestJob(job, kind, t0);
        return;
    }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    const trap = classifyWasmError(e) === 'trap';
    const format = errorFormat(message);
    fail(id, t0, message, { ...(format === undefined ? {} : { format }), trap });
    if (trap) self.close(); // the instance is unusable; the pool respawns this worker
  }
}

function runTestJob(job: JobRequest, kind: TestJobKind, t0: number): void {
  const opts = (job.opts ?? {}) as { mode?: string; pages?: number; ms?: number };
  switch (kind) {
    case 'trap':
      if (opts.mode === 'throw') {
        // an uncaught exception: the page sees an `error` event on the worker
        queueMicrotask(() => {
          throw new Error('trap test job: uncaught exception');
        });
        return;
      }
      // a real wasm trap, classified like any other
      (
        new WebAssembly.Instance(new WebAssembly.Module(UNREACHABLE_MODULE)).exports[
          't'
        ] as () => void
      )();
      return;
    case 'mem_grow':
      memory?.grow(opts.pages ?? 1);
      ok(job.id, t0, memoryBytes(), null);
      return;
    case 'busy':
      busyWait(opts.ms ?? 10);
      ok(job.id, t0, null, null);
      return;
  }
}

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  const request = event.data;
  if (request.type === 'init') {
    try {
      memory = initSync({ module: request.module }).memory;
      caps = request.caps;
      post({ type: 'ready', wasmVersion: version(), info: formatsInfo() });
    } catch (e) {
      post({ type: 'fatal', error: e instanceof Error ? e.message : String(e) });
    }
    return;
  }
  void runJob(request);
};
