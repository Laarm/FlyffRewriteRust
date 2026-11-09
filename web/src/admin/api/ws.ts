// The WebSocket client of the admin console (Plan/25 section 11.2, 11.4; WP-ADMIN-05). One socket per console tab, binary frames, one
// `AdminClientMsg` / `AdminServerMsg` (protocol `flyff.admin.v1`, `gen/proto/admin`) per frame.
// * Subscriptions are registered here, not on the socket: each has a client-chosen `sub_id`, the message that (re)creates it and a
//   handler. After a reconnect the client sends every registered subscription again (same ids) and the server answers with fresh
//   keyframes and snapshots; a `MapUpdate` changes the stored specification, so a replay after a pan opens the map where it is now.
// * Reconnect: exponential back-off 0.5 s to 10 s (with a little jitter), reset by the first message of a connection.
// * Liveness: a `Ping` every 15 s while open (the server closes after 45 s of silence); the client drops a socket that heard nothing
//   for 45 s itself and reconnects.
// * `SessionEnded` / close code 4001 ends the client for good (state `ended`): the session is gone, the shell shows the login.
// * 64-bit fields arrive as `bigint`; convert with `n64` from `gen/proto/js_safe` where a number is needed.
import { create, fromBinary, toBinary, type MessageInitShape } from '@bufbuild/protobuf';
import {
  AdminClientMsgSchema,
  AdminServerMsgSchema,
  SessionEndReason,
  type ChatChannel,
  type ContainerSnapshot,
  type CharacterDetail,
  type ChatLine,
  type EntityDetail,
  type LayerList,
  type LogLine,
  type MapDelta,
  type MapKeyframe,
  type RosterDelta,
  type StatusTick,
  type SubscriptionError,
  type Tier,
} from '../../gen/proto/admin';
import { b64 } from '../../gen/proto/js_safe';

/** The close code of a socket whose session ended (25 section 11.4). */
export const CLOSE_SESSION_ENDED = 4001;
/** The close code of a socket the server closed for silence. */
export const CLOSE_IDLE = 4002;
/** The client pings this often while the socket is open (25 section 11.4). */
export const PING_EVERY_MS = 15_000;
/** A socket that heard nothing for this long is dropped by the client (the server's own limit). */
export const SILENCE_TIMEOUT_MS = 45_000;
/** The first reconnect waits this long; every failure doubles it up to {@link MAX_BACKOFF_MS}. */
export const MIN_BACKOFF_MS = 500;
export const MAX_BACKOFF_MS = 10_000;

/**
 * `idle`: not started; `connecting`: a socket is opening; `open`: connected; `waiting`: back-off before the next attempt;
 * `ended`: the session ended (no reconnect); `closed`: `close()` was called.
 */
export type WsState = 'idle' | 'connecting' | 'open' | 'waiting' | 'ended' | 'closed';

/** The part of the browser `WebSocket` the client uses (tests pass a fake). */
export interface WebSocketLike {
  binaryType: string;
  readonly readyState: number;
  send(data: Uint8Array): void;
  close(code?: number, reason?: string): void;
  onopen: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
  onclose: ((ev: { code: number; reason: string }) => void) | null;
  onerror: ((ev: unknown) => void) | null;
}

export interface AdminWsOptions {
  /** Default: `ws(s)://<location.host>/admin/ws`. */
  readonly url?: string;
  /** Default: `new WebSocket(url)`. */
  readonly socketFactory?: (url: string) => WebSocketLike;
  readonly minBackoffMs?: number;
  readonly maxBackoffMs?: number;
  readonly pingEveryMs?: number;
  readonly silenceTimeoutMs?: number;
  /** `Math.random` by default (the back-off jitter). */
  readonly random?: () => number;
  /** `Date.now` by default (the ping payload and the silence clock). */
  readonly now?: () => number;
}

/** The rectangle, world, layer, kinds and detail ceiling of a map subscription (25 section 3.16 `MapSubscribe`). */
export interface MapSpec {
  readonly world: number;
  readonly layer: number;
  /** `[x0, z0, x1, z1]` in metres. */
  readonly rect: readonly [number, number, number, number];
  /** A bitset of `1 << EntityKind`; see {@link kindsBitset}. */
  readonly kinds: number;
  /** The finest tier the client accepts (the server picks the tier from the width and never goes finer). */
  readonly maxTier: Tier;
}

export interface Subscription {
  readonly id: number;
  /** Ends the subscription (the server stops producing, the registry forgets it). */
  close(): void;
}

export interface MapSubscription extends Subscription {
  /** The current specification (what a reconnect replays). */
  readonly spec: MapSpec;
  /** Changes the rectangle, kinds, tier, world or layer; the next frame is a keyframe. */
  update(patch: Partial<MapSpec>): void;
}

export interface CommonHandlers {
  /** The server refused the subscription (`FORBIDDEN`, `LIMIT`, `BAD_SPEC`, `NOT_FOUND`). */
  readonly onError?: (e: SubscriptionError) => void;
  /** The server dropped a queue: the stream restarts from a full state. */
  readonly onResync?: () => void;
}

export interface MapHandlers extends CommonHandlers {
  readonly onKeyframe?: (k: MapKeyframe) => void;
  readonly onDelta?: (d: MapDelta) => void;
}

export interface CharacterHandlers extends CommonHandlers {
  readonly onDetail?: (d: CharacterDetail) => void;
  readonly onContainer?: (c: ContainerSnapshot) => void;
}

export interface ChatFilter {
  /** Empty = every channel the session may see. */
  readonly channels?: readonly ChatChannel[];
  /** `0` / undefined = all worlds. */
  readonly world?: number;
  /** `0` / undefined = all speakers. */
  readonly characterId?: number;
  readonly text?: string;
}

/** What a server message hands to the handler of its subscription. */
type Delivery =
  | { readonly case: 'mapKeyframe'; readonly value: MapKeyframe }
  | { readonly case: 'mapDelta'; readonly value: MapDelta }
  | { readonly case: 'entityDetail'; readonly value: EntityDetail }
  | { readonly case: 'containerSnapshot'; readonly value: ContainerSnapshot }
  | { readonly case: 'characterDetail'; readonly value: CharacterDetail }
  | { readonly case: 'chatLine'; readonly value: ChatLine }
  | { readonly case: 'rosterDelta'; readonly value: RosterDelta }
  | { readonly case: 'layerList'; readonly value: LayerList }
  | { readonly case: 'logLine'; readonly value: LogLine }
  | { readonly case: 'statusTick'; readonly value: StatusTick };

type ClientInit = MessageInitShape<typeof AdminClientMsgSchema>;

interface Entry {
  readonly id: number;
  /** The message that (re)creates the subscription with its current specification. */
  open(): ClientInit;
  readonly deliver: (d: Delivery) => void;
  readonly handlers: CommonHandlers;
}

const OPEN = 1;

function defaultUrl(): string {
  const loc = globalThis.location;
  const scheme = loc.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${scheme}//${loc.host}/admin/ws`;
}

/** `kinds` for a list of `EntityKind` numbers (1..6). */
export function kindsBitset(kinds: readonly number[]): number {
  let bits = 0;
  for (const k of kinds) bits |= 1 << k;
  return bits >>> 0;
}

/** A short English text for the sign-in screen after the session ended. */
export function endReasonText(reason: SessionEndReason): string {
  switch (reason) {
    case SessionEndReason.LOGOUT:
      return 'You were signed out.';
    case SessionEndReason.BAN:
      return 'This account was banned.';
    case SessionEndReason.AUTHORITY:
      return 'The authority of this account changed. Sign in again.';
    case SessionEndReason.PASSWORD:
      return 'The password of this account changed. Sign in again.';
    case SessionEndReason.SHUTDOWN:
      return 'The server is shutting down.';
    case SessionEndReason.EXPIRED:
      return 'The session expired. Sign in again.';
    case SessionEndReason.END_UNSPECIFIED:
    default:
      return 'The session ended.';
  }
}

function detach(s: WebSocketLike, code?: number, reason?: string): void {
  s.onopen = null;
  s.onmessage = null;
  s.onclose = null;
  s.onerror = null;
  try {
    s.close(code, reason);
  } catch {
    // already closed
  }
}

export class AdminWs {
  private readonly o: {
    readonly url: string | undefined;
    readonly socketFactory: ((url: string) => WebSocketLike) | undefined;
    readonly minBackoffMs: number;
    readonly maxBackoffMs: number;
    readonly pingEveryMs: number;
    readonly silenceTimeoutMs: number;
    readonly random: () => number;
    readonly now: () => number;
  };
  private socket: WebSocketLike | null = null;
  private stateValue: WsState = 'idle';
  private readonly entries = new Map<number, Entry>();
  private nextId = 1;
  private attempt = 0;
  private backoff: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private lastHeard = 0;
  private endReason: SessionEndReason | null = null;
  private readonly stateListeners = new Set<(s: WsState) => void>();
  private readonly endListeners = new Set<(r: SessionEndReason) => void>();
  private readonly pongListeners = new Set<(rttMs: number) => void>();

  constructor(options: AdminWsOptions = {}) {
    this.o = {
      url: options.url,
      socketFactory: options.socketFactory,
      minBackoffMs: options.minBackoffMs ?? MIN_BACKOFF_MS,
      maxBackoffMs: options.maxBackoffMs ?? MAX_BACKOFF_MS,
      pingEveryMs: options.pingEveryMs ?? PING_EVERY_MS,
      silenceTimeoutMs: options.silenceTimeoutMs ?? SILENCE_TIMEOUT_MS,
      random: options.random ?? Math.random,
      now: options.now ?? Date.now,
    };
  }

  get state(): WsState {
    return this.stateValue;
  }

  /** Why the session ended, once `state` is `ended`. */
  get sessionEndReason(): SessionEndReason | null {
    return this.endReason;
  }

  /** The number of registered subscriptions. */
  get subscriptionCount(): number {
    return this.entries.size;
  }

  onState(fn: (s: WsState) => void): () => void {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
  }

  onSessionEnded(fn: (r: SessionEndReason) => void): () => void {
    this.endListeners.add(fn);
    return () => this.endListeners.delete(fn);
  }

  /** The round trip of every answered ping, in milliseconds. */
  onPong(fn: (rttMs: number) => void): () => void {
    this.pongListeners.add(fn);
    return () => this.pongListeners.delete(fn);
  }

  /** Starts connecting (idempotent; a no-op once started, after `ended` and after `close()`). */
  connect(): void {
    if (this.stateValue !== 'idle') return;
    this.openSocket();
  }

  /** Closes for good: no reconnect. The subscriptions are forgotten. */
  close(): void {
    if (this.stateValue === 'closed') return;
    this.clearTimers();
    this.entries.clear();
    const s = this.socket;
    this.socket = null;
    this.setState('closed');
    if (s) detach(s, 1000, 'closed by the client');
  }

  // ---- subscriptions ----------------------------------------------------------------------------------------------

  /** The live map. The server allows one per socket (25 section 3.15) and answers `LIMIT` to the next. */
  subscribeMap(spec: MapSpec, handlers: MapHandlers = {}): MapSubscription {
    let current = spec;
    const sub = this.register(
      (id) => ({
        msg: {
          case: 'mapSubscribe',
          value: {
            subId: id,
            world: current.world,
            layer: current.layer,
            rect: rect(current.rect),
            kinds: current.kinds,
            maxTier: current.maxTier,
          },
        },
      }),
      (d) => {
        if (d.case === 'mapKeyframe') handlers.onKeyframe?.(d.value);
        else if (d.case === 'mapDelta') handlers.onDelta?.(d.value);
      },
      handlers,
    );
    return {
      id: sub.id,
      close: () => {
        sub.close();
      },
      get spec() {
        return current;
      },
      update: (patch) => {
        current = { ...current, ...patch };
        if (!this.entries.has(sub.id)) return;
        this.send({
          msg: {
            case: 'mapUpdate',
            value: {
              subId: sub.id,
              world: current.world,
              layer: current.layer,
              rect: rect(current.rect),
              kinds: current.kinds,
              maxTier: current.maxTier,
            },
          },
        });
      },
    };
  }

  /** `EntityDetail` of one object, on change; the last one has `closed`. */
  watchEntity(
    objId: number,
    onDetail: (d: EntityDetail) => void,
    handlers: CommonHandlers = {},
  ): Subscription {
    return this.register(
      (id) => ({ msg: { case: 'entityWatch', value: { subId: id, objId } } }),
      (d) => {
        if (d.case === 'entityDetail') onDetail(d.value);
      },
      handlers,
    );
  }

  /** `CharacterDetail` every 2 s and the container snapshots of one character. */
  watchCharacter(characterId: number, handlers: CharacterHandlers = {}): Subscription {
    return this.register(
      (id) => ({
        msg: { case: 'characterWatch', value: { subId: id, characterId: b64(characterId) } },
      }),
      (d) => {
        if (d.case === 'characterDetail') handlers.onDetail?.(d.value);
        else if (d.case === 'containerSnapshot') handlers.onContainer?.(d.value);
      },
      handlers,
    );
  }

  subscribeChat(
    filter: ChatFilter,
    onLine: (l: ChatLine) => void,
    handlers: CommonHandlers = {},
  ): Subscription {
    return this.register(
      (id) => ({
        msg: {
          case: 'chatSubscribe',
          value: {
            subId: id,
            channels: [...(filter.channels ?? [])],
            world: filter.world ?? 0,
            characterId: b64(filter.characterId ?? 0),
            text: filter.text ?? '',
          },
        },
      }),
      (d) => {
        if (d.case === 'chatLine') onLine(d.value);
      },
      handlers,
    );
  }

  /** The online roster: a full table first, then deltas (a `Resync` is followed by a full table). */
  subscribeRoster(onDelta: (d: RosterDelta) => void, handlers: CommonHandlers = {}): Subscription {
    return this.register(
      (id) => ({ msg: { case: 'rosterSubscribe', value: { subId: id } } }),
      (d) => {
        if (d.case === 'rosterDelta') onDelta(d.value);
      },
      handlers,
    );
  }

  /** The live layers other than layer 0, of one world or (`null`) of all. */
  subscribeLayers(
    world: number | null,
    onList: (l: LayerList) => void,
    handlers: CommonHandlers = {},
  ): Subscription {
    return this.register(
      (id) => ({ msg: { case: 'layersSubscribe', value: { subId: id, world: world ?? 0 } } }),
      (d) => {
        if (d.case === 'layerList') onList(d.value);
      },
      handlers,
    );
  }

  /** The 1 Hz status of the server. */
  subscribeStatus(onTick: (t: StatusTick) => void, handlers: CommonHandlers = {}): Subscription {
    return this.register(
      (id) => ({ msg: { case: 'statusSubscribe', value: { subId: id } } }),
      (d) => {
        if (d.case === 'statusTick') onTick(d.value);
      },
      handlers,
    );
  }

  /** The server log (`act.server`; 1 error .. 5 trace). */
  subscribeLog(
    minLevel: number,
    targetPrefix: string,
    onLine: (l: LogLine) => void,
    handlers: CommonHandlers = {},
  ): Subscription {
    return this.register(
      (id) => ({
        msg: { case: 'logSubscribe', value: { subId: id, minLevel, targetPrefix } },
      }),
      (d) => {
        if (d.case === 'logLine') onLine(d.value);
      },
      handlers,
    );
  }

  private register(
    open: (id: number) => ClientInit,
    deliver: (d: Delivery) => void,
    handlers: CommonHandlers,
  ): Subscription {
    const id = this.nextId++;
    const entry: Entry = { id, open: () => open(id), deliver, handlers };
    this.entries.set(id, entry);
    // the replay on open sends it; when the socket is already open it goes out now
    if (this.stateValue === 'open') this.send(entry.open());
    return {
      id,
      close: () => {
        this.unsubscribe(id);
      },
    };
  }

  private unsubscribe(id: number): void {
    if (!this.entries.delete(id)) return;
    this.send({ msg: { case: 'unsubscribe', value: { subId: id } } });
  }

  // ---- the socket -------------------------------------------------------------------------------------------------

  private send(init: ClientInit): void {
    const s = this.socket;
    if (s?.readyState !== OPEN || this.stateValue !== 'open') return;
    s.send(toBinary(AdminClientMsgSchema, create(AdminClientMsgSchema, init)));
  }

  private setState(next: WsState): void {
    if (this.stateValue === next) return;
    this.stateValue = next;
    for (const fn of [...this.stateListeners]) fn(next);
  }

  private clearTimers(): void {
    if (this.backoff !== null) clearTimeout(this.backoff);
    this.backoff = null;
    if (this.pingTimer !== null) clearInterval(this.pingTimer);
    this.pingTimer = null;
  }

  private openSocket(): void {
    this.setState('connecting');
    const url = this.o.url ?? defaultUrl();
    const s = this.o.socketFactory
      ? this.o.socketFactory(url)
      : (new WebSocket(url) as unknown as WebSocketLike);
    s.binaryType = 'arraybuffer';
    this.socket = s;
    s.onopen = () => {
      if (this.socket !== s) return;
      this.lastHeard = this.o.now();
      this.setState('open');
      this.startPing();
      for (const e of [...this.entries.values()].sort((a, b) => a.id - b.id)) this.send(e.open());
    };
    s.onmessage = (ev) => {
      if (this.socket !== s) return;
      this.lastHeard = this.o.now();
      this.attempt = 0;
      this.onFrame(ev.data);
    };
    s.onclose = (ev) => {
      if (this.socket !== s) return;
      this.socket = null;
      this.clearTimers();
      if (ev.code === CLOSE_SESSION_ENDED)
        this.sessionEnded(this.endReason ?? SessionEndReason.END_UNSPECIFIED);
      else this.scheduleReconnect();
    };
    s.onerror = () => {
      // the close event follows
    };
  }

  private startPing(): void {
    if (this.pingTimer !== null) clearInterval(this.pingTimer);
    this.pingTimer = setInterval(() => {
      const s = this.socket;
      if (!s) return;
      if (this.o.now() - this.lastHeard >= this.o.silenceTimeoutMs) {
        // a dead connection that never delivers a close event
        this.socket = null;
        this.clearTimers();
        detach(s, 4000, 'no message from the server');
        this.scheduleReconnect();
        return;
      }
      this.send({ msg: { case: 'ping', value: { t: b64(this.o.now()) } } });
    }, this.o.pingEveryMs);
  }

  private scheduleReconnect(): void {
    if (this.stateValue === 'ended' || this.stateValue === 'closed') return;
    this.setState('waiting');
    const base = Math.min(this.o.maxBackoffMs, this.o.minBackoffMs * 2 ** this.attempt);
    this.attempt += 1;
    const delay = Math.round(base * (0.9 + 0.2 * this.o.random()));
    this.backoff = setTimeout(() => {
      this.backoff = null;
      if (this.stateValue === 'waiting') this.openSocket();
    }, delay);
  }

  private sessionEnded(reason: SessionEndReason): void {
    if (this.stateValue === 'ended' || this.stateValue === 'closed') return;
    this.endReason = reason;
    this.clearTimers();
    const s = this.socket;
    this.socket = null;
    this.setState('ended');
    if (s) detach(s, 1000, 'session ended');
    for (const fn of [...this.endListeners]) fn(reason);
  }

  private onFrame(data: unknown): void {
    if (!(data instanceof ArrayBuffer)) return; // the protocol is binary only
    let msg;
    try {
      msg = fromBinary(AdminServerMsgSchema, new Uint8Array(data));
    } catch {
      return; // a frame this build cannot read is skipped, the next one may be fine
    }
    const m = msg.msg;
    switch (m.case) {
      case undefined:
        return;
      case 'mapKeyframe':
      case 'mapDelta':
      case 'entityDetail':
      case 'containerSnapshot':
      case 'characterDetail':
      case 'chatLine':
      case 'rosterDelta':
      case 'layerList':
      case 'logLine':
      case 'statusTick':
        this.entries.get(m.value.subId)?.deliver(m);
        return;
      case 'pong':
        for (const fn of [...this.pongListeners]) fn(Math.max(0, this.o.now() - Number(m.value.t)));
        return;
      case 'sessionEnded':
        this.sessionEnded(m.value.reason);
        return;
      case 'subscriptionError':
        this.entries.get(m.value.subId)?.handlers.onError?.(m.value);
        return;
      case 'resync':
        this.entries.get(m.value.subId)?.handlers.onResync?.();
        return;
    }
  }
}

function rect(r: readonly [number, number, number, number]): {
  x0: number;
  z0: number;
  x1: number;
  z1: number;
} {
  return { x0: r[0], z0: r[1], x1: r[2], z1: r[3] };
}
