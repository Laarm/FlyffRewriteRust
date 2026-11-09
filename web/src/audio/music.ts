// `MusicPlayer` (Plan/44 §11.2, WP-AUDIO-03; port of `Music2.cpp:27-168`): ONE BGM stream through an
// `HTMLAudioElement` routed into the context (`MediaElementAudioSourceNode` -> gain -> bgmBus), never fully decoded.
//
// Rules: M1 `playMusicId` (id 0 / NULL_ID / absent id -> false without stopping; a missing file is a failed open),
// M2 `playBgm`, M3 same file (playing -> no-op; paused or ended -> restart from 0), M4 open (the old stream is closed,
// `filename` is set even when the open fails), M5 loops (0 = forever, else once), M6 start, M7 locked context (the track
// is pending, `isPlaying()` is true, the unlock starts it from 0), M8 `stop` = pause, M9/M10 `isPlaying`/`isStopped`,
// M11 async failure (the path becomes missing and is never requested again), M12 volume (the bgm bus; volume 0 keeps
// playing). Cross-fade (§11.8) is stubbed off: the A/B elements are alternated only by a later work package.

import { normalizeResourcePath } from '../assets/paths';
import type { Logger } from '../platform/log';
import type { AudioEngine, AudioNodeLike } from './engine';
import { type MusicId, MusicTable } from './tables';

/** The parts of an `HTMLAudioElement` the player uses. */
export interface MediaElementLike {
  src: string;
  preload: string;
  loop: boolean;
  readonly paused: boolean;
  readonly ended: boolean;
  currentTime: number;
  readonly error: unknown;
  play(): Promise<void>;
  pause(): void;
  load(): void;
  removeAttribute(name: string): void;
  addEventListener(type: 'ended' | 'error' | 'pause' | 'playing', fn: () => void): void;
  canPlayType?(type: string): string;
}

export interface MusicDeps {
  readonly engine: AudioEngine;
  readonly log: Logger;
  /** `assetUrl(path)`: the URL of a `/res` file; `''` when it is not client-visible. */
  readonly assetUrl: (path: string) => string;
  /** The `music` bundle (`MusicBundle.music`), `undefined` until it is loaded. */
  readonly bundle: () => Readonly<Record<string, unknown>> | null | undefined;
  /** Creates an audio element (default: `new Audio()` of the page). */
  readonly createElement?: () => MediaElementLike | null;
}

export type ElementName = 'A' | 'B';

export interface MusicSnapshot {
  readonly filename: string | null;
  readonly playing: boolean;
  readonly pending: boolean;
  readonly element: ElementName | null;
  readonly currentTime: number;
  /** The current element loops (`PlayMusic( id, 0 )`: the siege war music, T36); `false` without a track. */
  readonly loop: boolean;
}

/** Which codecs the page can play (44 §11.9). */
export interface CodecSupport {
  readonly ogg: boolean;
  readonly mp3: boolean;
  readonly wav: boolean;
}

interface Stream {
  readonly path: string;
  readonly name: ElementName;
  readonly el: MediaElementLike;
}

interface Channel {
  readonly el: MediaElementLike;
  readonly gain: AudioNodeLike | null;
  started: boolean;
}

function extensionOf(path: string): string {
  const dot = path.lastIndexOf('.');
  return dot < 0 ? '' : path.slice(dot + 1).toLowerCase();
}

export class MusicPlayer {
  readonly #deps: MusicDeps;
  readonly #channels = new Map<ElementName, Channel>();
  #table: MusicTable | null = null;
  #tableSource: unknown = null;
  #filename: string | null = null;
  #current: Stream | null = null;
  #pending = false;
  readonly #missing = new Set<string>();
  readonly #endedListeners = new Set<(path: string) => void>();
  #codecs: CodecSupport | null = null;
  #unsupportedSeen = false;

  constructor(deps: MusicDeps) {
    this.#deps = deps;
    // the first user gesture primes the elements (Safari unlocks media elements per element) and starts a pending track
    deps.engine.onGesture(() => {
      this.#prime();
    });
    deps.engine.onRunning(() => {
      this.#startPending();
    });
  }

  // ---- state ----------------------------------------------------------------------------------------------------

  /** `g_filename`: the last path that was asked for (also when its open failed). */
  get filename(): string | null {
    return this.#filename;
  }
  get pending(): boolean {
    return this.#pending;
  }
  get missing(): readonly string[] {
    return [...this.#missing];
  }
  /** The music table of the current `music` bundle (rebuilt when the bundle changes). */
  get table(): MusicTable {
    return this.#musicTable();
  }
  currentPath(): string | null {
    return this.#current?.path ?? null;
  }
  /** `true` after a track was refused because its codec is not playable (the notice of §11.9 is due). */
  get unsupportedCodecSeen(): boolean {
    return this.#unsupportedSeen;
  }

  snapshot(): MusicSnapshot {
    const cur = this.#current;
    return {
      filename: this.#filename,
      playing: this.isPlaying(),
      pending: this.#pending,
      element: cur?.name ?? null,
      currentTime: cur?.el.currentTime ?? 0,
      loop: cur?.el.loop === true,
    };
  }

  onEnded(fn: (path: string) => void): () => void {
    this.#endedListeners.add(fn);
    return () => {
      this.#endedListeners.delete(fn);
    };
  }

  codecs(): CodecSupport {
    if (this.#codecs) return this.#codecs;
    const probe = this.#element('A')?.el;
    const can = (type: string): boolean => {
      try {
        return (probe?.canPlayType?.(type) ?? '') !== '';
      } catch {
        return false;
      }
    };
    this.#codecs = {
      ogg: can('audio/ogg; codecs="vorbis"'),
      mp3: can('audio/mpeg'),
      wav: true,
    };
    return this.#codecs;
  }

  // ---- the rules ------------------------------------------------------------------------------------------------

  /** M1: `PlayMusic(DWORD id, int loops = 1)`. */
  playMusicId(id: MusicId, loops = 1): boolean {
    const table = this.#musicTable();
    const entry = table.lookup(id);
    if (entry.kind === 'absent') return false;
    if (entry.kind === 'missing') {
      // `null` = the file is missing: a failed open (M4), the old stream is closed
      return this.#open(`missing:${String(id)}`, loops, true);
    }
    return this.playMusicPath(entry.path, loops);
  }

  /** M2: `PlayBGM(id)` = `playMusicId(id, 1)`. */
  playBgm(id: MusicId): boolean {
    return this.playMusicId(id, 1);
  }

  /** `PlayMusic(LPCTSTR, int)`: M3-M6. */
  playMusicPath(path: string, loops: number): boolean {
    const normal = normalizeResourcePath(path) ?? path;
    // M3: the same file
    if (normal === this.#filename) {
      if (this.isPlaying()) return false;
      const cur = this.#current;
      if (cur && !this.#missing.has(normal)) {
        // a paused or ended track of the same path restarts from the beginning
        cur.el.loop = loops <= 0;
        cur.el.currentTime = 0;
        this.#begin(cur);
        return true;
      }
    }
    return this.#open(normal, loops, this.#missing.has(normal));
  }

  /** M8: `StopMusic()` = pause; the track stays loaded. */
  stop(): void {
    const cur = this.#current;
    if (!cur) return;
    this.#pending = false;
    try {
      cur.el.pause();
    } catch (e) {
      this.#deps.engine.noteError('music-pause', e instanceof Error ? e.message : String(e));
    }
  }

  /** M9. */
  isPlaying(): boolean {
    const cur = this.#current;
    if (!cur) return false;
    if (this.#pending) return true;
    return !cur.el.paused && !cur.el.ended && !cur.el.error;
  }

  /** M10. */
  isStopped(): boolean {
    return !this.isPlaying();
  }

  /** `pagehide`: pause the stream (the page is going away). */
  pageHide(): void {
    this.stop();
  }

  // ---- internals ------------------------------------------------------------------------------------------------

  #musicTable(): MusicTable {
    const bundle = this.#deps.bundle();
    if (this.#table === null || bundle !== this.#tableSource) {
      this.#tableSource = bundle;
      this.#table = new MusicTable(bundle, () => this.#deps.engine.options.bgmAliasFix);
    }
    return this.#table;
  }

  #element(name: ElementName): Channel | null {
    const existing = this.#channels.get(name);
    if (existing) return existing;
    let el: MediaElementLike | null = null;
    try {
      el = this.#deps.createElement
        ? this.#deps.createElement()
        : typeof Audio === 'function'
          ? new Audio()
          : null;
    } catch {
      el = null;
    }
    if (!el) return null;
    // each element is connected to the graph ONCE (a second createMediaElementSource on it throws)
    let gain: AudioNodeLike | null = null;
    const ctx = this.#deps.engine.context;
    const bus = this.#deps.engine.bgmBus;
    if (ctx && bus && typeof ctx.createMediaElementSource === 'function') {
      try {
        const source = ctx.createMediaElementSource(el);
        const g = ctx.createGain();
        source.connect(g);
        g.connect(bus);
        gain = g;
      } catch (e) {
        this.#deps.engine.noteError('music-graph', e instanceof Error ? e.message : String(e));
      }
    }
    const channel: Channel = { el, gain, started: false };
    el.addEventListener('ended', () => {
      const cur = this.#current;
      if (cur?.el === el) {
        for (const fn of [...this.#endedListeners]) {
          try {
            fn(cur.path);
          } catch (err) {
            this.#deps.engine.noteError(
              'music-ended',
              err instanceof Error ? err.message : String(err),
            );
          }
        }
      }
    });
    el.addEventListener('error', () => {
      // M11: a 404 or a decode error: the path is missing for good, the stream is dropped
      const cur = this.#current;
      if (cur?.el === el) {
        this.#missing.add(cur.path);
        this.#deps.log.debug('audio', `music ${cur.path} could not be played`);
        this.#close(cur);
        this.#current = null;
        this.#pending = false;
      }
    });
    this.#channels.set(name, channel);
    return channel;
  }

  /** M4-M6 for a path that may be missing. */
  #open(path: string, loops: number, knownMissing: boolean): boolean {
    this.#closeCurrent();
    this.#filename = path;
    if (knownMissing || path.startsWith('missing:')) return false;
    const ext = extensionOf(path);
    const codecs = this.codecs();
    const playable =
      ext === 'wav' || (ext === 'ogg' && codecs.ogg) || (ext === 'mp3' && codecs.mp3);
    if (!playable) {
      if (ext === 'ogg') this.#unsupportedSeen = true;
      this.#missing.add(path);
      return false;
    }
    const url = this.#deps.assetUrl(path);
    if (url === '') {
      this.#missing.add(path);
      return false;
    }
    const ch = this.#element('A');
    if (!ch) {
      this.#missing.add(path);
      return false;
    }
    const stream: Stream = { path, name: 'A', el: ch.el };
    this.#current = stream;
    ch.el.loop = loops <= 0; // M5
    ch.el.preload = 'auto';
    ch.el.src = url;
    this.#begin(stream);
    return true;
  }

  /** M6/M7: start now, or mark as pending while the context is locked. */
  #begin(stream: Stream): void {
    const engine = this.#deps.engine;
    if (engine.state === 'locked' || engine.state === 'interrupted') {
      this.#pending = true;
      return;
    }
    this.#pending = false;
    this.#play(stream);
  }

  #play(stream: Stream): void {
    try {
      const p = stream.el.play();
      void p.catch((e: unknown) => {
        // a NotAllowedError keeps the track pending for the next unlock; anything else is a play failure
        const name = e instanceof Error ? e.name : '';
        if (name === 'NotAllowedError') {
          if (this.#current === stream) this.#pending = true;
        } else {
          this.#deps.engine.noteError('music-play', e instanceof Error ? e.message : String(e));
        }
      });
    } catch (e) {
      this.#deps.engine.noteError('music-play', e instanceof Error ? e.message : String(e));
    }
  }

  #startPending(): void {
    const cur = this.#current;
    if (!cur || !this.#pending) return;
    this.#pending = false;
    try {
      cur.el.currentTime = 0;
    } catch {
      // a not yet loaded element has no position
    }
    this.#play(cur);
  }

  /** The first gesture: prime the other element (a silent play/pause) and start the pending track inside the gesture. */
  #prime(): void {
    const cur = this.#current;
    if (cur && this.#pending) {
      // the element that holds a pending track calls play() inside the gesture
      this.#pending = false;
      this.#play(cur);
    }
  }

  #closeCurrent(): void {
    const cur = this.#current;
    if (cur) this.#close(cur);
    this.#current = null;
    this.#pending = false;
  }

  #close(stream: Stream): void {
    try {
      stream.el.pause();
      stream.el.removeAttribute('src');
      stream.el.load();
    } catch (e) {
      this.#deps.engine.noteError('music-close', e instanceof Error ? e.message : String(e));
    }
  }

  dispose(): void {
    this.#closeCurrent();
    this.#endedListeners.clear();
  }
}
