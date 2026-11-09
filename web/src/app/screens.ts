// Screen bodies per `ScreenState` (Plan/40 section 4.1, 45 section 1.6): doc 40's state machine decides the
// state, doc 45 renders the body of each state. Bodies register here (at boot, before or after the first
// render: `Root` re-renders when the registry changes); until one is registered `Root` shows a placeholder.

import type { ComponentType } from 'react';
import { createStore, type StoreApi } from 'zustand/vanilla';
import type { Unsubscribe } from '../platform/visibility';
import type { ScreenName } from '../stores/screen';
import type { ClientContext } from './context';

export type ScreenBody = ComponentType<{ readonly ctx: ClientContext }>;

export interface ScreenRegistryState {
  readonly bodies: Readonly<Partial<Record<ScreenName, ScreenBody>>>;
}

export interface ScreenRegistry {
  /** Registers the body of a screen (replaces an earlier one); returns the unregister function. */
  register(name: ScreenName, body: ScreenBody): Unsubscribe;
  get(name: ScreenName): ScreenBody | undefined;
  /** The observable state `Root` subscribes to. */
  readonly store: StoreApi<ScreenRegistryState>;
}

export function createScreenRegistry(): ScreenRegistry {
  const store = createStore<ScreenRegistryState>(() => ({ bodies: {} }));
  return {
    register(name, body) {
      store.setState((s) => ({ bodies: { ...s.bodies, [name]: body } }));
      return () => {
        if (store.getState().bodies[name] !== body) return;
        store.setState((s) => ({
          bodies: Object.fromEntries(
            Object.entries(s.bodies).filter(([key]) => key !== name),
          ) as ScreenRegistryState['bodies'],
        }));
      };
    },
    get: (name) => store.getState().bodies[name],
    store,
  };
}
