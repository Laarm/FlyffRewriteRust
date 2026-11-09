// Public API of the audio module (Plan/44 §2, WP-AUDIO-01..09): `createAudio(deps)` is the init step of boot B5 (a context created
// SUSPENDED, the unlock listeners armed, the visibility and options subscriptions), `createNullAudio()` the no-op facade.
//
//   engine      the AudioContext, buses, unlock, visibility policy (§4)                    music     the BGM MusicPlayer (§11.2)
//   cache       the decoded-sound LRU with the pinned core set (§6)                       sfx       the PLAYSND port (§7)
//   regions     the region loop (§11.4)                                                   director  the BGM state machine (§11.3)
//   cues        the typed trigger functions other modules call (§10.1)                    preload   the prefetch policy (§6.5)
//   bindings    app states (§11.7), protocol observers (§10.2), sim events (§8)
//
// Everything is optional in `AudioDeps` beyond the engine inputs: a piece whose dependency is missing stays silent (tests of one
// piece build only what they need). Nothing here throws into a caller (§2.3).

import type { Logger } from '../platform/log';
import type { Unsubscribe } from '../platform/visibility';
import type { ScreenState } from '../stores/screen';
import { createAppBinding, type AppBinding } from './bindings/app';
import { bindSimEvents as bindSim, type SimEventSource } from './bindings/sim';
import { type ObserverRegistry, registerNetObservers } from './bindings/net';
import { type AudioCues, createCues, NULL_CUES } from './cues';
import { createAudioProbe, type AudioProbe } from './debug';
import {
  type AudioData,
  type AudioFeatures,
  type AudioStateReaders,
  NO_FEATURES,
  type Vec3,
} from './deps';
import { BgmDirector } from './director';
import { type AudioEngine, createAudioEngine, type EngineDeps } from './engine';
import {
  AudioBufferCache,
  type BufferPriority,
  DECODED_BUDGET_BYTES,
  DECODED_BUDGET_BYTES_LOW,
} from './loader';
import { type MediaElementLike, MusicPlayer } from './music';
import type { AudioOptions } from './options';
import { Preloader } from './preload';
import { RegionTracker } from './regions';
import { MAX_VOICES, MAX_VOICES_COARSE, SfxPlayer } from './sfx';
import { SoundTable } from './tables';

export type { AudioEngine, AudioState } from './engine';
export { createAudioEngine } from './engine';
export type { AudioContextLike, EngineDeps, GestureTarget } from './engine';
export { MusicPlayer } from './music';
export type { MediaElementLike, MusicSnapshot } from './music';
export {
  bgmGain,
  DEFAULT_AUDIO_OPTIONS,
  masterGain,
  normalizeAudioOptions,
  optionsFromNeuzIni,
  sfxGain,
  sfxHundredthsDb,
  sfxPlayable,
} from './options';
export type { AudioOptions } from './options';
export { BGM_ALIASES, MusicTable, SoundTable, soundFilePath } from './tables';
export type { MusicEntry, SoundEntry } from './tables';
export { AUDIO_TEXT_OGG_UNSUPPORTED, createAppBinding } from './bindings/app';
export { createAudioProbe } from './debug';
export type { AudioProbe } from './debug';
export { AudioBufferCache } from './loader';
export { parsePcmWav } from './wav';
export { relativePosition } from './positional';
export { SfxPlayer } from './sfx';
export type { PlayRecord, VoiceClass } from './sfx';
export { RegionTracker } from './regions';
export { BgmDirector } from './director';
export { createCues, IMPACT_SOUND, NULL_CUES } from './cues';
export type { AudioCues, ProjectileImpact } from './cues';
export { Preloader } from './preload';
export type { AudioData, AudioFeatures, AudioMover, AudioStateReaders, Vec3 } from './deps';
export { withAudioFx } from './fxAudio';

/** The dependencies of `createAudio` (44 §2.2 `AudioDeps`). Only the engine inputs are required. */
export interface AudioDeps {
  readonly log: Logger;
  readonly visibility: EngineDeps['visibility'];
  readonly options: EngineDeps['options'];
  /** `assetUrl(path)` of 24 §6.5; `''` when the file is not client-visible. */
  readonly assetUrl: (path: string) => string;
  /** The `music` bundle, `undefined` until loaded. */
  readonly musicBundle: () => Readonly<Record<string, unknown>> | null | undefined;
  /** The `sounds` bundle (`SoundsBundle.sounds`), `undefined` until loaded. */
  readonly soundsBundle?: () => Readonly<Record<string, unknown>> | null | undefined;
  /** One client-only system chat line (`deps.notice`). */
  readonly notice: (text: string) => void;
  /** Screen-state changes of doc 40 (`machine.subscribe`); the app binding turns them into music calls. */
  readonly onScreen?: (fn: (screen: ScreenState) => void) => Unsubscribe;
  /** The current screen (the binding applies it once at creation). */
  readonly screen?: () => ScreenState;
  /** The lookups of the other bundles (items, movers, sfx table, npcs, worlds, motions). */
  readonly data?: () => AudioData | null;
  /** The one-shot byte fetch of a `Sound/` or `Music/` file at the audio priorities (the asset manager's `fetchBytes`). */
  readonly fetchBytes?: (path: string, priority: BufferPriority) => Promise<ArrayBuffer>;
  /** Read-only views of the stores and the entity table. */
  readonly state?: AudioStateReaders;
  /** `SimBridge.landHeight` (cap Q): the `RA_INN` check of the region tracker. */
  readonly landHeight?: (x: number, z: number) => number | undefined;
  /** The client feature switches the triggers consult (`ClientFeatureConfig`). */
  readonly features?: AudioFeatures;
  /** Monotonic ms (default `performance.now`). */
  readonly now?: () => number;
  /** Phone-class device: halves the voice cap and the decoded budget (default: `(pointer: coarse)` / `deviceMemory <= 4`). */
  readonly lowEnd?: boolean;
  /** Test seams. */
  readonly createContext?: EngineDeps['createContext'];
  readonly target?: EngineDeps['target'];
  readonly createElement?: () => MediaElementLike | null;
}

/** The facade held as `ClientContext.audio` (44 §2.2). */
export interface Audio {
  readonly engine: AudioEngine | null;
  readonly music: MusicPlayer | null;
  readonly sfx: SfxPlayer | null;
  readonly director: BgmDirector | null;
  readonly regions: RegionTracker | null;
  readonly cache: AudioBufferCache | null;
  readonly preload: Preloader | null;
  readonly cues: AudioCues;
  /** Boot B5, before `registry.freeze()`: the Pre / Fx observers of §10.2. */
  registerObservers(registry: ObserverRegistry): void;
  /** Boot B5: the sim-event consumer of §8.3; returns the unsubscribe function. */
  bindSimEvents(events: SimEventSource): () => void;
  /** Game loop step 6, once per 60 Hz client step (also in the hidden-tab pump): the director and the prefetch queue. */
  tick(): void;
  /** Game loop step 10, once per rendered frame: the local player position and the camera's raw yaw in degrees. */
  setListener(playerPos: Vec3, cameraRotxDeg: number): void;
  setHidden(hidden: boolean): void;
  setOptions(patch: Partial<AudioOptions>): void;
  /** The debug probe section (`window.__flyffDebug.audio`). */
  readonly probe: AudioProbe | null;
  dispose(): void;
}

/** The no-op facade: every call is safe (tests, `unavailable` fallbacks, before boot step B5). */
export function createNullAudio(): Audio {
  return {
    engine: null,
    music: null,
    sfx: null,
    director: null,
    regions: null,
    cache: null,
    preload: null,
    cues: NULL_CUES,
    registerObservers: () => undefined,
    bindSimEvents: () => () => undefined,
    tick: () => undefined,
    setListener: () => undefined,
    setHidden: () => undefined,
    setOptions: () => undefined,
    probe: null,
    dispose: () => undefined,
  };
}

/** `matchMedia("(pointer: coarse)")` or a device with 4 GB of memory or less (44 §6.4, §7.5). */
function detectLowEnd(): boolean {
  try {
    if (typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches) return true;
    const mem = (globalThis as { navigator?: { deviceMemory?: number } }).navigator?.deviceMemory;
    return typeof mem === 'number' && mem <= 4;
  } catch {
    return false;
  }
}

/** The state readers of a context without a game: no screen change, no local player. */
function nullState(screen: () => ScreenState): AudioStateReaders {
  return {
    screen,
    self: () => ({ objId: 0, level: 1, skillPoints: 0 }),
    world: () => ({ worldId: 0, folder: '' }),
    mover: () => undefined,
    posOf: () => false,
    selfPos: () => false,
  };
}

/** `createAudio(deps)`: boot step B5. Never throws: a failure yields the `unavailable` engine. */
export function createAudio(deps: AudioDeps): Audio {
  let engine: AudioEngine;
  try {
    engine = createAudioEngine({
      log: deps.log,
      visibility: deps.visibility,
      options: deps.options,
      ...(deps.createContext ? { createContext: deps.createContext } : {}),
      ...(deps.target !== undefined ? { target: deps.target } : {}),
    });
  } catch (e) {
    deps.log.warn(
      'audio',
      `the audio engine could not be created: ${e instanceof Error ? e.message : String(e)}`,
    );
    return createNullAudio();
  }
  const now = deps.now ?? ((): number => performance.now());
  const lowEnd = deps.lowEnd ?? detectLowEnd();
  const state = deps.state ?? nullState(deps.screen ?? (() => ({ s: 'Boot' })));
  const data = deps.data ?? ((): AudioData | null => null);
  const features = deps.features ?? NO_FEATURES;

  // -- music and the BGM director -------------------------------------------------------------------------------------
  const music = new MusicPlayer({
    engine,
    log: deps.log,
    assetUrl: deps.assetUrl,
    bundle: deps.musicBundle,
    ...(deps.createElement ? { createElement: deps.createElement } : {}),
  });
  const regions = new RegionTracker({
    regions: () => {
      const folder = state.world().folder;
      return folder === '' ? undefined : data()?.worldFolder(folder)?.regions;
    },
    landHeight: deps.landHeight ?? (() => undefined),
    onDeferred: () => {
      engine.counters.landHeightDeferred += 1;
    },
  });
  const worldSettings = (): { readonly bgm: number; readonly indoor: boolean } | undefined => {
    const folder = state.world().folder;
    return folder === '' ? undefined : data()?.worldFolder(folder)?.settings;
  };
  const director = new BgmDirector({
    music,
    regions,
    worldSettings,
    worldId: () => state.world().worldId,
    battleBgm: () => engine.options.battleBgm,
    selfPos: (out) => state.selfPos(out),
  });

  // -- sound effects ---------------------------------------------------------------------------------------------------
  let soundTable = new SoundTable(null);
  let soundSource: unknown = null;
  const sounds = (): SoundTable => {
    const bundle = deps.soundsBundle?.();
    if (bundle !== soundSource) {
      soundSource = bundle;
      soundTable = new SoundTable(bundle);
    }
    return soundTable;
  };
  const cache = new AudioBufferCache({
    engine,
    fetchBytes:
      deps.fetchBytes ?? ((): Promise<ArrayBuffer> => Promise.reject(new Error('no asset source'))),
    log: deps.log,
    budgetBytes: lowEnd ? DECODED_BUDGET_BYTES_LOW : DECODED_BUDGET_BYTES,
  });
  const sfx = new SfxPlayer({
    engine,
    cache,
    sounds,
    now,
    log: deps.log,
    maxVoices: lowEnd ? MAX_VOICES_COARSE : MAX_VOICES,
  });

  // -- cues and the prefetch policy ----------------------------------------------------------------------------------------
  const cues = createCues({ sfx, music, director, data, state, features });
  const preload = new Preloader({
    cache,
    sounds,
    music: () => music.table,
    data,
    state,
    warm: (path) => (deps.fetchBytes ? deps.fetchBytes(path, 'prefetch') : Promise.resolve()),
    now,
    log: deps.log,
  });

  // -- bindings --------------------------------------------------------------------------------------------------------
  const binding: AppBinding = createAppBinding({
    music,
    notice: deps.notice,
    director,
    preload,
  });
  const offs: Unsubscribe[] = [];
  if (deps.onScreen) {
    // a screen other than InGame / Reconnecting ends the world the pending `world` sounds were requested in (a world change is a
    // Loading screen, a logout or a character selection another one); `ui` sounds are never ended by a screen change
    let lastScreen = deps.screen?.().s;
    offs.push(
      deps.onScreen((screen) => {
        binding.onScreen(screen);
        if (screen.s !== lastScreen && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
          sfx.endWorldScope();
        }
        lastScreen = screen.s;
      }),
    );
  }
  offs.push(
    deps.visibility.on((e) => {
      if (e === 'pagehide') binding.pageHide();
    }),
  );
  // the music element's `ended` event runs block A at once, also in a hidden tab (44 §11.3)
  offs.push(
    music.onEnded(() => {
      if (state.screen().s === 'InGame' && state.self().objId !== 0) director.onMusicEnded();
    }),
  );
  if (deps.screen) binding.onScreen(deps.screen());

  const noteError = (what: string, error: unknown): void => {
    engine.noteError(what, error instanceof Error ? error.message : String(error));
  };

  return {
    engine,
    music,
    sfx,
    director,
    regions,
    cache,
    preload,
    cues,
    registerObservers(registry) {
      try {
        registerNetObservers(
          registry,
          {
            sfx,
            music,
            director,
            cues,
            state,
            data,
            options: () => engine.options,
            preload,
          },
          noteError,
        );
      } catch (error) {
        noteError('register-observers', error);
      }
    },
    bindSimEvents(events) {
      try {
        return bindSim(events, { sfx, cues, state });
      } catch (error) {
        noteError('bind-sim-events', error);
        return () => undefined;
      }
    },
    tick() {
      try {
        preload.tick();
        // the director runs while a world is shown and the local player exists (a loading screen or a reconnect pauses it)
        if (state.screen().s === 'InGame' && state.self().objId !== 0) director.tick();
      } catch (error) {
        noteError('tick', error);
      }
    },
    setListener(pos, rotx) {
      sfx.setListener(pos, rotx);
    },
    setHidden: (h) => {
      engine.setHidden(h);
    },
    setOptions: (patch) => {
      engine.setOptions(patch);
    },
    probe: createAudioProbe(engine, music, { sfx, director, regions, cache, preload }),
    dispose: () => {
      for (const off of offs.splice(0)) off();
      sfx.dispose();
      cache.dispose();
      music.dispose();
      engine.dispose();
    },
  };
}
