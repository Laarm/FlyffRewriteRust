// Observable boot status shown by the CSS-only boot screen while the screen state is `Boot` (Plan/40 section 3.1 B0):
// one line per boot step. Replaced by the screen of the current state (Login, ...) as soon as the state machine leaves
// `Boot`; the fatal screen needs none of it.

import { createStore, type StoreApi } from 'zustand/vanilla';

export type StepState = 'pending' | 'ok' | 'error';

export interface BootStep {
  readonly state: StepState;
  /** Human readable result: the wasm version, an error message, ... */
  readonly detail: string;
}

export interface BootStatus {
  readonly phase: 'starting' | 'ready' | 'failed';
  /** B1a: `GET /api/bootstrap` and the version guard. */
  readonly bootstrap: BootStep;
  /** B1b/B3: the wasm module. */
  readonly wasm: BootStep;
  /** B3: the decode workers. */
  readonly worker: BootStep;
  /** B4/B5: the pre-login bundles. */
  readonly data: BootStep;
  /** B1c: the fallback font. */
  readonly fonts: BootStep;
}

const pending = (detail: string): BootStep => ({ state: 'pending', detail });

export const initialBootStatus: BootStatus = {
  phase: 'starting',
  bootstrap: pending('contacting the server'),
  wasm: pending('loading'),
  worker: pending('starting'),
  data: pending('waiting'),
  fonts: pending('loading'),
};

export type BootStatusStore = StoreApi<BootStatus>;

export function createBootStatus(): BootStatusStore {
  return createStore<BootStatus>(() => initialBootStatus);
}
