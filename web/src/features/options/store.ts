// The options store (Plan/45 §2.3 `optionsStore`, §27.1): a persisted vanilla store over `flyff.options.v1`, defined in the
// context's registry (`options`, scope `never`), plus the views the subsystems read:
//
//   * `uiOptionsPort(store)`: the raw record `UiHost.options` wants (`windowAlpha`, `uiScale`, `gamma`, `bright`,
//     `contrast`, `chatFilters`, `chatFilterEtc`, `instantHelp`),
//   * `audioOptionsPort(store)`: the `audio` section (`createAudio` deps).

import type { StoreApi } from 'zustand/vanilla';
import type { AudioOptions } from '../../audio/options';
import type { StoreRegistry } from '../../stores/registry';
import {
  defaultOptions,
  forceOptions,
  OPTIONS_KEY,
  OPTIONS_VERSION,
  type OptionsV1,
} from './schema';

export type OptionsStore = StoreApi<OptionsV1>;

/** Defines (or returns) the `options` store of a registry. */
export function defineOptionsStore(registry: StoreRegistry): OptionsStore {
  const existing = registry.get('options');
  if (existing) return existing as unknown as OptionsStore;
  return registry.defineStore<OptionsV1>({
    name: 'options',
    scope: 'never',
    owner: '45',
    initial: defaultOptions,
    persist: {
      key: OPTIONS_KEY,
      version: OPTIONS_VERSION,
      forced: forceOptions,
    },
  });
}

export interface RecordPort {
  get(): Readonly<Record<string, unknown>>;
  subscribe(fn: (value: Readonly<Record<string, unknown>>) => void): () => void;
}

/** The UI's options as the flat record `uiHost` hands to 42 (`UiOptionsStore` normalises it). */
export function uiRecord(o: OptionsV1): Readonly<Record<string, unknown>> {
  return {
    windowAlpha: o.ui.windowAlpha,
    uiScale: o.ui.uiScale,
    chatFilters: o.ui.chatFilters,
    chatFilterEtc: o.ui.chatFilterEtc,
    instantHelp: o.ui.instantHelp,
    gamma: o.render.gamma,
    bright: o.render.bright,
    contrast: o.render.contrast,
  };
}

/** Memoises a projection so that `get()` returns the same object until the projected fields change. */
function projected(
  store: OptionsStore,
  project: (o: OptionsV1) => Readonly<Record<string, unknown>>,
): RecordPort {
  let last: { source: OptionsV1; value: Readonly<Record<string, unknown>> } | null = null;
  const get = (): Readonly<Record<string, unknown>> => {
    const state = store.getState();
    if (last?.source === state) return last.value;
    const value = project(state);
    last = { source: state, value };
    return value;
  };
  return {
    get,
    subscribe(fn) {
      return store.subscribe(() => {
        fn(get());
      });
    },
  };
}

export function uiOptionsPort(store: OptionsStore): RecordPort {
  return projected(store, uiRecord);
}

export function audioOptionsPort(store: OptionsStore): RecordPort {
  return projected(store, (o) => o.audio as unknown as Record<string, unknown>);
}

/** `setOption(path, value)` of the probe's test hook: `"audio.sfxStep"`, `"ui.uiScale"`, ... (e2e only). */
export function setOptionByPath(store: OptionsStore, path: string, value: unknown): void {
  const parts = path.split('.');
  const last = parts.pop();
  if (last === undefined || parts.length === 0)
    throw new Error(`option path "${path}" needs a section`);
  const state = structuredClone(store.getState()) as unknown as Record<string, unknown>;
  let node: Record<string, unknown> = state;
  for (const part of parts) {
    const next = node[part];
    if (typeof next !== 'object' || next === null) throw new Error(`no option section "${part}"`);
    node = next as Record<string, unknown>;
  }
  node[last] = value;
  store.setState(forceOptions(state as unknown as OptionsV1));
}

export type { AudioOptions };
