// Loading the wasm module (Plan/40 section 7.1, ADR-015): compile once with streaming, instantiate on
// the main thread, post the `WebAssembly.Module` to every worker (which calls `initSync`).
// Only wasm/, workers/, game/simBridge.ts and rules/ may import wasm/pkg (eslint layering rules).

import init, { hello, initSync, selftest_det, version, type InitOutput } from './pkg/flyff_wasm';
import wasmUrl from './pkg/flyff_wasm_bg.wasm?url';

/** The exports of the module that are not decoders (the decoders are in `decoders.ts`). */
export interface WasmApi {
  /** Workspace version of `flyff-wasm`. */
  version(): string;
  /** Greeting used to prove the module is callable. */
  hello(name: string): string;
  /** Determinism self-test hash (Plan/02 9.5). */
  selftestDet(seed: number): bigint;
}

export interface LoadedWasm {
  /** Compiled once; posted to the decode workers. */
  readonly module: WebAssembly.Module;
  readonly api: WasmApi;
}

/** One sample of the linear memory of the main instance (Plan/40 7.11), for the debug probe. */
export interface MemorySample {
  /** `memory.buffer.byteLength`. */
  readonly bytes: number;
  /** 64 KiB pages. */
  readonly pages: number;
}

const api: WasmApi = {
  version: () => version(),
  hello: (name) => hello(name),
  selftestDet: (seed) => selftest_det(seed),
};

/** The compiled module and the instance of THIS realm (set by `loadWasm`/`ensureWasm`/`instantiateSync`). */
let currentModule: WebAssembly.Module | null = null;
let currentExports: InitOutput | null = null;
let pending: Promise<LoadedWasm> | null = null;

/**
 * `WebAssembly.compileStreaming(fetch(url))`; when that throws a `TypeError` (wrong MIME type behind a
 * proxy, old browser) the bytes are fetched and compiled instead.
 */
export async function compileModule(
  url: string = wasmUrl,
  fetchFn: typeof fetch = fetch,
  compileStreaming: typeof WebAssembly.compileStreaming = WebAssembly.compileStreaming.bind(
    WebAssembly,
  ),
): Promise<WebAssembly.Module> {
  try {
    return await compileStreaming(fetchFn(url));
  } catch (e) {
    if (!(e instanceof TypeError)) throw e;
    const response = await fetchFn(url);
    if (!response.ok) {
      throw new Error(`wasm fetch failed: HTTP ${String(response.status)}`, { cause: e });
    }
    return WebAssembly.compile(await response.arrayBuffer());
  }
}

/** Compiles and instantiates the module on the calling thread. Prefer {@link ensureWasm}. */
export async function loadWasm(url?: string, fetchFn?: typeof fetch): Promise<LoadedWasm> {
  const module = await compileModule(url, fetchFn);
  currentExports = await init({ module_or_path: module });
  currentModule = module;
  return { module, api };
}

/**
 * Idempotent main-thread load (boot step B3): every caller gets the same promise, so the module is
 * downloaded, compiled and instantiated exactly once per page. A failed load can be retried.
 */
export function ensureWasm(url?: string, fetchFn?: typeof fetch): Promise<LoadedWasm> {
  pending ??= loadWasm(url, fetchFn).catch((e: unknown) => {
    pending = null;
    throw e;
  });
  return pending;
}

/** The compiled module, for posting to the decode workers; throws before `ensureWasm` resolved. */
export function getModule(): WebAssembly.Module {
  if (!currentModule) throw new Error('the wasm module is not loaded yet (call ensureWasm first)');
  return currentModule;
}

/** `true` once the module of this realm is instantiated. */
export function isWasmLoaded(): boolean {
  return currentExports !== null;
}

/** Synchronous instantiation from bytes or a compiled module (workers, Node tests). */
export function instantiateSync(source: BufferSource | WebAssembly.Module): WasmApi {
  currentExports = initSync({ module: source });
  if (source instanceof WebAssembly.Module) currentModule = source;
  return api;
}

/** The linear memory of this realm's instance (`null` before instantiation). */
export function wasmMemory(): WebAssembly.Memory | null {
  return currentExports?.memory ?? null;
}

/**
 * Samples `memory.buffer.byteLength` (the probe samples it at 1 Hz; above 240 MB the bridge warns, Plan/40
 * 7.11). `null` before instantiation. Reading `buffer` does not call into wasm.
 */
export function sampleMemory(): MemorySample | null {
  const memory = wasmMemory();
  if (!memory) return null;
  const bytes = memory.buffer.byteLength;
  return { bytes, pages: bytes / 65536 };
}

/** Test seam: forget the module of this realm. */
export function resetWasmForTests(): void {
  currentModule = null;
  currentExports = null;
  pending = null;
}
