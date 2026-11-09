// Wasm access for the client (Plan/40 section 7): loader (compile once, main instance, module for
// workers), the boundary-call wrapper (WP-CLIENT-03), memory-growth-safe views and the typed decoders (Plan/30).
export {
  compileModule,
  ensureWasm,
  getModule,
  instantiateSync,
  isWasmLoaded,
  loadWasm,
  sampleMemory,
  wasmMemory,
  type LoadedWasm,
  type MemorySample,
  type WasmApi,
} from './loader';
export {
  callStats,
  CATEGORY_CAPS,
  categoryOf,
  endWasmFrame,
  FRAME_CALL_BUDGET,
  isWasmCallError,
  setWasmCallHooks,
  wasmCall,
  WasmCallError,
  type CallCategory,
  type CallStats,
  type WasmCallFailure,
  type WasmCallHooks,
} from './call';
export { MemoryViews, type TypedArrayCtor, type ViewHandle, type ViewLocation } from './memory';
export * from './decoders';
export * from './simTypes';
export {
  constructRulesContext,
  type RulesBundlesBytes,
  type RulesContextHandle,
} from './rulesContext';
