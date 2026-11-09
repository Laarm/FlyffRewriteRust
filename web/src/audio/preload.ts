// The preloading policy (Plan/44 §6.5, WP-AUDIO-09): which sound files are fetched and decoded BEFORE they are first needed, so
// that a first-use sound rarely has to start late (a sound whose buffer arrives late still plays, no window drops it any more,
// PL-166 as changed 2026-10-29). All of it is cosmetic and low priority (`audio-prefetch`, the last class of the
// asset queue): a play request that cannot wait asks for its own file at `audio-demand` (`AudioBufferCache.get`).
//
//   core (pinned)  when the world is entered (`Loading`): the 17 UI / pickup / equip / level-up sounds, and the `sfx-table` sounds of
//                  the level-up, hero-mark and hit effects. Never evicted.
//   self           after `SpawnSelf` and on every own `DoEquipNtf`: the own active-hand item's attack sounds and every `MA_SOUND` id
//                  of the clips of the own model's motion slots (the `motions` bundle).
//   mover kind     the first `SpawnNpc` / `SpawnPlayer` of each mover prop id (de-duplicated per page): its damage and idle sounds,
//                  the virtual attack item's sounds and the `MA_SOUND` ids of its clips, at most 32 files per second (token bucket).
//   BGM            after entering a world: the HTTP cache of the level-up and death jingles is warmed (fetched, not decoded).
//
// The current world BGM streams from its media element on demand; it is never prefetched. A set whose data is not there yet (the
// `sounds` / `sfx-table` bundles of a slow boot) is retried from `tick()` until it can be resolved.

import { OT_MOVER } from '../gen/defs/define';
import { II_WEA_HAN_HAND } from '../gen/defs/item';
import { PARTS_RWEAPON } from '../gen/defs/neuz';
import {
  XI_GEN_LEVEL_UP01,
  XI_HIT_CRITICAL01,
  XI_HIT_HITBLOCK01,
  XI_HIT_MISS01,
  XI_HIT_PARRY01,
  XI_SYS_HEROMARK01,
  XI_SYS_HEROMARK02,
  XI_SYS_HEROMARK03,
  XI_SYS_HEROMARK05,
  XI_SYS_HEROMARK06,
} from '../gen/defs/obj';
import {
  BGM_IN_DEATH,
  BGM_IN_LEVELUP,
  SND_GEN_LEVELUP,
  SND_GEN_WATER,
  SND_INF_CLICK,
  SND_INF_EQUIPACCESSORY,
  SND_INF_EQUIPARMOR,
  SND_INF_EQUIPCLOTH,
  SND_INF_EQUIPWEAPON,
  SND_INF_GROUNDDROP,
  SND_INF_GROUNDPICKUP,
  SND_INF_INVENTORYDROP,
  SND_INF_MESSENGERRING,
  SND_INF_TRADE,
  SND_INF_UPGRADEFAIL,
  SND_INF_UPGRADESUCCESS,
  SND_ITEM_DROPDING,
  SND_ITEM_DROPDING1,
  SND_WALK_LANDSOFT,
} from '../gen/defs/sound';
import type { Logger } from '../platform/log';
import { type AudioData, type AudioStateReaders, isSoundId } from './deps';
import type { AudioBufferCache } from './loader';
import { type MusicTable, type SoundTable, soundFilePath } from './tables';

/** Files per second the mover-kind prefetch may start (44 §14.1). */
export const PREFETCH_RATE_PER_S = 32;

/** The sound ids of the core set (44 §6.5). */
export const CORE_SOUND_IDS: readonly number[] = [
  SND_INF_CLICK,
  SND_INF_TRADE,
  SND_INF_INVENTORYDROP,
  SND_INF_GROUNDDROP,
  SND_INF_GROUNDPICKUP,
  SND_INF_EQUIPWEAPON,
  SND_INF_EQUIPARMOR,
  SND_INF_EQUIPCLOTH,
  SND_INF_EQUIPACCESSORY,
  SND_INF_MESSENGERRING,
  SND_INF_UPGRADEFAIL,
  SND_INF_UPGRADESUCCESS,
  SND_ITEM_DROPDING,
  SND_ITEM_DROPDING1,
  SND_GEN_LEVELUP,
  SND_GEN_WATER,
  SND_WALK_LANDSOFT,
];

/** The effects whose `sfx-table` sound belongs to the core set. */
export const CORE_SFX_IDS: readonly number[] = [
  XI_GEN_LEVEL_UP01,
  XI_SYS_HEROMARK01,
  XI_SYS_HEROMARK02,
  XI_SYS_HEROMARK03,
  XI_SYS_HEROMARK05,
  XI_SYS_HEROMARK06,
  XI_HIT_CRITICAL01,
  XI_HIT_MISS01,
  XI_HIT_HITBLOCK01,
  XI_HIT_PARRY01,
];

/** The jingles whose files the HTTP cache is warmed with after entering a world. */
export const WARM_MUSIC_IDS: readonly number[] = [BGM_IN_LEVELUP, BGM_IN_DEATH];

export interface PreloadDeps {
  readonly cache: Pick<AudioBufferCache, 'pin' | 'prefetch' | 'has' | 'isMissing'>;
  readonly sounds: () => SoundTable;
  readonly music: () => MusicTable;
  readonly data: () => AudioData | null;
  readonly state: Pick<AudioStateReaders, 'self' | 'mover'>;
  /** Fetches a file to warm the HTTP cache (no decode): the asset manager's one-shot fetch at `audio-prefetch`. */
  readonly warm: (path: string) => Promise<unknown>;
  readonly now: () => number;
  readonly log: Logger;
}

export interface PreloadStats {
  readonly corePinned: number;
  readonly coreDone: boolean;
  readonly selfRequests: number;
  readonly moverKinds: number;
  readonly queued: number;
  readonly started: number;
  readonly warmed: number;
}

export class Preloader {
  readonly #deps: PreloadDeps;
  #titlePending = false;
  #titleDone = false;
  #corePending = false;
  #coreDone = false;
  #corePinned = 0;
  #warmPending = false;
  #warmed = 0;
  #selfRequests = 0;
  #started = 0;
  readonly #seenKinds = new Set<number>();
  readonly #queue: string[] = [];
  readonly #queued = new Set<string>();
  #tokens = PREFETCH_RATE_PER_S;
  #lastRefill: number | null = null;
  #ticks = 0;

  constructor(deps: PreloadDeps) {
    this.#deps = deps;
  }

  get stats(): PreloadStats {
    return {
      corePinned: this.#corePinned,
      coreDone: this.#coreDone,
      selfRequests: this.#selfRequests,
      moverKinds: this.#seenKinds.size,
      queued: this.#queue.length,
      started: this.#started,
      warmed: this.#warmed,
    };
  }

  // ---- the sets ----------------------------------------------------------------------------------------------------

  /**
   * The title screens (`Login`, `CharSelect`): pins the button click so the first press of a session does not wait for the
   * network (it would start late). A tiny addition to 44 §6.5, whose core set is pinned at `Loading`.
   */
  onTitle(): void {
    if (this.#titleDone) return;
    this.#titlePending = true;
    this.#tryTitle();
  }

  /** `EnterWorld` was sent (client state `Loading`): pins the core set (once per page; retried while the data is missing). */
  onLoading(): void {
    if (this.#coreDone) return;
    this.#corePending = true;
    this.#tryCore();
  }

  /** After `SpawnSelf` / on every own `DoEquipNtf`: the own weapon sounds and the clips' `MA_SOUND` ids. */
  onSelfSpawned(): void {
    this.#selfSet();
  }
  onOwnEquip(): void {
    this.#selfSet();
  }

  /** The first spawn of a mover prop id: its sounds go through the rate-limited queue. */
  onMoverKind(propId: number): void {
    if (propId <= 0 || this.#seenKinds.has(propId)) return;
    const data = this.#deps.data();
    if (data === null) return; // retried by the next spawn of the kind (it was not marked seen)
    this.#seenKinds.add(propId);
    for (const path of this.#moverPaths(data, propId)) this.#enqueue(path);
    this.#drain();
  }

  /** Entering a world: warms the HTTP cache of the jingles that follow the first level up or death. */
  onWorldEntered(): void {
    if (this.#warmed > 0) return; // once per page: the HTTP cache keeps the files
    this.#warmPending = true;
    this.#tryWarm();
  }

  /** Once per client step: refills the bucket, drains the queue, retries sets whose data was missing. */
  tick(): void {
    this.#ticks += 1;
    this.#drain();
    // 15 retries per second at most; the sets resolve at once when their data is in
    if (this.#ticks % 4 === 0) {
      if (this.#titlePending && !this.#titleDone) this.#tryTitle();
      if (this.#corePending && !this.#coreDone) this.#tryCore();
      if (this.#warmPending) this.#tryWarm();
    }
  }

  // ---- internals ------------------------------------------------------------------------------------------------

  #pathOfSound(id: number | undefined): string | null {
    if (!isSoundId(id)) return null;
    const e = this.#deps.sounds().lookup(id);
    return e?.kind === 'file' ? e.path : null;
  }

  #tryTitle(): void {
    const table = this.#deps.sounds();
    if (table.size === 0) return; // the bundle is not in yet: tick() retries
    this.#titleDone = true;
    this.#titlePending = false;
    const p = this.#pathOfSound(SND_INF_CLICK);
    if (p !== null) void this.#deps.cache.pin([p], 'prefetch');
  }

  #tryCore(): void {
    const data = this.#deps.data();
    const table = this.#deps.sounds();
    if (data === null || table.size === 0) return; // the bundles are not in yet: tick() retries
    const paths = new Set<string>();
    for (const id of CORE_SOUND_IDS) {
      const p = this.#pathOfSound(id);
      if (p !== null) paths.add(p);
    }
    for (const xi of CORE_SFX_IDS) {
      const file = data.sfx(xi)?.sound;
      const p = typeof file === 'string' && file !== '' ? soundFilePath(file) : null;
      if (p !== null) paths.add(p);
    }
    this.#coreDone = true;
    this.#corePending = false;
    this.#corePinned = paths.size;
    void this.#deps.cache.pin(paths, 'prefetch');
  }

  #selfSet(): void {
    const data = this.#deps.data();
    if (data === null) return;
    const me = this.#deps.state.mover(this.#deps.state.self().objId);
    if (me === undefined) return;
    this.#selfRequests += 1;
    // the active-hand item of the own character
    const worn = me.visual?.equip.get(PARTS_RWEAPON)?.itemId;
    const item = data.item(worn !== undefined && worn > 0 ? worn : II_WEA_HAN_HAND);
    for (const id of [item?.snd_attack1, item?.snd_attack2]) {
      const p = this.#pathOfSound(id);
      if (p !== null) this.#deps.cache.prefetch(p);
    }
    // the clips of the own model
    for (const id of data.motionSounds?.(OT_MOVER, me.propId) ?? []) {
      const p = this.#pathOfSound(id);
      if (p !== null) this.#deps.cache.prefetch(p);
    }
  }

  #moverPaths(data: AudioData, propId: number): string[] {
    const out: string[] = [];
    const add = (id: number | undefined): void => {
      const p = this.#pathOfSound(id);
      if (p !== null) out.push(p);
    };
    const prop = data.mover(propId);
    if (prop !== undefined) {
      add(prop.snd_dmg[1]);
      add(prop.snd_idle[0]);
      const atk = data.item(prop.atk1);
      add(atk?.snd_attack1);
      add(atk?.snd_attack2);
    }
    for (const id of data.motionSounds?.(OT_MOVER, propId) ?? []) add(id);
    return out;
  }

  #enqueue(path: string): void {
    const { cache } = this.#deps;
    if (cache.has(path) || cache.isMissing(path) || this.#queued.has(path)) return;
    this.#queued.add(path);
    this.#queue.push(path);
  }

  /** Starts as many queued prefetches as the token bucket (32 files per second) allows. */
  #drain(): void {
    const now = this.#deps.now();
    if (this.#lastRefill !== null) {
      const dt = Math.max(0, now - this.#lastRefill) / 1000;
      this.#tokens = Math.min(PREFETCH_RATE_PER_S, this.#tokens + dt * PREFETCH_RATE_PER_S);
    }
    this.#lastRefill = now;
    while (this.#tokens >= 1) {
      const path = this.#queue.shift();
      if (path === undefined) break;
      this.#queued.delete(path);
      this.#tokens -= 1;
      this.#started += 1;
      this.#deps.cache.prefetch(path);
    }
  }

  #tryWarm(): void {
    const table = this.#deps.music();
    if (table.size === 0) return; // the bundle is not in yet
    this.#warmPending = false;
    for (const id of WARM_MUSIC_IDS) {
      const e = table.lookup(id);
      if (e.kind !== 'file') continue;
      this.#warmed += 1;
      this.#deps.warm(e.path).catch(() => {
        // a failed warm-up is not an error: the jingle is fetched when it plays
      });
    }
  }
}
