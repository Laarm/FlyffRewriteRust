// `BgmDirector` (Plan/44 §11.3, §11.6, WP-AUDIO-04): the BGM block of `CWndWorld::Process` (`src:_Interface/WndWorld.cpp:8226-8260,
// 8775-8890`) ported literally.
//
//   state: idBgm (m_dwIdBgmMusic, 0), directLatch (m_bBGM, false), firstFlying (m_bFirstFlying, false)
//   block A (first in the step):   if idBgm != 0: if the music is stopped: playBgm(idBgm); if !latch: idBgm = 0
//                                  else: idBgm = world.bgm, or BGM_TH_GENERAL (8) when it is 0
//   block B (after the world work): for the region entered this tick: idBgm = its music, latch = its direct_music,
//                                  playMusicId(music, 1) (a no-op for 0 / unknown / the same file playing), !latch -> idBgm = 0
//
// KEEP-QUIRKS: the world BGM is a "play once" track that block A restarts by polling, so after any jingle it starts again FROM
// THE BEGINNING within two ticks; a `direct_music` region replaces the world loop until the next region entry or a world
// change (leaving the region restores nothing, the latch is never cleared on exit); a region whose track is missing and whose
// `direct_music` is set leaves the BGM silent (KQ-30). The music element's `ended` event runs block A at once (needed in a
// hidden tab where the step runs at 1 Hz); it never adds a call that polling would not make.

import {
  BGM_BA_CRISIS,
  BGM_BA_DARKON,
  BGM_BA_FLARIS,
  BGM_BA_SAINTMORNING,
  BGM_EV_LIGHTWING,
  BGM_TH_FLYING,
  BGM_TH_GENERAL,
} from '../gen/defs/sound';
import {
  WI_DUNGEON_BEAR,
  WI_DUNGEON_KRRR,
  WI_DUNGEON_MUSCLE,
  WI_WORLD_GUILDWAR,
} from '../gen/defs/world';
import type { MusicId } from './tables';
import type { RegionTracker } from './regions';

/** The parts of the `MusicPlayer` the director uses (a recording fake in the trace tests). */
export interface DirectorMusic {
  playMusicId(id: MusicId, loops?: number): boolean;
  playBgm(id: MusicId): boolean;
  isStopped(): boolean;
  stop(): void;
}

export interface DirectorDeps {
  readonly music: DirectorMusic;
  readonly regions: RegionTracker;
  /** The `.wld` settings of the current world (`bgm`, `indoor`), `undefined` while they are not known. */
  readonly worldSettings: () => { readonly bgm: number; readonly indoor: boolean } | undefined;
  /** `WI_*` of the current world. */
  readonly worldId: () => number;
  /** Option `battleBgm` (legacy `BattleBGM`). */
  readonly battleBgm: () => boolean;
  /** The local player's LH position into `out`; `false` when there is none yet. */
  readonly selfPos: (out: [number, number, number]) => boolean;
}

/** `CRect rectSaintmorning(7535, 1846, 9283, 3134)` of `PlayCombatMusic`. */
const SAINT_MORNING = { left: 7535, top: 1846, right: 9283, bottom: 3134 } as const;

export class BgmDirector {
  readonly #deps: DirectorDeps;
  readonly #pos: [number, number, number] = [0, 0, 0];
  #idBgm: MusicId = 0;
  #directLatch = false;
  #firstFlying = false;

  constructor(deps: DirectorDeps) {
    this.#deps = deps;
  }

  get idBgm(): MusicId {
    return this.#idBgm;
  }
  get directLatch(): boolean {
    return this.#directLatch;
  }
  get firstFlying(): boolean {
    return this.#firstFlying;
  }

  /** `EnterWorld` accepted (§11.7): idBgm 0, latch false, firstFlying false, the region flags cleared. */
  reset(): void {
    this.#idBgm = 0;
    this.#directLatch = false;
    this.#firstFlying = false;
    this.#deps.regions.reset();
  }

  /** `Replace` (pre-phase; any world change or teleport): idBgm 0, the music stops, the region flags clear; latch and firstFlying stay. */
  onReplace(): void {
    this.#idBgm = 0;
    this.#deps.music.stop();
    this.#deps.regions.reset();
  }

  /** One 60 Hz client step while `InGame` with a local player. */
  tick(): void {
    this.blockA();
    this.blockB();
  }

  /** A. The BGM block (first in the step). */
  blockA(): void {
    const music = this.#deps.music;
    if (this.#idBgm !== 0) {
      if (music.isStopped()) {
        music.playBgm(this.#idBgm);
        if (!this.#directLatch) this.#idBgm = 0;
      }
    } else {
      const bgm = this.#deps.worldSettings()?.bgm ?? 0;
      this.#idBgm = bgm !== 0 ? bgm : BGM_TH_GENERAL;
    }
  }

  /** B. The region block (after the world processing). */
  blockB(): void {
    const p = this.#pos;
    if (!this.#deps.selfPos(p)) return;
    const entered = this.#deps.regions.update(p[0], p[1], p[2]);
    const region = entered[0];
    if (region === undefined) return;
    this.#idBgm = region.music;
    this.#directLatch = region.direct_music;
    this.#deps.music.playMusicId(region.music, 1);
    if (!this.#directLatch) this.#idBgm = 0;
  }

  /** The music element's `ended` event while `InGame`: block A only, never block B. */
  onMusicEnded(): void {
    this.blockA();
  }

  /**
   * `CMover::PlayCombatMusic` (§11.6; `src:_Common/MoverMove.cpp:31-68`): the combat band track of the player's position.
   * Repeated attacks while the same track plays are same-file no-ops (M3).
   */
  combatMusic(): void {
    if (!this.#deps.battleBgm()) return;
    const world = this.#deps.worldId();
    if (world === WI_WORLD_GUILDWAR) return;
    if (world === WI_DUNGEON_MUSCLE || world === WI_DUNGEON_KRRR || world === WI_DUNGEON_BEAR) {
      return;
    }
    if (this.#deps.worldSettings()?.indoor === true) return;
    const p = this.#pos;
    if (!this.#deps.selfPos(p)) return;
    const x = p[0];
    const px = Math.trunc(x);
    const pz = Math.trunc(p[2]);
    const r = SAINT_MORNING;
    const music = this.#deps.music;
    if (px >= r.left && px < r.right && pz >= r.top && pz < r.bottom) {
      music.playMusicId(BGM_BA_SAINTMORNING);
    } else if (x < 4609) music.playMusicId(BGM_BA_DARKON);
    else if (x < 6309) music.playMusicId(BGM_BA_CRISIS);
    else if (x < 8138) music.playMusicId(BGM_BA_FLARIS);
    else music.playMusicId(BGM_BA_SAINTMORNING);
  }

  /** T22: the local player mounts a ride: `BGM_TH_FLYING` the first time of the world session, then `BGM_EV_LIGHTWING`. */
  rideEquipped(): void {
    if (!this.#firstFlying) {
      this.#firstFlying = true;
      this.#deps.music.playMusicId(BGM_TH_FLYING, 1);
    } else {
      this.#deps.music.playMusicId(BGM_EV_LIGHTWING, 1);
    }
  }
}
