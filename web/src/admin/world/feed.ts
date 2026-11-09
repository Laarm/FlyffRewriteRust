// The map's subscription (Plan/25 section 3.7, 3.8, 3.15, 3.16): ONE `MapSubscribe` on the tab's admin socket while the map is shown, with the
// viewport rectangle plus a 25 % margin, the enabled kinds and the world and layer. It follows the camera with a hysteresis (the rectangle is
// only sent again when the viewport nears the edge of what the server sends, or the zoom changes by a good deal, at most every 200 ms), and
// it is CLOSED when the tab is left, the browser tab is hidden, or nobody touched the window for ten minutes.
import type { MapHandlers, MapSpec, MapSubscription } from '../api/ws';
import type { MapDelta, MapKeyframe } from '../../gen/proto/admin';
import { Tier } from '../../gen/proto/admin';
import { frameOf, type FrameData } from './entities';
import type { Rect } from './view';

/** The window may be left alone this long before the live view is paused (a "Resume live view" overlay appears). */
export const IDLE_MS = 10 * 60 * 1000;
/** Updates of the rectangle are at most this often. */
export const MIN_UPDATE_GAP_MS = 200;

export type FeedState =
  /** No subscription (the map is hidden, paused, or nothing was asked yet). */
  | { readonly kind: 'closed' }
  | { readonly kind: 'live' }
  /** The server refused the subscription (`FORBIDDEN`, `LIMIT`, `BAD_SPEC`, `NOT_FOUND`). */
  | { readonly kind: 'refused'; readonly code: number; readonly message: string };

/** What the feed needs of the socket (the real one is `adminSocket()`, tests pass a fake). */
export interface FeedSocket {
  subscribeMap(spec: MapSpec, handlers: MapHandlers): MapSubscription;
}

export interface FeedOptions {
  readonly socket: () => FeedSocket;
  readonly onFrame: (frame: FrameData, receivedAt: number) => void;
  readonly onState?: (state: FeedState) => void;
  /** A keyframe or delta arrived after a request: the round trip. */
  readonly onLatency?: (ms: number) => void;
  /** The server asked for a fresh start (`Resync`). */
  readonly onResync?: () => void;
  readonly now?: () => number;
  readonly setTimer?: (fn: () => void, ms: number) => unknown;
  readonly clearTimer?: (id: unknown) => void;
}

function sameRect(a: Rect, b: Rect): boolean {
  return a[0] === b[0] && a[1] === b[1] && a[2] === b[2] && a[3] === b[3];
}

/**
 * Whether the rectangle that was sent no longer serves the one wanted: the viewport (the inner part of `want`) leaves the sent rectangle,
 * the centre drifted by more than a tenth of its width, or the size changed by more than 15 %. A pan inside the margin costs no message.
 */
export function rectDrifted(sent: Rect, want: Rect): boolean {
  const sw = sent[2] - sent[0];
  const sh = sent[3] - sent[1];
  const ww = want[2] - want[0];
  const wh = want[3] - want[1];
  if (sw <= 0 || sh <= 0) return true;
  const ratio = ww / sw;
  if (ratio > 1.15 || ratio < 1 / 1.15) return true;
  const dx = Math.abs((want[0] + want[2]) / 2 - (sent[0] + sent[2]) / 2);
  const dz = Math.abs((want[1] + want[3]) / 2 - (sent[1] + sent[3]) / 2);
  return dx > 0.1 * sw || dz > 0.1 * sh || wh / sh > 1.15 || wh / sh < 1 / 1.15;
}

/** `true` when the sent rectangle no longer covers the viewport (the inner rectangle of the wanted one): an update must go out at once. */
export function viewportUncovered(sent: Rect, viewport: Rect): boolean {
  return (
    viewport[0] < sent[0] || viewport[1] < sent[1] || viewport[2] > sent[2] || viewport[3] > sent[3]
  );
}

export class MapFeed {
  readonly #o: FeedOptions;
  #sub: MapSubscription | null = null;
  #sent: MapSpec | null = null;
  #want: MapSpec | null = null;
  #viewport: Rect | null = null;
  #lastSendAt = 0;
  #timer: unknown = null;
  #requestedAt = 0;
  #state: FeedState = { kind: 'closed' };

  constructor(options: FeedOptions) {
    this.#o = options;
  }

  get state(): FeedState {
    return this.#state;
  }

  /** `true` while a subscription is registered. */
  get open(): boolean {
    return this.#sub !== null;
  }

  /** The specification the server last got. */
  get sent(): MapSpec | null {
    return this.#sent;
  }

  /**
   * States what the map wants to see. Opens the subscription on the first call after `close()`; later calls send a `MapUpdate` when the
   * world, layer, kinds or tier changed, when the viewport left the sent rectangle, or when the rectangle drifted (throttled).
   */
  want(spec: MapSpec, viewport: Rect): void {
    this.#want = spec;
    this.#viewport = viewport;
    if (!this.#sub) {
      this.#open(spec);
      return;
    }
    this.#sync(false);
  }

  /** Closes the subscription (the server stops producing at once). */
  close(): void {
    this.#clearTimer();
    const s = this.#sub;
    this.#sub = null;
    this.#sent = null;
    this.#want = null;
    if (s) s.close();
    this.#set({ kind: 'closed' });
  }

  #now(): number {
    return this.#o.now?.() ?? Date.now();
  }

  #set(state: FeedState): void {
    this.#state = state;
    this.#o.onState?.(state);
  }

  #open(spec: MapSpec): void {
    this.#sent = spec;
    this.#lastSendAt = this.#now();
    this.#requestedAt = this.#lastSendAt;
    this.#sub = this.#o.socket().subscribeMap(spec, {
      onKeyframe: (k: MapKeyframe) => {
        this.#arrived(frameOf(true, k));
      },
      onDelta: (d: MapDelta) => {
        this.#arrived(frameOf(false, d));
      },
      onError: (e) => {
        this.#set({ kind: 'refused', code: e.code, message: e.message });
      },
      onResync: () => {
        this.#o.onResync?.();
      },
    });
    this.#set({ kind: 'live' });
  }

  #arrived(frame: FrameData): void {
    const now = this.#now();
    if (this.#requestedAt > 0) {
      this.#o.onLatency?.(Math.max(0, now - this.#requestedAt));
      this.#requestedAt = 0;
    }
    if (this.#state.kind !== 'live') this.#set({ kind: 'live' });
    this.#o.onFrame(frame, now);
  }

  #sync(fromTimer: boolean): void {
    const sub = this.#sub;
    const want = this.#want;
    const sent = this.#sent;
    if (!sub || !want || !sent) return;
    const kindsChanged =
      want.world !== sent.world ||
      want.layer !== sent.layer ||
      want.kinds !== sent.kinds ||
      want.maxTier !== sent.maxTier;
    const uncovered = this.#viewport !== null && viewportUncovered(sent.rect, this.#viewport);
    const drifted = !sameRect(sent.rect, want.rect) && rectDrifted(sent.rect, want.rect);
    if (!kindsChanged && !uncovered && !drifted) {
      this.#clearTimer();
      return;
    }
    const urgent = kindsChanged || uncovered;
    const wait = this.#lastSendAt + MIN_UPDATE_GAP_MS - this.#now();
    if (!urgent && wait > 0 && !fromTimer) {
      // a drift only: the trailing update goes out when the gap is over
      this.#arm(wait);
      return;
    }
    this.#clearTimer();
    this.#send(sub, want);
  }

  #send(sub: MapSubscription, want: MapSpec): void {
    this.#sent = want;
    this.#lastSendAt = this.#now();
    this.#requestedAt = this.#lastSendAt;
    sub.update({
      world: want.world,
      layer: want.layer,
      rect: want.rect,
      kinds: want.kinds,
      maxTier: want.maxTier,
    });
  }

  #arm(ms: number): void {
    if (this.#timer !== null) return;
    const set = this.#o.setTimer ?? ((fn, t) => setTimeout(fn, t));
    this.#timer = set(() => {
      this.#timer = null;
      this.#sync(true);
    }, ms);
  }

  #clearTimer(): void {
    if (this.#timer === null) return;
    (
      this.#o.clearTimer ??
      ((id) => {
        clearTimeout(id as ReturnType<typeof setTimeout>);
      })
    )(this.#timer);
    this.#timer = null;
  }
}

/** The finest tier the console asks for: the server picks the tier from the rectangle and never goes finer than this. */
export const FINEST_TIER: Tier = Tier.A;
