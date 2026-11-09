// `SfxPlayer` (Plan/44 §7, WP-AUDIO-05): the port of `PLAYSND` / `CSoundManager::Play`.
//
//   playId(id, pos?, cls)   id -> file through the `sounds` table (absent / silent / missing -> nothing)
//   playFile(file, pos?)    a literal file relative to `Sound/` (SFX-table sounds, NPC scripts, a few UI sounds)
//
// The guards, in the order of 44 §7.1: engine unavailable / closed; locked or interrupted context (only `ui` sounds start:
// they are the unlocking gesture itself); SFX volume step 0; hidden tab; `id <= 0`; table lookup; the positional math
// (`positional.ts`: > 100 horizontal -> culled, > 70 -> a MUTED voice that still occupies its slot); at most 10 concurrent
// voices per file (the 11th is dropped); the decoded buffer (a first-use sound is fetched and starts when its buffer arrives,
// however late: it is dropped only when its source is gone - `stopAll`, or for a `world` sound `endWorldScope` - and the guards
// of the arrival moment apply, PL-166 as changed 2026-10-29); the global voice cap (48, 24 on a coarse pointer) where a `ui`
// sound steals the oldest `world` voice (PL-167).
//
// Positions are LH world coordinates; the listener is the local player (set once per rendered frame by the game loop,
// `setListener`). The WebAudio listener never moves (A6); the source position handed to the `PannerNode` is the
// listener-relative `mirrorPos` image of the legacy buffer position, frozen at play time.

import type { Logger } from '../platform/log';
import type { AudioBufferLike, AudioEngine, BufferSourceLike, PannerLike } from './engine';
import type { AudioBufferCache } from './loader';
import { type Relative, relativePosition, type Vec3 } from './positional';
import { sfxPlayable } from './options';
import { type SoundId, type SoundTable, soundFilePath } from './tables';

export type VoiceClass = 'ui' | 'world';

/** Global voice caps (44 §7.5, PL-167). */
export const MAX_VOICES = 48;
export const MAX_VOICES_COARSE = 24;
/** DirectSound's ten buffers per sound file. */
export const MAX_VOICES_PER_FILE = 10;
/** Entries of the `lastPlays` ring of the debug probe. */
export const PLAY_RING = 64;

/** One entry of `__flyffDebug.audio.lastPlays`. */
export interface PlayRecord {
  /** `now()` ms. */
  readonly t: number;
  /** The normalised path (`sound/…`), else `id:<n>` / `file:<name>` when it was not resolved. */
  readonly key: string;
  readonly id?: number;
  readonly cls: VoiceClass;
  /** The source position as given (LH), absent for a non-positional sound. */
  readonly pos?: readonly [number, number, number];
  /** The legacy listener-relative buffer position (integers) of a positional voice that started. */
  readonly rel?: readonly [number, number, number];
  /** `played` | `late` | `muted` | `culled` | `dropped:<why>`. */
  readonly result: string;
}

export interface SfxDeps {
  readonly engine: AudioEngine;
  readonly cache: AudioBufferCache;
  /** The current `sounds` table (rebuilt by the owner when the bundle changes). */
  readonly sounds: () => SoundTable;
  /** Monotonic ms (`platform/time`). */
  readonly now: () => number;
  readonly log: Logger;
  /** Default `MAX_VOICES` (`MAX_VOICES_COARSE` on a coarse pointer, decided by the owner). */
  readonly maxVoices?: number;
}

interface Slot {
  /** The voice (or muted placeholder) occupies the file's slot until this time. */
  until: number;
}

interface Voice {
  readonly path: string;
  readonly cls: VoiceClass;
  readonly src: BufferSourceLike;
  readonly panner: PannerLike | null;
  readonly slot: Slot;
  readonly release: () => void;
  ended: boolean;
}

const NON_POSITIONAL: Relative = {
  kind: 'play',
  px: 0,
  py: 0,
  pz: 0,
  panner: [0, 0, 0],
  distance: 0,
};

export class SfxPlayer {
  readonly #deps: SfxDeps;
  readonly #maxVoices: number;
  readonly #voices = new Set<Voice>();
  readonly #slots = new Map<string, Slot[]>();
  readonly #ring: PlayRecord[] = [];
  #ringNext = 0;
  #listener: Vec3 = [0, 0, 0];
  #rotx = 0;
  /** Bumped by `stopAll`: a first-use request made in an earlier epoch has lost its source (44 §6.2). */
  #epoch = 0;
  /** Bumped by `endWorldScope`: the world a pending `world` request was made in is gone. */
  #worldEpoch = 0;

  constructor(deps: SfxDeps) {
    this.#deps = deps;
    this.#maxVoices = deps.maxVoices ?? MAX_VOICES;
  }

  // ---- the listener -----------------------------------------------------------------------------------------------

  /** `CSoundManager::SetListener`: stores the local player position and the camera's raw yaw (degrees); nothing else moves. */
  setListener(pos: Vec3, rotxDeg: number): void {
    this.#listener = [pos[0], pos[1], pos[2]];
    this.#rotx = rotxDeg;
  }
  get listener(): Vec3 {
    return this.#listener;
  }
  get rotx(): number {
    return this.#rotx;
  }

  // ---- state ----------------------------------------------------------------------------------------------------

  get activeVoices(): number {
    return this.#voices.size;
  }
  get maxVoices(): number {
    return this.#maxVoices;
  }
  /** The last plays, oldest first (a copy). */
  lastPlays(): PlayRecord[] {
    if (this.#ring.length < PLAY_RING) return [...this.#ring];
    return [...this.#ring.slice(this.#ringNext), ...this.#ring.slice(0, this.#ringNext)];
  }

  // ---- the two entry points (PLAYSND) ---------------------------------------------------------------------------------

  /** `PLAYSND(int id, D3DXVECTOR3* pos)`. */
  playId(id: SoundId, pos?: Vec3, cls: VoiceClass = 'world'): void {
    try {
      if (!this.#admit(cls, `id:${String(id)}`, id, pos)) return;
      if (!(id > 0)) return; // SND_NONE and below never play (KEEP-QUIRK)
      const entry = this.#deps.sounds().lookup(id);
      if (entry === undefined || entry.kind === 'silent') return;
      if (entry.kind === 'missing') {
        this.#deps.engine.counters.droppedMissing += 1;
        this.#record(`id:${String(id)}`, cls, id, pos, 'dropped:missing');
        return;
      }
      this.#playPath(entry.path, pos, cls, id);
    } catch (error) {
      this.#deps.engine.noteError('sfx', error instanceof Error ? error.message : String(error));
    }
  }

  /** `PLAYSND(const char* file, pos)`: `file` is relative to `Sound/`. */
  playFile(file: string, pos?: Vec3, cls: VoiceClass = 'world'): void {
    try {
      if (!this.#admit(cls, `file:${file}`, undefined, pos)) return;
      const path = soundFilePath(file);
      if (path === null) return;
      this.#playPath(path, pos, cls, undefined);
    } catch (error) {
      this.#deps.engine.noteError('sfx', error instanceof Error ? error.message : String(error));
    }
  }

  /** Stops every voice (dispose, a context loss); pending first-use starts are dropped (their source is gone). */
  stopAll(): void {
    this.#epoch += 1;
    for (const v of [...this.#voices]) this.#stop(v);
  }

  /**
   * The world the pending `world` sounds were requested in is gone (a world change, the logout, the character selection): a
   * first-use `world` sound still waiting for its buffer is dropped when it arrives (`dropped:late`). `ui` sounds are not
   * affected (44 §6.2, PL-166 as changed 2026-10-29).
   */
  endWorldScope(): void {
    this.#worldEpoch += 1;
  }

  // ---- internals ------------------------------------------------------------------------------------------------

  /** The guards that do not depend on the id (44 §7.1 first four lines). */
  #admit(cls: VoiceClass, key: string, id: number | undefined, pos: Vec3 | undefined): boolean {
    const { engine } = this.#deps;
    const state = engine.state;
    if (state === 'unavailable' || state === 'closed') return false;
    if ((state === 'locked' || state === 'interrupted') && cls !== 'ui') {
      // no queued bursts on resume; a `ui` click is the unlocking gesture itself
      engine.counters.droppedLocked += 1;
      this.#record(key, cls, id, pos, 'dropped:locked');
      return false;
    }
    if (!sfxPlayable(engine.options.sfxStep)) {
      engine.counters.droppedVolume0 += 1; // m_nSoundVolume == -5000
      return false;
    }
    if (engine.hidden && !engine.options.sfxInBackground) {
      engine.counters.droppedHidden += 1;
      this.#record(key, cls, id, pos, 'dropped:hidden');
      return false;
    }
    return true;
  }

  #playPath(path: string, pos: Vec3 | undefined, cls: VoiceClass, id: number | undefined): void {
    const { engine, cache } = this.#deps;
    const rel =
      pos === undefined ? NON_POSITIONAL : relativePosition(this.#listener, this.#rotx, pos);
    if (rel.kind === 'skip') {
      engine.counters.culled100 += 1;
      this.#record(path, cls, id, pos, 'culled');
      return;
    }
    const now = this.#deps.now();
    if (this.#occupied(path, now) >= MAX_VOICES_PER_FILE) {
      engine.counters.droppedSlotFull += 1;
      this.#record(path, cls, id, pos, 'dropped:slots');
      return;
    }
    const buffer = cache.peek(path);
    if (buffer === undefined) {
      if (cache.isMissing(path)) {
        engine.counters.droppedMissing += 1;
        this.#record(path, cls, id, pos, 'dropped:missing');
        return;
      }
      // first use: fetch and decode; the voice starts when the buffer arrives, however late (PL-166 as changed 2026-10-29, the
      // original loaded synchronously and always played it). It is dropped only when its source is gone: `stopAll`, or the
      // world scope of a `world` sound ended. The position stays the listener-relative one frozen at the request.
      const epoch = this.#epoch;
      const worldEpoch = this.#worldEpoch;
      void cache.get(path, 'demand').then(
        (b) => {
          if (b === null) {
            engine.counters.droppedMissing += 1;
            this.#record(path, cls, id, pos, 'dropped:missing');
          } else if (
            epoch !== this.#epoch ||
            (cls === 'world' && worldEpoch !== this.#worldEpoch)
          ) {
            // the counter now means "dropped because its source was gone"
            engine.counters.droppedLate += 1;
            this.#record(path, cls, id, pos, 'dropped:late');
          } else if (this.#admit(cls, path, id, pos)) {
            // the guards of the arrival moment (engine closed, locked, volume step 0, hidden tab) count and record themselves
            this.#start(path, b, rel, cls, id, pos, 'late');
          }
        },
        () => undefined,
      );
      return;
    }
    this.#start(path, buffer, rel, cls, id, pos, 'played');
  }

  #start(
    path: string,
    buffer: AudioBufferLike,
    rel: Relative,
    cls: VoiceClass,
    id: number | undefined,
    pos: Vec3 | undefined,
    result: 'played' | 'late',
  ): void {
    const { engine, cache } = this.#deps;
    const ctx = engine.context;
    const bus = engine.sfxBus;
    if (ctx === null || bus === null || rel.kind === 'skip') return;
    const now = this.#deps.now();
    if (this.#occupied(path, now) >= MAX_VOICES_PER_FILE) {
      engine.counters.droppedSlotFull += 1;
      this.#record(path, cls, id, pos, 'dropped:slots');
      return;
    }
    // the slot is taken for the buffer's duration, also by a muted voice (KEEP)
    const seconds = buffer.duration ?? buffer.length / (buffer.sampleRate ?? 44100);
    const slot: Slot = { until: now + seconds * 1000 };
    this.#slotsOf(path).push(slot);
    if (rel.kind === 'muted') {
      engine.counters.muted70 += 1;
      this.#record(path, cls, id, pos, 'muted');
      return;
    }
    if (this.#voices.size >= this.#maxVoices) {
      const victim = cls === 'ui' ? this.#oldestWorldVoice() : null;
      if (victim === null) {
        slot.until = 0;
        engine.counters.droppedVoiceCap += 1;
        this.#record(path, cls, id, pos, 'dropped:voice-cap');
        return;
      }
      engine.counters.stolen += 1;
      this.#stop(victim);
    }
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    let panner: PannerLike | null = null;
    if (pos === undefined) {
      src.connect(bus);
    } else {
      panner = ctx.createPanner();
      panner.panningModel = 'equalpower';
      panner.distanceModel = 'inverse';
      panner.refDistance = 4;
      panner.rolloffFactor = 1;
      panner.maxDistance = 10000;
      panner.coneInnerAngle = 360;
      panner.coneOuterAngle = 360;
      panner.coneOuterGain = 0;
      const [x, y, z] = rel.panner;
      if (panner.positionX && panner.positionY && panner.positionZ) {
        panner.positionX.value = x;
        panner.positionY.value = y;
        panner.positionZ.value = z;
      } else {
        panner.setPosition?.(x, y, z);
      }
      src.connect(panner);
      panner.connect(bus);
    }
    const voice: Voice = {
      path,
      cls,
      src,
      panner,
      slot,
      release: cache.retain(path),
      ended: false,
    };
    src.onended = () => {
      this.#end(voice);
    };
    this.#voices.add(voice);
    try {
      src.start();
    } catch (error) {
      this.#end(voice);
      engine.noteError('sfx-start', error instanceof Error ? error.message : String(error));
      return;
    }
    engine.counters.plays += 1;
    engine.counters.activeVoices = this.#voices.size;
    this.#record(
      path,
      cls,
      id,
      pos,
      result,
      pos === undefined ? undefined : [rel.px, rel.py, rel.pz],
    );
  }

  #oldestWorldVoice(): Voice | null {
    for (const v of this.#voices) if (v.cls === 'world') return v;
    return null;
  }

  /** `src.stop()` for a voice; its `ended` handler releases it (also called synchronously, the release is idempotent). */
  #stop(voice: Voice): void {
    try {
      voice.src.stop();
    } catch {
      // already stopped
    }
    this.#end(voice);
  }

  #end(voice: Voice): void {
    if (voice.ended) return;
    voice.ended = true;
    voice.slot.until = 0;
    voice.release();
    this.#voices.delete(voice);
    this.#deps.engine.counters.activeVoices = this.#voices.size;
    try {
      voice.src.disconnect?.();
      voice.panner?.disconnect?.();
    } catch {
      // a disconnected node
    }
  }

  /** Slots of `path` that are still occupied at `now` (expired ones are dropped). */
  #occupied(path: string, now: number): number {
    const slots = this.#slots.get(path);
    if (slots === undefined) return 0;
    let n = 0;
    for (let i = slots.length - 1; i >= 0; i -= 1) {
      const s = slots[i];
      if (s === undefined) continue;
      if (s.until > now) n += 1;
      else slots.splice(i, 1);
    }
    if (slots.length === 0) this.#slots.delete(path);
    return n;
  }

  #slotsOf(path: string): Slot[] {
    let slots = this.#slots.get(path);
    if (slots === undefined) {
      slots = [];
      this.#slots.set(path, slots);
    }
    return slots;
  }

  #record(
    key: string,
    cls: VoiceClass,
    id: number | undefined,
    pos: Vec3 | undefined,
    result: string,
    rel?: readonly [number, number, number],
  ): void {
    const rec: PlayRecord = {
      t: this.#deps.now(),
      key,
      cls,
      result,
      ...(id !== undefined ? { id } : {}),
      ...(pos !== undefined ? { pos: [pos[0], pos[1], pos[2]] as const } : {}),
      ...(rel !== undefined ? { rel } : {}),
    };
    if (this.#ring.length < PLAY_RING) {
      this.#ring.push(rec);
    } else {
      this.#ring[this.#ringNext] = rec;
      this.#ringNext = (this.#ringNext + 1) % PLAY_RING;
    }
  }

  dispose(): void {
    this.stopAll();
    this.#slots.clear();
  }
}
