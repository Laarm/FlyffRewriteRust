// The map runtime: the canvases, the engine and the controller of ONE map, kept alive for two minutes after the Map tab is left (Plan/25 section 3.8:
// "GPU textures are released 2 min after the map is hidden"). Coming back within that time shows the map where it was, with its tiles; after it the
// runtime is disposed and the GPU memory is free. The subscription itself closes the moment the tab is left (`session.detach`), not after two minutes.
import { rest } from '../api/rest';
import { loadMeta, loadWorlds } from '../components/mapMeta';
import { writeMapRoute } from './route';
import { useActing, actingFor } from '../stores/acting';
import { adminSocket } from '../stores/live';
import { useSession } from '../stores/session';
import { toast } from '../stores/toasts';
import { safeStorage } from '../../platform/storage';
import { terminateDecodePool } from './decodePool';
import { MapEngine, type MapStats } from './engine';
import { GlLayer } from './gl';
import { loadGameAssets } from './gameAssets';
import { debugHooks, type MapDebug } from './debug';
import { createMapStore } from './state';
import { MapSession } from './session';
import type { FindDoc } from './metaTypes';
import { browserIo } from './tileTextures';

/** The GPU memory is released this long after the map was left. */
export const RELEASE_MS = 2 * 60 * 1000;

declare global {
  interface Window {
    __adminMapStats?: MapStats;
    __adminMapDebug?: MapDebug;
  }
}

export class MapRuntime {
  readonly root: HTMLDivElement;
  readonly engine: MapEngine;
  readonly session: MapSession;
  #observer: ResizeObserver | null = null;
  #host: HTMLElement | null = null;
  #removeThemeWatch: (() => void) | null = null;
  #disposed = false;

  constructor() {
    this.root = document.createElement('div');
    this.root.className = 'map-stage';
    const glCanvas = document.createElement('canvas');
    glCanvas.className = 'map-canvas map-canvas--gl';
    glCanvas.setAttribute('aria-label', 'Live map');
    const overlay = document.createElement('canvas');
    overlay.className = 'map-canvas map-canvas--overlay';
    overlay.setAttribute('aria-hidden', 'true');
    const labels = document.createElement('div');
    labels.className = 'map-labels';
    this.root.append(glCanvas, overlay, labels);
    const gl = new GlLayer(glCanvas);
    const ui = createMapStore();
    this.engine = new MapEngine({ gl, overlay, labelHost: labels, io: browserIo(), ui });
    this.session = new MapSession({
      engine: this.engine,
      socket: () => adminSocket(),
      loadWorlds,
      loadMeta,
      find: (q) => rest.get<FindDoc>('/admin/api/world/find', { query: { q } }),
      loadAssets: (meta) => loadGameAssets(meta),
      storage: safeStorage.local,
      toast,
      writeRoute: writeMapRoute,
      actingCharacter: () => {
        const a = actingFor(
          useSession.getState().info,
          useActing.getState().chosen,
          useActing.getState().candidates,
        );
        return a?.id ?? 0;
      },
    });
  }

  /** Puts the map into `host` and starts it. */
  attach(host: HTMLElement): void {
    if (this.#disposed) return;
    this.#host = host;
    host.appendChild(this.root);
    const measure = (): void => {
      const r = this.root.getBoundingClientRect();
      this.engine.resize(r.width, r.height, Math.min(window.devicePixelRatio || 1, 2));
    };
    measure();
    this.#observer = new ResizeObserver(measure);
    this.#observer.observe(this.root);
    // the theme (light, dark, system) recolours the void and the vector overlays
    const mo = new MutationObserver(() => {
      this.engine.themeChanged();
    });
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onScheme = (): void => {
      this.engine.themeChanged();
    };
    mq.addEventListener('change', onScheme);
    this.#removeThemeWatch = () => {
      mo.disconnect();
      mq.removeEventListener('change', onScheme);
    };
    window.__adminMapStats = this.engine.stats;
    window.__adminMapDebug = debugHooks(this.engine, this.session);
    this.session.attach();
  }

  /** The map left the screen: streams close, the canvases leave the DOM, the textures stay for two minutes. */
  detach(): void {
    this.session.detach();
    this.#observer?.disconnect();
    this.#observer = null;
    this.#removeThemeWatch?.();
    this.#removeThemeWatch = null;
    this.root.remove();
    this.#host = null;
    if (window.__adminMapStats === this.engine.stats) {
      window.__adminMapStats = {
        ...this.engine.stats,
        instances: 0,
        drawn: 0,
        labels: 0,
        subscribed: false,
      };
    }
  }

  get attached(): boolean {
    return this.#host !== null;
  }

  /** Frees the GPU memory and the workers for good. */
  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    this.detach();
    this.session.dispose();
    this.engine.dispose();
    terminateDecodePool();
    delete window.__adminMapDebug;
  }
}

/** What the parking needs of a runtime (tests park fakes: the real one needs a WebGL context). */
export interface Parkable {
  detach(): void;
  dispose(): void;
}

let parked: Parkable | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;

/** The runtime for a map that is about to show: the parked one when it is still alive, else a new one made by `make`. */
export function acquireRuntime<T extends Parkable = MapRuntime>(
  make: () => T = () => new MapRuntime() as unknown as T,
): T {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  if (parked) {
    const rt = parked as T;
    parked = null;
    return rt;
  }
  return make();
}

/** The map left the screen: park it; its textures are released after two minutes. */
export function releaseRuntime(rt: Parkable, ms = RELEASE_MS): void {
  rt.detach();
  parked = rt;
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = null;
    if (parked === rt) parked = null;
    rt.dispose();
  }, ms);
}

/** Tests: drops the parked runtime without waiting. */
export function disposeParked(): void {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  parked?.dispose();
  parked = null;
}
