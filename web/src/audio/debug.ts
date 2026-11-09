// `window.__flyffDebug.audio` (Plan/44 §14.4, `debug.ts`): the read-only probe of the audio layer for Playwright and the diagnostics:
// the engine state, the options, the telemetry counters of §14.3, the ring of the last plays (`lastPlays`), the music state, the
// director state and the missing files. Every property is a getter so a reader always sees the current values; every function
// returns a copy.

import type { AudioEngine, AudioState } from './engine';
import type { BgmDirector } from './director';
import type { AudioBufferCache } from './loader';
import type { MusicPlayer, MusicSnapshot } from './music';
import type { AudioOptions } from './options';
import type { PreloadStats, Preloader } from './preload';
import type { RegionTracker } from './regions';
import type { PlayRecord, SfxPlayer } from './sfx';

export type AudioPlayRecord = PlayRecord;

/** The probe section. */
export interface AudioProbe {
  readonly state: AudioState;
  readonly options: AudioOptions;
  readonly counters: Record<string, number>;
  /** The last 64 plays, oldest first. */
  readonly lastPlays: readonly PlayRecord[];
  readonly music: MusicSnapshot;
  readonly director: {
    idBgm: number;
    directLatch: boolean;
    firstFlying: boolean;
    insideRegions: number[];
  };
  /** Sound files and music tracks that do not exist for this client. */
  readonly missing: readonly string[];
  /** The decoded-buffer cache: resident / pinned buffers, requests in flight. */
  readonly cache: {
    size: number;
    pinned: number;
    inflight: number;
    budgetBytes: number;
    /** The pinned core set and the resident buffers, as paths (the e2e "all core keys present" check). */
    pinnedPaths: string[];
    residentPaths: string[];
  };
  /** The prefetch policy of §6.5. */
  readonly preload: PreloadStats | null;
  /**
   * Dev/test helper: `play(id, pos?, cls?)` through the SFX player (default: class `ui`, non-positional; `pos` is an LH world
   * position and makes the voice positional). The Playwright micro-benchmark and the kq_30 / pl_014 / pl_166 specs use it.
   */
  play(id: number, pos?: readonly [number, number, number], cls?: 'ui' | 'world'): void;
  /** Dev/test helper: `playMusic(id, loops?)` through the music player. */
  playMusic(id: number, loops?: number): void;
}

export interface ProbeParts {
  readonly sfx?: SfxPlayer;
  readonly director?: BgmDirector;
  readonly regions?: RegionTracker;
  readonly cache?: AudioBufferCache;
  readonly preload?: Preloader;
}

/** Builds the probe object over the pieces of the audio layer. */
export function createAudioProbe(
  engine: AudioEngine,
  music: MusicPlayer,
  parts: ProbeParts = {},
): AudioProbe {
  const probe: AudioProbe = {
    get state() {
      return engine.state;
    },
    get options() {
      return { ...engine.options };
    },
    get counters() {
      const c = engine.counters;
      const out: Record<string, number> = {};
      for (const [k, v] of Object.entries(c)) if (typeof v === 'number') out[k] = v;
      for (const [k, v] of Object.entries(c.errors)) out[`error:${k}`] = v;
      return out;
    },
    get lastPlays() {
      return parts.sfx?.lastPlays() ?? [];
    },
    get music() {
      return music.snapshot();
    },
    get director() {
      return {
        idBgm: parts.director?.idBgm ?? 0,
        directLatch: parts.director?.directLatch ?? false,
        firstFlying: parts.director?.firstFlying ?? false,
        insideRegions: parts.regions?.insideRegions ?? [],
      };
    },
    get missing() {
      return [...music.missing, ...(parts.cache?.missing ?? [])];
    },
    get cache() {
      const c = parts.cache;
      return {
        size: c?.size ?? 0,
        pinned: c?.pinned.length ?? 0,
        inflight: c?.inflight ?? 0,
        budgetBytes: c?.budgetBytes ?? 0,
        pinnedPaths: [...(c?.pinned ?? [])],
        residentPaths: [...(c?.residentPaths ?? [])],
      };
    },
    get preload() {
      return parts.preload?.stats ?? null;
    },
    play(id, pos, cls = 'ui') {
      parts.sfx?.playId(id, pos, cls);
    },
    playMusic(id, loops = 1) {
      music.playMusicId(id, loops);
    },
  };
  return probe;
}
