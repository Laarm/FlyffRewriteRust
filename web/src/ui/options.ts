// `UiOptionsStore` (Plan/42 §18, PL-128): the view of 45's options store that the UI reads. The host hands over a
// structural port (`UiHost.options`: `get()` of a raw record + `subscribe`), the store normalises it into the typed
// `UiOptions` (defaults, ranges) and notifies its own subscribers only when a field the UI reads changed.
//
// Consumers: `UiScaler` (`uiScale`), `UiRoot` (`windowAlpha`; title screens force 255), `UiRamp` (`gamma`,
// `bright`, `contrast`), the chat window of WP-UI-16 (`chatFilters`, `chatFilterEtc`, `instantHelp`).

import { DEFAULT_BRIGHT, DEFAULT_CONTRAST, DEFAULT_GAMMA } from '../render/rampLut';

/** The options of 45's store that the UI reads (§18 table). */
export interface UiOptions {
  /** `WindowAlpha` 0-255: the opacity of the skin canvases. */
  readonly windowAlpha: number;
  /** 0 = automatic (the §16 rule), else a scale in `[UI_SCALE_MIN, UI_SCALE_MAX]`. */
  readonly uiScale: number;
  readonly chatFilters: readonly number[];
  readonly chatFilterEtc: number;
  readonly instantHelp: boolean;
  readonly gamma: number;
  readonly bright: number;
  readonly contrast: number;
}

/** The explicit scale range (§16). */
export const UI_SCALE_MIN = 0.6;
export const UI_SCALE_MAX = 2.0;

export const DEFAULT_UI_OPTIONS: UiOptions = Object.freeze({
  windowAlpha: 255,
  uiScale: 0,
  chatFilters: Object.freeze<number[]>([]),
  chatFilterEtc: 0,
  instantHelp: true,
  gamma: DEFAULT_GAMMA,
  bright: DEFAULT_BRIGHT,
  contrast: DEFAULT_CONTRAST,
});

/** The port of the host (`UiHostContract.options`: 40's adapter over 45's `optionsStore`). */
export interface UiOptionsSource {
  get(): Readonly<Record<string, unknown>>;
  subscribe(fn: (options: Readonly<Record<string, unknown>>) => void): () => void;
}

const num = (v: unknown): number | undefined =>
  typeof v === 'number' && Number.isFinite(v) ? v : undefined;

/**
 * Turns a raw options record into typed options: a missing, mistyped or out-of-range field takes its default or is
 * clamped (`windowAlpha` into 0-255, `uiScale` 0 or into 0.6-2.0, `gamma` > 0, `contrast` >= 0, `bright` rounded).
 */
export function normalizeUiOptions(raw: Readonly<Record<string, unknown>>): UiOptions {
  const d = DEFAULT_UI_OPTIONS;
  const alpha = num(raw['windowAlpha']);
  const scale = num(raw['uiScale']);
  const gamma = num(raw['gamma']);
  const bright = num(raw['bright']);
  const contrast = num(raw['contrast']);
  const etc = num(raw['chatFilterEtc']);
  const filters = raw['chatFilters'];
  return {
    windowAlpha:
      alpha === undefined ? d.windowAlpha : Math.min(255, Math.max(0, Math.round(alpha))),
    uiScale:
      scale === undefined || scale <= 0 ? 0 : Math.min(UI_SCALE_MAX, Math.max(UI_SCALE_MIN, scale)),
    chatFilters: Array.isArray(filters)
      ? filters.filter((f): f is number => typeof f === 'number' && Number.isFinite(f))
      : d.chatFilters,
    chatFilterEtc: etc === undefined ? d.chatFilterEtc : Math.trunc(etc),
    instantHelp: typeof raw['instantHelp'] === 'boolean' ? raw['instantHelp'] : d.instantHelp,
    gamma: gamma !== undefined && gamma > 0 ? gamma : d.gamma,
    bright: bright === undefined ? d.bright : Math.round(bright),
    contrast: contrast !== undefined && contrast >= 0 ? contrast : d.contrast,
  };
}

function sameOptions(a: UiOptions, b: UiOptions): boolean {
  return (
    a.windowAlpha === b.windowAlpha &&
    a.uiScale === b.uiScale &&
    a.chatFilterEtc === b.chatFilterEtc &&
    a.instantHelp === b.instantHelp &&
    a.gamma === b.gamma &&
    a.bright === b.bright &&
    a.contrast === b.contrast &&
    a.chatFilters.length === b.chatFilters.length &&
    a.chatFilters.every((v, i) => v === b.chatFilters[i])
  );
}

type Listener = (options: UiOptions) => void;

/**
 * The typed view. `set(patch)` records an override ON TOP of the host's values (the gallery and the tests drive the
 * UI through it, `ramp=off` / `scale=` ...); the real options window writes 45's store, which this view follows.
 */
export class UiOptionsStore {
  readonly #source: UiOptionsSource | null;
  readonly #listeners = new Set<Listener>();
  #overrides: Partial<Record<string, unknown>> = {};
  #value: UiOptions;
  #unsubscribe: (() => void) | null = null;

  constructor(source?: UiOptionsSource | null) {
    this.#source = source ?? null;
    this.#value = this.#read();
    // a store that is never subscribed to keeps no host listener: it attaches with the first subscriber
  }

  #read(): UiOptions {
    let raw: Readonly<Record<string, unknown>>;
    try {
      raw = this.#source?.get() ?? {};
    } catch {
      raw = {};
    }
    return normalizeUiOptions({ ...raw, ...this.#overrides });
  }

  /** The current options (the same object until a field the UI reads changed). */
  readonly get = (): UiOptions => {
    if (this.#unsubscribe === null) this.#value = this.#refresh(this.#value);
    return this.#value;
  };

  #refresh(prev: UiOptions): UiOptions {
    const next = this.#read();
    return sameOptions(prev, next) ? prev : next;
  }

  /** Overrides fields (`uiScale`, `windowAlpha`, `gamma`, ...) on top of the host's options. */
  set(patch: Partial<Record<keyof UiOptions, unknown>>): void {
    this.#overrides = { ...this.#overrides, ...patch };
    this.#changed();
  }

  /** Drops every override: the host's values apply again. */
  clearOverrides(): void {
    this.#overrides = {};
    this.#changed();
  }

  #changed(): void {
    const next = this.#refresh(this.#value);
    if (next === this.#value) return;
    this.#value = next;
    for (const fn of [...this.#listeners]) fn(next);
  }

  readonly subscribe = (fn: Listener): (() => void) => {
    this.#listeners.add(fn);
    if (this.#unsubscribe === null && this.#source) {
      try {
        this.#unsubscribe = this.#source.subscribe(() => {
          this.#changed();
        });
      } catch {
        this.#unsubscribe = null;
      }
      this.#value = this.#refresh(this.#value);
    }
    return () => {
      this.#listeners.delete(fn);
      if (this.#listeners.size === 0 && this.#unsubscribe) {
        this.#unsubscribe();
        this.#unsubscribe = null;
      }
    };
  };

  /** Number of subscribers (leak checks in tests). */
  get listenerCount(): number {
    return this.#listeners.size;
  }
}
