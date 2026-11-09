// Creating a `SimWorld` outside the game's `SimBridge` (WP-CLIENT-18): the renderer's dev pages, the preview fixture
// page and tests. The module must be loaded (`ensureWasm`). Only `wasm/` may import `wasm/pkg` (layering rules).

import { SimWorld } from './pkg/flyff_wasm';
import { wasmMemory } from './loader';
import type { SimWorldHandle } from './simTypes';

/** `[flags bits, hit_attr_rule, seed, remote_extrap_cap_frames]` (Plan/31 section 17.3 `SimWorld::new`). */
export const DEFAULT_SIM_CONFIG: readonly number[] = [31, 0, 7, 37];

/** A new `SimWorld` (no world loaded; preview movers work without one). */
export function createSimWorld(config: ArrayLike<number> = DEFAULT_SIM_CONFIG): SimWorldHandle {
  return new SimWorld(Uint32Array.from(Array.from(config)));
}

/** The linear memory of the main instance (re-create typed views from it after EVERY call, rule B4). */
export function simMemory(): WebAssembly.Memory {
  const memory = wasmMemory();
  if (memory === null) throw new Error('the wasm module is not loaded (call ensureWasm first)');
  return memory;
}
