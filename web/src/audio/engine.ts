// `AudioEngine` (Plan/44 §4, WP-AUDIO-01): ONE `AudioContext` for SFX and BGM, created suspended at boot and resumed on
// the first user gesture; the bus graph; the visibility policy; the failure policy.
//
//   sfx sources -> [panner] -> sfxBus (sfxGain(step) * hiddenFactor) -+
//   music element A -> gainA -+                                      +-> masterBus (masterStep / 10) -> destination
//   music element B -> gainB -+-> bgmBus (bgmStep / 10 * bgHiddenFactor) +
//
// The listener is fixed at the origin with a constant orientation (A6): front `mirrorPos(0, 0, -1) = (0, 0, 1)`, up
// `(0, 1, 0)`; it is written ONCE. States: `unavailable` (no Web Audio), `locked`, `running`, `interrupted`, `closed`.
// Nothing here throws into a caller (44 §2.3): every failure is counted and logged at debug level.

import type { Logger } from '../platform/log';
import type { Unsubscribe } from '../platform/visibility';
import { mirrorPos } from '../render/space';
import {
  type AudioOptions,
  bgmGain,
  DEFAULT_AUDIO_OPTIONS,
  masterGain,
  normalizeAudioOptions,
  sameAudioOptions,
  sfxGain,
  sfxPlayable,
} from './options';

export type AudioState = 'unavailable' | 'locked' | 'running' | 'interrupted' | 'closed';

// ---- the structural parts of the Web Audio API the engine uses (fakes in tests) --------------------------------------

export interface AudioParamLike {
  value: number;
  setValueAtTime(value: number, time: number): unknown;
  linearRampToValueAtTime(value: number, time: number): unknown;
  setTargetAtTime(target: number, time: number, timeConstant: number): unknown;
  cancelScheduledValues(time: number): unknown;
}

export interface AudioNodeLike {
  connect(destination: unknown): unknown;
  disconnect?(): void;
}

export interface GainNodeLike extends AudioNodeLike {
  readonly gain: AudioParamLike;
}

export interface AudioListenerLike {
  readonly forwardX?: AudioParamLike;
  readonly forwardY?: AudioParamLike;
  readonly forwardZ?: AudioParamLike;
  readonly upX?: AudioParamLike;
  readonly upY?: AudioParamLike;
  readonly upZ?: AudioParamLike;
  setOrientation?(x: number, y: number, z: number, ux: number, uy: number, uz: number): void;
}

export interface AudioBufferLike {
  readonly length: number;
  /** Seconds (absent in the minimal fakes of the engine tests). */
  readonly duration?: number;
  readonly numberOfChannels?: number;
  readonly sampleRate?: number;
  copyToChannel?(source: Float32Array, channelNumber: number): void;
}

export interface BufferSourceLike extends AudioNodeLike {
  buffer: AudioBufferLike | null;
  /** Fires when the source finished or was stopped. */
  onended: (() => void) | null;
  start(when?: number): void;
  stop(when?: number): void;
}

/** The parts of a `PannerNode` the SFX player sets (44 §4.1). */
export interface PannerLike extends AudioNodeLike {
  panningModel: string;
  distanceModel: string;
  refDistance: number;
  maxDistance: number;
  rolloffFactor: number;
  coneInnerAngle: number;
  coneOuterAngle: number;
  coneOuterGain: number;
  readonly positionX?: AudioParamLike;
  readonly positionY?: AudioParamLike;
  readonly positionZ?: AudioParamLike;
  setPosition?(x: number, y: number, z: number): void;
}

export interface AudioContextLike {
  readonly state: string;
  readonly currentTime: number;
  readonly destination: unknown;
  readonly listener: AudioListenerLike;
  resume(): Promise<void>;
  close(): Promise<void>;
  createGain(): GainNodeLike;
  createBuffer(channels: number, length: number, sampleRate: number): AudioBufferLike;
  createBufferSource(): BufferSourceLike;
  /** Positional SFX voices (44 §7.1). */
  createPanner(): PannerLike;
  /** Decodes an encoded file (WAV for the sound effects). */
  decodeAudioData(data: ArrayBuffer): Promise<AudioBufferLike>;
  /** Routes a media element into the graph (music); absent in minimal fakes. */
  createMediaElementSource?(element: unknown): AudioNodeLike;
  addEventListener(type: 'statechange', fn: () => void): void;
  removeEventListener(type: 'statechange', fn: () => void): void;
}

/** What the engine listens on for the unlock gesture: `window` in the browser. */
export interface GestureTarget {
  addEventListener(type: string, fn: (e: Event) => void, options?: unknown): void;
  removeEventListener(type: string, fn: (e: Event) => void, options?: unknown): void;
}

export interface EngineDeps {
  readonly log: Logger;
  readonly visibility: {
    on(fn: (e: 'hidden' | 'visible' | 'pagehide') => void): Unsubscribe;
  };
  readonly options: {
    get(): Readonly<Record<string, unknown>> | AudioOptions;
    subscribe(fn: (o: Readonly<Record<string, unknown>> | AudioOptions) => void): Unsubscribe;
  };
  /** The context factory (default: `AudioContext` / `webkitAudioContext` of the page; `null` = no Web Audio). */
  readonly createContext?: () => AudioContextLike | null;
  /** The gesture event target (default `window`). */
  readonly target?: GestureTarget | null;
}

/** The unlock events (44 §4.2 step 1). */
export const UNLOCK_EVENTS = ['pointerdown', 'keydown', 'touchend'] as const;

/** The ramp lengths in seconds: options (20 ms) and the visibility factor (50 ms). */
export const OPTION_RAMP_S = 0.02;
export const HIDDEN_RAMP_S = 0.05;

/** The telemetry counters of 44 §14.3 (one object shared by the engine, the buffer cache and the SFX player). */
export interface EngineCounters {
  errors: Record<string, number>;
  unlockGestures: number;
  droppedHidden: number;
  plays: number;
  droppedVolume0: number;
  droppedLocked: number;
  culled100: number;
  muted70: number;
  droppedSlotFull: number;
  droppedVoiceCap: number;
  stolen: number;
  droppedLate: number;
  droppedMissing: number;
  decodes: number;
  decodeFallbacks: number;
  decodeFailures: number;
  evictions: number;
  /** Gauge: bytes of the decoded buffers in the cache. */
  decodedBytes: number;
  /** Gauge: voices that are playing right now. */
  activeVoices: number;
  landHeightDeferred: number;
}

function freshCounters(): EngineCounters {
  return {
    errors: {},
    unlockGestures: 0,
    droppedHidden: 0,
    plays: 0,
    droppedVolume0: 0,
    droppedLocked: 0,
    culled100: 0,
    muted70: 0,
    droppedSlotFull: 0,
    droppedVoiceCap: 0,
    stolen: 0,
    droppedLate: 0,
    droppedMissing: 0,
    decodes: 0,
    decodeFallbacks: 0,
    decodeFailures: 0,
    evictions: 0,
    decodedBytes: 0,
    activeVoices: 0,
    landHeightDeferred: 0,
  };
}

export interface AudioEngine {
  readonly state: AudioState;
  /** The context; `null` for `unavailable`. */
  readonly context: AudioContextLike | null;
  readonly masterBus: GainNodeLike | null;
  readonly sfxBus: GainNodeLike | null;
  readonly bgmBus: GainNodeLike | null;
  readonly options: AudioOptions;
  readonly hidden: boolean;
  /** The telemetry counters (44 §14.3); the SFX player and the buffer cache increment them. */
  readonly counters: EngineCounters;
  /** `true` for the current hidden policy: a new SFX is dropped while hidden (unless `sfxInBackground`). */
  acceptSfx(): boolean;
  /** Visibility policy of 44 §4.3 (the engine calls it from its own `deps.visibility` subscription). */
  setHidden(hidden: boolean): void;
  /** Applies a patch of options with 20 ms ramps. */
  setOptions(patch: Partial<AudioOptions>): void;
  onState(fn: (state: AudioState) => void): Unsubscribe;
  /** Called synchronously inside the first user gesture (media-element priming, 44 §4.2 step 2). */
  onGesture(fn: () => void): Unsubscribe;
  /** Called once the context is running after a lock or interruption (the music player starts its pending track). */
  onRunning(fn: () => void): Unsubscribe;
  /** Counts an error under `reason` (and logs it once per reason at debug level). */
  noteError(reason: string, detail?: string): void;
  dispose(): void;
}

type AudioContextCtor = new (options?: { latencyHint?: string }) => AudioContextLike;

/** The page's AudioContext constructor, `null` when Web Audio is missing. */
function pageContextFactory(): AudioContextLike | null {
  const g = globalThis as {
    AudioContext?: AudioContextCtor;
    webkitAudioContext?: AudioContextCtor;
  };
  const Ctor = g.AudioContext ?? g.webkitAudioContext;
  if (typeof Ctor !== 'function') return null;
  return new Ctor({ latencyHint: 'interactive' });
}

function pageTarget(): GestureTarget | null {
  return typeof window === 'undefined' ? null : window;
}

export function createAudioEngine(deps: EngineDeps): AudioEngine {
  const log = deps.log;
  const stateListeners = new Set<(s: AudioState) => void>();
  const gestureListeners = new Set<() => void>();
  const runningListeners = new Set<() => void>();
  const counters: EngineCounters = freshCounters();
  const seenErrors = new Set<string>();
  let state: AudioState = 'unavailable';
  let ctx: AudioContextLike | null = null;
  let nodes: {
    readonly master: GainNodeLike;
    readonly sfx: GainNodeLike;
    readonly bgm: GainNodeLike;
  } | null = null;
  let hidden = false;
  let armed = false;
  let disposed = false;
  let options: AudioOptions = normalizeAudioOptions(readOptions());
  const offs: Unsubscribe[] = [];
  const target = deps.target === undefined ? pageTarget() : deps.target;

  function readOptions(): Readonly<Record<string, unknown>> {
    try {
      return deps.options.get() as Readonly<Record<string, unknown>>;
    } catch {
      return {};
    }
  }

  const noteError = (reason: string, detail?: string): void => {
    counters.errors[reason] = (counters.errors[reason] ?? 0) + 1;
    const key = `${reason}:${detail ?? ''}`;
    if (!seenErrors.has(key)) {
      seenErrors.add(key);
      log.debug('audio', detail === undefined ? reason : `${reason}: ${detail}`);
    }
  };

  const safe = <T>(what: string, fn: () => T): T | undefined => {
    try {
      return fn();
    } catch (e) {
      noteError(what, e instanceof Error ? e.message : String(e));
      return undefined;
    }
  };

  const setState = (next: AudioState): void => {
    if (state === next) return;
    state = next;
    for (const fn of [...stateListeners]) {
      safe('state-listener', () => {
        fn(next);
      });
    }
    if (next === 'running') {
      for (const fn of [...runningListeners]) safe('running-listener', fn);
    }
  };

  /** Ramps `param` to `value`: a linear ramp over `seconds` from the current value (20 ms options) or a time-constant approach. */
  const ramp = (
    param: AudioParamLike,
    value: number,
    seconds: number,
    targetStyle: boolean,
  ): void => {
    const c = ctx;
    if (!c) return;
    safe('ramp', () => {
      const t = c.currentTime;
      param.cancelScheduledValues(t);
      if (targetStyle) {
        // 44 §4.3: `setTargetAtTime`; a time constant of a third of the ramp length settles within it
        param.setTargetAtTime(value, t, seconds / 3);
      } else {
        param.setValueAtTime(param.value, t);
        param.linearRampToValueAtTime(value, t + seconds);
      }
    });
  };

  const hiddenFactorSfx = (): number => (hidden && !options.sfxInBackground ? 0 : 1);
  const hiddenFactorBgm = (): number => (hidden && !options.bgmInBackground ? 0 : 1);

  const applyGains = (immediate: boolean, targetStyle: boolean): void => {
    if (!ctx || !nodes) return;
    const { master, sfx, bgm } = nodes;
    const m = masterGain(options.masterStep);
    const s = sfxGain(options.sfxStep) * hiddenFactorSfx();
    const b = bgmGain(options.bgmStep) * hiddenFactorBgm();
    if (immediate) {
      safe('gain', () => {
        master.gain.value = m;
        sfx.gain.value = s;
        bgm.gain.value = b;
      });
      return;
    }
    const seconds = targetStyle ? HIDDEN_RAMP_S : OPTION_RAMP_S;
    ramp(master.gain, m, seconds, targetStyle);
    ramp(sfx.gain, s, seconds, targetStyle);
    ramp(bgm.gain, b, seconds, targetStyle);
  };

  /** The constant listener orientation (A6), written once: params path, `setOrientation` fallback. */
  const setListenerOnce = (listener: AudioListenerLike): void => {
    // `+ 0` turns the `-0` of the mirrored zero Z into a plain 0
    const [fx, fy, fz] = mirrorPos(0, 0, -1).map((v) => v + 0) as [number, number, number];
    const [ux, uy, uz] = mirrorPos(0, 1, 0).map((v) => v + 0) as [number, number, number];
    safe('listener', () => {
      if (
        listener.forwardX &&
        listener.forwardY &&
        listener.forwardZ &&
        listener.upX &&
        listener.upY &&
        listener.upZ
      ) {
        listener.forwardX.value = fx;
        listener.forwardY.value = fy;
        listener.forwardZ.value = fz;
        listener.upX.value = ux;
        listener.upY.value = uy;
        listener.upZ.value = uz;
      } else if (listener.setOrientation) {
        listener.setOrientation(fx, fy, fz, ux, uy, uz);
      }
    });
  };

  // ---- unlock (44 §4.2) -----------------------------------------------------------------------------------------------

  const onGesture = (): void => {
    counters.unlockGestures += 1;
    const c = ctx;
    if (!c || disposed) return;
    // synchronously inside the gesture: resume, the 1-frame silent buffer (iOS warm-up) and the media-element priming
    safe('resume', () => {
      void c.resume().then(
        () => {
          settle();
        },
        (e: unknown) => {
          noteError('resume', e instanceof Error ? e.message : String(e));
        },
      );
    });
    safe('warmup', () => {
      const buffer = c.createBuffer(1, 1, 22050);
      const source = c.createBufferSource();
      source.buffer = buffer;
      source.connect(c.destination);
      source.start(0);
    });
    for (const fn of [...gestureListeners]) safe('gesture-listener', fn);
    settle();
  };

  const armUnlock = (): void => {
    if (armed || !target || disposed) return;
    armed = true;
    for (const e of UNLOCK_EVENTS)
      target.addEventListener(e, onGesture, { capture: true, passive: true });
  };

  const disarmUnlock = (): void => {
    if (!armed || !target) return;
    armed = false;
    for (const e of UNLOCK_EVENTS) target.removeEventListener(e, onGesture, { capture: true });
  };

  /** Reads the context state: `running` removes the listeners; `interrupted`/`suspended` re-arms them. */
  const settle = (): void => {
    const c = ctx;
    if (!c || disposed || state === 'closed') return;
    const cs = c.state;
    if (cs === 'running') {
      disarmUnlock();
      setState('running');
    } else if (cs === 'closed') {
      disarmUnlock();
      setState('closed');
    } else if (cs === 'interrupted') {
      setState('interrupted');
      armUnlock();
    } else if (state === 'running') {
      // `suspended` after it ran: not caused by us (hidden tabs only change gains)
      setState('interrupted');
      armUnlock();
    } else {
      // still suspended before the first unlock
      if (state !== 'interrupted') setState('locked');
      armUnlock();
    }
  };

  // ---- construction ---------------------------------------------------------------------------------------------------

  const factory = deps.createContext ?? pageContextFactory;
  try {
    ctx = factory();
  } catch (e) {
    ctx = null;
    log.warn('audio', `no AudioContext: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (ctx) {
    const c = ctx;
    nodes = { master: c.createGain(), sfx: c.createGain(), bgm: c.createGain() };
    const g = nodes;
    safe('graph', () => {
      g.sfx.connect(g.master);
      g.bgm.connect(g.master);
      g.master.connect(c.destination);
    });
    applyGains(true, false);
    setListenerOnce(c.listener);
    const onStateChange = (): void => {
      settle();
    };
    safe('statechange', () => {
      c.addEventListener('statechange', onStateChange);
    });
    offs.push(() => {
      safe('statechange', () => {
        c.removeEventListener('statechange', onStateChange);
      });
    });
    state = 'locked';
    settle();
  } else {
    log.warn('audio', 'Web Audio is not available: the client stays silent');
  }

  offs.push(
    deps.visibility.on((e) => {
      if (e === 'hidden') engine.setHidden(true);
      else if (e === 'visible') engine.setHidden(false);
    }),
  );
  offs.push(
    deps.options.subscribe((raw) => {
      engine.setOptions(
        normalizeAudioOptions(raw as Readonly<Record<string, unknown>>, DEFAULT_AUDIO_OPTIONS),
      );
    }),
  );

  const engine: AudioEngine = {
    get state() {
      return state;
    },
    get context() {
      return ctx;
    },
    get masterBus() {
      return nodes?.master ?? null;
    },
    get sfxBus() {
      return nodes?.sfx ?? null;
    },
    get bgmBus() {
      return nodes?.bgm ?? null;
    },
    get options() {
      return options;
    },
    get hidden() {
      return hidden;
    },
    counters,
    acceptSfx() {
      if (state === 'unavailable' || state === 'closed') return false;
      if (!sfxPlayable(options.sfxStep)) return false;
      if (hidden && !options.sfxInBackground) {
        counters.droppedHidden += 1;
        return false;
      }
      return true;
    },
    setHidden(h) {
      if (disposed || hidden === h) return;
      hidden = h;
      applyGains(false, true);
      if (!h && ctx && state !== 'unavailable' && state !== 'closed') {
        // visible again: a suspended context is resumed, else the unlock is re-armed
        const c = ctx;
        if (c.state !== 'running') {
          safe('resume', () => {
            void c.resume().then(settle, () => {
              armUnlock();
            });
          });
          settle();
        }
      }
    },
    setOptions(patch) {
      if (disposed) return;
      const next = normalizeAudioOptions({ ...options, ...patch }, options);
      if (sameAudioOptions(next, options)) return;
      options = next;
      applyGains(false, false);
    },
    onState(fn) {
      stateListeners.add(fn);
      return () => {
        stateListeners.delete(fn);
      };
    },
    onGesture(fn) {
      gestureListeners.add(fn);
      return () => {
        gestureListeners.delete(fn);
      };
    },
    onRunning(fn) {
      runningListeners.add(fn);
      return () => {
        runningListeners.delete(fn);
      };
    },
    noteError,
    dispose() {
      if (disposed) return;
      disarmUnlock();
      disposed = true;
      for (const off of offs.splice(0)) safe('dispose', off);
      const c = ctx;
      if (c && state !== 'unavailable') {
        safe('close', () => {
          void c.close().catch(() => undefined);
        });
        state = 'closed';
        for (const fn of [...stateListeners]) {
          safe('state-listener', () => {
            fn('closed');
          });
        }
      }
      stateListeners.clear();
      gestureListeners.clear();
      runningListeners.clear();
    },
  };
  return engine;
}
