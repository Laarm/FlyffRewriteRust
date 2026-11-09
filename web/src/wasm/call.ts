// The boundary-call wrapper (Plan/40 section 7.3 and 7.8, WP-CLIENT-03): every call into the main-thread
// instance (`SimWorld` and `RulesContext`) goes through `wasmCall`, which
//   * classifies failures: an `Error` returned by an export (`Result::Err` -> `JsError`) is RECOVERABLE (the
//     caller decides: asset -> failed + placeholder, command buffer -> L2 resync); a wasm trap
//     (`WebAssembly.RuntimeError`, or the RangeError of a wasm stack overflow) makes the instance unusable and
//     fires the recovery hook (L3 reload, wired by WP-CLIENT-13) exactly once;
//   * counts the call for the per-frame boundary statistics (B3): `callsThisFrame`, a counter per category,
//     `maxCalls`, `overBudget` (iterations above a diagnostic threshold). The counters are TELEMETRY: no call is ever
//     refused or deferred because of a count (owner 2026-10-29; the ~10 per iteration of Plan/40 7.3 are reported only).
// The loop calls `endWasmFrame()` at the end of each iteration (Plan/40 10.2 step 12).
//
// This module holds module-level state on purpose: there is ONE main instance per page. Tests reset it with
// `resetWasmCalls()`.

import { classifyWasmError } from './decoders';

/**
 * Budget categories of Plan/40 7.3: F fixed, C camera, Q queries, L loads, R rules, U UI-driven (outside the sum),
 * O one-time calls of the boot and of the session inputs (`load_motion_table`, ...; outside the sum, never counted).
 */
export type CallCategory = 'F' | 'C' | 'Q' | 'L' | 'R' | 'U' | 'O';

/** The categories that count toward the per-iteration sum. */
export type BudgetCategory = Exclude<CallCategory, 'U' | 'O'>;

/**
 * A diagnostic threshold, never a limit: an iteration with more budgeted calls is counted in `overBudget` and reported
 * through `onOverBudget` (a runaway loop). No call is refused or deferred because of a count (owner 2026-10-29); the
 * numbers of the Plan/40 7.3 table (F 5, C 1, Q 2, L 1-3, R 1) are reported by the counters (`maxCalls`, `maxByCategory`),
 * never enforced.
 */
export const FRAME_CALL_BUDGET = 1000;

/** Per-category diagnostic thresholds: every category has the same one as the sum (no per-category cap). */
export const CATEGORY_CAPS: Readonly<Record<BudgetCategory, number>> = {
  F: FRAME_CALL_BUDGET,
  C: FRAME_CALL_BUDGET,
  Q: FRAME_CALL_BUDGET,
  L: FRAME_CALL_BUDGET,
  R: FRAME_CALL_BUDGET,
};

/** Counters of the boundary calls (a copy; see {@link callStats}). */
export interface CallStats {
  /** Budgeted calls (F, C, Q, L, R) of the current iteration. */
  readonly callsThisFrame: number;
  /** Largest `callsThisFrame` of any finished iteration. */
  readonly maxCalls: number;
  /** Calls of the current iteration per category. */
  readonly byCategory: Readonly<Record<BudgetCategory, number>>;
  /** Largest per-category count of any finished iteration (`__flyffDebug.loop().wasmCallsByCategoryMax`). */
  readonly maxByCategory: Readonly<Record<BudgetCategory, number>>;
  /** Largest number of UI-driven (category U) calls of any finished iteration. */
  readonly maxUiCalls: number;
  /** Iterations that finished above the diagnostic threshold {@link FRAME_CALL_BUDGET} (a runaway loop; nothing is refused). */
  readonly overBudget: number;
  /** UI-driven calls (category U), cumulative; not capped. */
  readonly uiCalls: number;
  /** Calls of every category since the start. */
  readonly totalCalls: number;
  /** Iterations finished by {@link endWasmFrame}. */
  readonly frames: number;
  /** Recoverable errors seen. */
  readonly errors: number;
  /** `true` once a trap was seen: the instance is unusable. */
  readonly trapped: boolean;
}

/** Why a call failed. */
export type WasmCallFailure = 'recoverable' | 'trap';

/**
 * Thrown by {@link wasmCall} instead of the raw error, so callers can tell the two kinds apart without
 * inspecting wasm-bindgen internals. `cause` is the original error.
 */
export class WasmCallError extends Error {
  readonly kind: WasmCallFailure;
  /** The export that failed (`name` argument of `wasmCall`). */
  readonly fn: string;

  constructor(kind: WasmCallFailure, fn: string, cause: unknown) {
    const detail = cause instanceof Error ? cause.message : String(cause);
    super(`wasm ${fn} ${kind === 'trap' ? 'trapped' : 'failed'}: ${detail}`, { cause });
    this.name = 'WasmCallError';
    this.kind = kind;
    this.fn = fn;
  }
}

/** Known export names and their category (Plan/40 7.3); unknown names count as category `Q`. */
const KNOWN_CATEGORY: Readonly<Record<string, CallCategory>> = {
  apply_commands: 'F',
  local_input: 'F',
  step: 'F',
  take_events: 'F',
  render_view: 'F',
  camera_check_bound: 'C',
  local_act: 'Q',
  ray_cast: 'Q',
  land_height: 'Q',
  full_height: 'Q',
  height_attr: 'Q',
  blink_point: 'Q',
  target_check: 'Q',
  range_point: 'Q',
  set_world: 'L',
  reset_render_smoothing: 'L',
  load_chunk: 'L',
  unload_chunk: 'L',
  add_skeleton: 'L',
  add_clip: 'L',
  memory_report: 'L',
  derived_stats: 'R',
  rules_sync: 'R',
  equip_check: 'U',
  item_multiplier: 'U',
  stat_preview: 'U',
  skill_plan_check: 'U',
};

export interface WasmCallHooks {
  /** Called exactly once, on the first trap (the L3 hook of the recovery ladder, WP-CLIENT-13). */
  onTrap?: (error: WasmCallError) => void;
  /** Called for every recoverable error (logging). */
  onRecoverable?: (error: WasmCallError) => void;
  /**
   * Called when an iteration ends above the diagnostic threshold ({@link FRAME_CALL_BUDGET}): reported, nothing is
   * refused (owner 2026-10-29); the count is in `stats.overBudget`.
   */
  onOverBudget?: (info: { calls: number; byCategory: Readonly<Record<string, number>> }) => void;
}

interface State {
  calls: number;
  byCategory: Record<BudgetCategory, number>;
  maxByCategory: Record<BudgetCategory, number>;
  maxCalls: number;
  overBudget: number;
  uiCalls: number;
  uiFrame: number;
  maxUiCalls: number;
  totalCalls: number;
  frames: number;
  errors: number;
  trapped: boolean;
  trapNotified: boolean;
  hooks: WasmCallHooks;
}

function fresh(): State {
  return {
    calls: 0,
    byCategory: { F: 0, C: 0, Q: 0, L: 0, R: 0 },
    maxByCategory: { F: 0, C: 0, Q: 0, L: 0, R: 0 },
    maxCalls: 0,
    overBudget: 0,
    uiCalls: 0,
    uiFrame: 0,
    maxUiCalls: 0,
    totalCalls: 0,
    frames: 0,
    errors: 0,
    trapped: false,
    trapNotified: false,
    hooks: {},
  };
}

let state = fresh();

/** Installs the hooks (replaces the previous ones). */
export function setWasmCallHooks(hooks: WasmCallHooks): void {
  state.hooks = hooks;
}

/** Category of an export name (Plan/40 7.3 table); unknown names are `Q`. */
export function categoryOf(name: string): CallCategory {
  return KNOWN_CATEGORY[name] ?? 'Q';
}

/**
 * Runs one call into wasm: counts it, classifies a failure and rethrows it as {@link WasmCallError}.
 * After a trap every further call throws a trap error without running `fn` (the instance is corrupt).
 */
export function wasmCall<T>(name: string, fn: () => T, category?: CallCategory): T {
  if (state.trapped) {
    throw new WasmCallError('trap', name, new Error('the wasm instance trapped earlier'));
  }
  const cat = category ?? categoryOf(name);
  state.totalCalls += 1;
  if (cat === 'U') {
    state.uiCalls += 1;
    state.uiFrame += 1;
  } else if (cat === 'O') {
    // one-time calls outside the budget: only `totalCalls` knows them
  } else {
    state.calls += 1;
    state.byCategory[cat] += 1;
  }
  try {
    return fn();
  } catch (e) {
    const kind: WasmCallFailure = classifyWasmError(e) === 'trap' ? 'trap' : 'recoverable';
    const error = new WasmCallError(kind, name, e);
    if (kind === 'trap') {
      state.trapped = true;
      if (!state.trapNotified) {
        state.trapNotified = true;
        state.hooks.onTrap?.(error);
      }
    } else {
      state.errors += 1;
      state.hooks.onRecoverable?.(error);
    }
    throw error;
  }
}

/** `true` when `e` is a {@link WasmCallError} of the given kind. */
export function isWasmCallError(e: unknown, kind?: WasmCallFailure): e is WasmCallError {
  return e instanceof WasmCallError && (kind === undefined || e.kind === kind);
}

/**
 * Ends one loop iteration (Plan/40 10.2 step 12): folds the counters into `maxCalls` / `overBudget` and
 * resets the per-iteration counts. Calls made between iterations (DOM handlers) count toward the next one.
 */
export function endWasmFrame(): void {
  const s = state;
  s.frames += 1;
  s.maxCalls = Math.max(s.maxCalls, s.calls);
  for (const c of Object.keys(s.byCategory) as BudgetCategory[]) {
    s.maxByCategory[c] = Math.max(s.maxByCategory[c], s.byCategory[c]);
  }
  s.maxUiCalls = Math.max(s.maxUiCalls, s.uiFrame);
  s.uiFrame = 0;
  const overSum = s.calls > FRAME_CALL_BUDGET;
  const overCap = (Object.keys(CATEGORY_CAPS) as (keyof typeof CATEGORY_CAPS)[]).some(
    (c) => s.byCategory[c] > CATEGORY_CAPS[c],
  );
  const info = { calls: s.calls, byCategory: { ...s.byCategory } };
  s.calls = 0;
  s.byCategory = { F: 0, C: 0, Q: 0, L: 0, R: 0 };
  if (overSum || overCap) {
    s.overBudget += 1;
    s.hooks.onOverBudget?.(info);
  }
}

/** Calls of budget category `cat` in the current iteration (no allocation; the bridge's once-per-iteration camera answer reads it). */
export function categoryCalls(cat: BudgetCategory): number {
  return state.byCategory[cat];
}

/** A snapshot of the counters (never the live object). */
export function callStats(): CallStats {
  return {
    callsThisFrame: state.calls,
    maxCalls: state.maxCalls,
    byCategory: { ...state.byCategory },
    maxByCategory: { ...state.maxByCategory },
    maxUiCalls: state.maxUiCalls,
    overBudget: state.overBudget,
    uiCalls: state.uiCalls,
    totalCalls: state.totalCalls,
    frames: state.frames,
    errors: state.errors,
    trapped: state.trapped,
  };
}

/** Test seam (and the L3 reload path): forgets counters, the trap state and the hooks. */
export function resetWasmCalls(): void {
  state = fresh();
}
