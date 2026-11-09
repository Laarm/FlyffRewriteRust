// Screen state machine (Plan/40 section 4): a pure reducer `next(machine, event) -> { machine, effects }`
// (unit-testable, one row of section 4.2 per case) and an impure wrapper (`createStateMachine`) that applies
// the new screen to `sessionStore` (rule SM1: the state machine is the ONLY writer of `sessionStore.screen`),
// keeps the transition history for the debug probe and runs the effects through the `SessionHost` port.
//
// Rules implemented here: SM2 (lobby requests are single-flight: `busy` rejects a second request), SM3 (the
// first `SpawnSelf` must carry the selected `idPlayer`; otherwise one L2 resync, then L4), SM4 lives in the
// handler registry (`inJoin`), and the R4 auto-enter (section 4.3: once per page load, only for a remembered,
// non-blocked slot).

import { TID_DIAG_0023, TID_DIAG_0043 } from '../gen/defs/text';
import { HelloAck_Result, type Kick_Reason } from '../gen/proto/flyff/v1/session_pb';
import { sendEnterWorld } from '../game/session/actions';
import type { EntityTable } from '../game/entities';
import type { Logger } from '../platform/log';
import { now as defaultNow } from '../platform/time';
import type { Unsubscribe } from '../platform/visibility';
import { CloseCode, helloAckOutcome, kickOutcome } from '../net/closeCodes';
import type { NetSender } from '../net/sender';
import type { CoreStores } from '../stores/core';
import type { ResettableScope } from '../stores/registry';
import {
  CONNECTED_SCREENS,
  type FatalInfo,
  type LoadingGate,
  type LoadingReason,
  type ScreenState,
  type UiNotice,
} from '../stores/screen';
import type { ConnectionState, OverlayKind } from '../stores/session';

export type { FatalInfo, LoadingGate, LoadingReason, ScreenState, UiNotice };

// ---- machine data -------------------------------------------------------------------------------

/** One occupied slot of the last `CharList` (the state machine keeps the ids for the R4 auto-enter). */
export interface CharSlotLite {
  readonly slot: number;
  readonly idPlayer: number;
  readonly name: string;
  readonly blocked: boolean;
}

/** The `flyff.session.inWorld` marker (12 section 18.5): the character that was in the world. */
export interface InWorldMarker {
  readonly idPlayer: number;
  readonly slot: number;
}

export type AutoEnter = 'armed' | 'used' | 'off';

export interface MachineCtx {
  /** The last `CharList`. */
  readonly chars: readonly CharSlotLite[];
  /** The in-world marker (null: none). */
  readonly remembered: InWorldMarker | null;
  /** R4 auto-enter: armed at boot/reconnect with a marker, `used` after it sent `EnterWorld`, else `off`. */
  readonly autoEnter: AutoEnter;
  /** SM3: the `id_player` the first `SpawnSelf` must carry (null: unknown, accept any). */
  readonly expectedIdPlayer: number | null;
  /** SM3: resyncs requested because of an id mismatch (a second mismatch is fatal). */
  readonly sm3Resyncs: number;
}

export interface Machine {
  readonly screen: ScreenState;
  readonly ctx: MachineCtx;
}

export function initialMachine(): Machine {
  return {
    screen: { s: 'Boot' },
    ctx: { chars: [], remembered: null, autoEnter: 'off', expectedIdPlayer: null, sm3Resyncs: 0 },
  };
}

// ---- events and effects -------------------------------------------------------------------------

export type MachineEvent =
  /** Boot is ready: `hasToken` = a session token exists in `sessionStorage`; `inWorld` = the marker. */
  | {
      readonly type: 'bootReady';
      readonly hasToken: boolean;
      readonly inWorld: InWorldMarker | null;
    }
  /** `POST /api/login` answered 200 (the auth module stored the token). */
  | { readonly type: 'loginOk' }
  | { readonly type: 'loginFailed'; readonly notice: UiNotice }
  | {
      readonly type: 'helloAck';
      readonly result: HelloAck_Result;
      readonly reattached: boolean;
      readonly now: number;
    }
  | {
      readonly type: 'kick';
      readonly reason: Kick_Reason;
      readonly tid: number;
      readonly reconnectAfterMs: number;
    }
  /** The socket closed without a `Kick` (1001, 1006, 1011, network loss, liveness 4901) or we gave up on it. */
  | { readonly type: 'socketLost'; readonly now: number }
  /** The server closed with 4013 (bad token) / 4012 (reload) and no `Kick` frame arrived. */
  | { readonly type: 'socketClosedWith'; readonly code: number }
  | { readonly type: 'reconnectAttempt'; readonly attempt: number }
  | { readonly type: 'reconnectGraceExceeded' }
  | { readonly type: 'charList'; readonly slots: readonly CharSlotLite[] }
  | { readonly type: 'lobbyError'; readonly code: number }
  /** The user opens the create window for an empty slot. */
  | { readonly type: 'openCreate'; readonly slot: number }
  /** The user closes the create window with Cancel: back to the character select (45 §5.3; not while a create is in flight). */
  | { readonly type: 'closeCreate' }
  /** A create (CharCreate) or delete (CharSelect) request was sent: single-flight (SM2). */
  | { readonly type: 'lobbyRequest'; readonly kind: 'create' | 'delete' }
  /** The user presses Start (rules ready is the UI's check, section 3.5). */
  | { readonly type: 'start'; readonly slot: number; readonly idPlayer: number }
  /** Back (or quit) from character select: close the socket, drop the token. */
  | { readonly type: 'back' }
  /** Quit from anywhere with a session (45 §7.13: the countdown ended, a dialog logged out): close the socket, drop the token. */
  | { readonly type: 'quit' }
  | { readonly type: 'worldReadInfo'; readonly worldId: number; readonly now: number }
  | { readonly type: 'replace'; readonly worldId: number; readonly now: number }
  | { readonly type: 'selfSpawned'; readonly idPlayer: number; readonly now: number }
  /** The loading gate's asset conditions are satisfied (section 9.8 step 5, WP-CLIENT-23). */
  | { readonly type: 'gateAssetsReady' }
  /** The gate waited 60 s (section 9.8 step 6). */
  | { readonly type: 'gateTimeout'; readonly now: number }
  /** 75 s without `WorldReadInfo` after `EnterWorld` (section 4.2). */
  | { readonly type: 'enteringTimeout' }
  /** `ReturnToCharSelect` was sent (logout completes). */
  | { readonly type: 'leave' }
  | { readonly type: 'recoveryL3'; readonly reason: string }
  | { readonly type: 'recoveryL4'; readonly error: FatalInfo };

/** Side effects of a transition, executed by the runner through the `SessionHost` port. */
export type Effect =
  | { readonly kind: 'openSocket'; readonly reattach: boolean }
  /** Close with `code` and drop queued sends; a following `openSocket` waits for the close (12 section 18.2). */
  | { readonly kind: 'closeSocket'; readonly code: number }
  | { readonly kind: 'dropToken' }
  | {
      readonly kind: 'sendEnterWorld';
      readonly slot: number;
      readonly idPlayer: number;
      readonly auto: boolean;
    }
  | { readonly kind: 'resetScope'; readonly scope: ResettableScope }
  | { readonly kind: 'setInWorld'; readonly marker: InWorldMarker | null }
  | { readonly kind: 'fetchGatedBundles' }
  | { readonly kind: 'reloadOnce'; readonly reason: string }
  /** L3: `location.reload()` (the recovery ladder wrote its memory). */
  | { readonly kind: 'reloadPage'; readonly reason: string }
  /** L2: close with 4900 and run the reconnect policy (section 12.2). */
  | { readonly kind: 'resync'; readonly reason: string }
  | { readonly kind: 'beginReconnect'; readonly wasInGame: boolean }
  | { readonly kind: 'scheduleReconnect'; readonly afterMs: number }
  | { readonly kind: 'beginJoin' }
  | { readonly kind: 'releaseInput' }
  | { readonly kind: 'flushPublishers' }
  | { readonly kind: 'messageBox'; readonly tid: number }
  | { readonly kind: 'warn'; readonly text: string };

/** Why an event changed nothing. */
export type TransitionStatus =
  /** A row of section 4.2 applied. */
  | 'ok'
  /** No row for this event in this state (late or duplicate events are normal, e.g. after a close). */
  | 'ignored'
  /** SM2: a lobby request is in flight. */
  | 'busy'
  /** The request names a slot or character the last `CharList` does not offer. */
  | 'invalid';

export interface Transition {
  readonly machine: Machine;
  readonly effects: readonly Effect[];
  readonly status: TransitionStatus;
}

// ---- helpers ------------------------------------------------------------------------------------

/** The gate of a fresh loading screen. */
export const newGate = (nowMs: number): LoadingGate => ({
  selfSpawned: false,
  assetsReady: false,
  startedAtMs: nowMs,
});

const gateOpen = (g: LoadingGate): boolean => g.selfSpawned && g.assetsReady;

const ok = (machine: Machine, ...effects: Effect[]): Transition => ({
  machine,
  effects,
  status: 'ok',
});
const ignored = (machine: Machine): Transition => ({ machine, effects: [], status: 'ignored' });
const refused = (machine: Machine, status: 'busy' | 'invalid'): Transition => ({
  machine,
  effects: [],
  status,
});

const withScreen = (m: Machine, screen: ScreenState, ctx: Partial<MachineCtx> = {}): Machine => ({
  screen,
  ctx: { ...m.ctx, ...ctx },
});

/** Entering `Login` resets the `session` scope (section 6.5) and forgets the session. */
const toLogin = (m: Machine, notice: UiNotice | undefined, dropToken: boolean): Transition => {
  const effects: Effect[] = [];
  if (dropToken) effects.push({ kind: 'dropToken' });
  effects.push({ kind: 'setInWorld', marker: null }, { kind: 'resetScope', scope: 'session' });
  return ok(
    withScreen(m, notice ? { s: 'Login', notice } : { s: 'Login' }, {
      chars: [],
      remembered: null,
      autoEnter: 'off',
      expectedIdPlayer: null,
      sm3Resyncs: 0,
    }),
    ...effects,
  );
};

/** `true` for the states in which an avatar is (about to be) in the world: reconnect re-attaches. */
const wasInGame = (s: ScreenState): boolean =>
  s.s === 'InGame' || s.s === 'Loading' || s.s === 'Entering';

function loading(m: Machine, reason: LoadingReason, worldId: number, nowMs: number): Machine {
  return withScreen(m, { s: 'Loading', reason, worldId, gate: newGate(nowMs) });
}

/** The gate opened: `InGame`, the in-world marker is written and the publishers flush (section 9.8 step 7). */
function openGate(m: Machine, idPlayer: number, extra: Effect[] = []): Transition {
  const known = m.ctx.chars.find((c) => c.idPlayer === idPlayer);
  const marker: InWorldMarker = { idPlayer, slot: known?.slot ?? m.ctx.remembered?.slot ?? 0 };
  return ok(
    withScreen(m, { s: 'InGame' }, { remembered: marker, autoEnter: 'off', sm3Resyncs: 0 }),
    ...extra,
    { kind: 'setInWorld', marker },
    { kind: 'flushPublishers' },
  );
}

// ---- the reducer --------------------------------------------------------------------------------

/** The pure transition function: one row of Plan/40 section 4.2 per case. */
export function next(m: Machine, e: MachineEvent): Transition {
  const s = m.screen;

  switch (e.type) {
    case 'bootReady': {
      if (s.s !== 'Boot') return ignored(m);
      if (!e.hasToken) return ok(withScreen(m, { s: 'Login' }));
      const reattach = e.inWorld !== null;
      return ok(
        withScreen(
          m,
          { s: 'Connecting', reattach },
          {
            remembered: e.inWorld,
            autoEnter: reattach ? 'armed' : 'off',
            expectedIdPlayer: e.inWorld?.idPlayer ?? null,
          },
        ),
        { kind: 'openSocket', reattach },
      );
    }

    case 'loginOk':
      if (s.s !== 'Login') return ignored(m);
      return ok(
        withScreen(m, { s: 'Connecting', reattach: false }, { autoEnter: 'off' }),
        { kind: 'fetchGatedBundles' },
        { kind: 'openSocket', reattach: false },
      );

    case 'loginFailed':
      if (s.s !== 'Login') return ignored(m);
      return ok(withScreen(m, { s: 'Login', notice: e.notice }));

    case 'helloAck': {
      if (s.s !== 'Connecting' && s.s !== 'Reconnecting') return ignored(m);
      const failure = helloAckOutcome(e.result);
      if (failure) {
        if (failure.reload) {
          return ok(withScreen(m, { s: 'Reloading', reason: 'proto' }), {
            kind: 'reloadOnce',
            reason: 'proto',
          });
        }
        // a reconnecting client that is refused (grace is the server's) goes to the login screen
        return toLogin(m, failure.notice, failure.dropToken);
      }
      if (e.reattached) {
        // the server re-sends everything from memory: world-scoped reset, then the join batch follows (12 RA2)
        return ok(
          withScreen(
            m,
            { s: 'Loading', reason: 'reattach', worldId: 0, gate: newGate(e.now) },
            {
              autoEnter: 'off',
            },
          ),
          { kind: 'resetScope', scope: 'world' },
          { kind: 'beginJoin' },
        );
      }
      return ok(withScreen(m, { s: 'CharSelect', busy: true }));
    }

    case 'kick': {
      if (!CONNECTED_SCREENS.has(s.s) && s.s !== 'Reconnecting') return ignored(m);
      const outcome = kickOutcome(e.reason, e.tid, e.reconnectAfterMs);
      if (outcome.reload) {
        return ok(withScreen(m, { s: 'Reloading', reason: 'proto' }), {
          kind: 'reloadOnce',
          reason: 'proto',
        });
      }
      if (outcome.reconnect) {
        if (s.s === 'Reconnecting') {
          return ok(
            m,
            ...(e.reconnectAfterMs > 0
              ? [{ kind: 'scheduleReconnect', afterMs: e.reconnectAfterMs } as const]
              : []),
          );
        }
        return ok(
          withScreen(
            m,
            { s: 'Reconnecting', attempt: 0, since: 0, wasInGame: wasInGame(s) },
            reconnectCtx(m, s),
          ),
          { kind: 'releaseInput' },
          { kind: 'beginReconnect', wasInGame: wasInGame(s) },
          ...(e.reconnectAfterMs > 0
            ? [{ kind: 'scheduleReconnect', afterMs: e.reconnectAfterMs } as const]
            : []),
        );
      }
      return toLogin(m, outcome.notice, outcome.dropToken);
    }

    case 'socketLost': {
      if (!CONNECTED_SCREENS.has(s.s)) return ignored(m);
      const inGame = wasInGame(s);
      return ok(
        withScreen(
          m,
          { s: 'Reconnecting', attempt: 0, since: e.now, wasInGame: inGame },
          reconnectCtx(m, s),
        ),
        { kind: 'releaseInput' },
        { kind: 'beginReconnect', wasInGame: inGame },
      );
    }

    case 'socketClosedWith': {
      // 4013 = bad token, 4012 = reload; both normally follow a HelloAck/Kick we already handled
      if (e.code === CloseCode.BadToken && (s.s === 'Connecting' || s.s === 'Reconnecting')) {
        return toLogin(m, undefined, true);
      }
      if (e.code === CloseCode.ReloadRequired && CONNECTED_SCREENS.has(s.s)) {
        return ok(withScreen(m, { s: 'Reloading', reason: 'proto' }), {
          kind: 'reloadOnce',
          reason: 'proto',
        });
      }
      return ignored(m);
    }

    case 'reconnectAttempt':
      if (s.s !== 'Reconnecting') return ignored(m);
      return ok(withScreen(m, { ...s, attempt: e.attempt }));

    case 'reconnectGraceExceeded':
      if (s.s !== 'Reconnecting') return ignored(m);
      return toLogin(m, { tid: TID_DIAG_0023 }, true);

    case 'charList': {
      const chars = e.slots;
      if (s.s === 'Leaving') {
        return ok(
          withScreen(
            m,
            { s: 'CharSelect', busy: false },
            { chars, remembered: null, autoEnter: 'off' },
          ),
          { kind: 'setInWorld', marker: null },
          { kind: 'resetScope', scope: 'world' },
        );
      }
      if (s.s !== 'CharSelect' && s.s !== 'CharCreate' && s.s !== 'Entering') return ignored(m);
      // R4: the first CharList after a (re)connect enters the remembered character, once
      const auto =
        m.ctx.autoEnter === 'armed' && s.s === 'CharSelect' ? autoEnterTarget(m.ctx, chars) : null;
      if (auto) {
        return ok(
          withScreen(
            m,
            { s: 'Entering', slot: auto.slot, idPlayer: auto.idPlayer },
            {
              chars,
              autoEnter: 'used',
              expectedIdPlayer: auto.idPlayer,
            },
          ),
          { kind: 'sendEnterWorld', slot: auto.slot, idPlayer: auto.idPlayer, auto: true },
        );
      }
      const armed = m.ctx.autoEnter === 'armed';
      return ok(
        withScreen(
          m,
          { s: 'CharSelect', busy: false },
          armed ? { chars, autoEnter: 'off', remembered: null } : { chars },
        ),
        ...(armed ? [{ kind: 'setInWorld', marker: null } as const] : []),
      );
    }

    case 'lobbyError': {
      if (s.s === 'CharCreate') {
        // 105 (slot taken) closes the create window; other codes keep it open for another try
        return e.code === 105
          ? ok(withScreen(m, { s: 'CharSelect', busy: false }))
          : ok(withScreen(m, { s: 'CharCreate', slot: s.slot, busy: false }));
      }
      if (s.s === 'CharSelect' || s.s === 'Entering') {
        return ok(withScreen(m, { s: 'CharSelect', busy: false }));
      }
      return ignored(m);
    }

    case 'openCreate':
      if (s.s !== 'CharSelect') return ignored(m);
      if (s.busy) return refused(m, 'busy');
      if (e.slot < 0 || e.slot > 2 || m.ctx.chars.some((c) => c.slot === e.slot)) {
        return refused(m, 'invalid');
      }
      return ok(withScreen(m, { s: 'CharCreate', slot: e.slot, busy: false }));

    case 'closeCreate':
      if (s.s !== 'CharCreate') return ignored(m);
      if (s.busy) return refused(m, 'busy');
      return ok(withScreen(m, { s: 'CharSelect', busy: false }));

    case 'lobbyRequest': {
      if (e.kind === 'create') {
        if (s.s !== 'CharCreate') return ignored(m);
        if (s.busy) return refused(m, 'busy');
        return ok(withScreen(m, { s: 'CharCreate', slot: s.slot, busy: true }));
      }
      if (s.s !== 'CharSelect') return ignored(m);
      if (s.busy) return refused(m, 'busy');
      return ok(withScreen(m, { s: 'CharSelect', busy: true }));
    }

    case 'start': {
      if (s.s !== 'CharSelect') return ignored(m);
      if (s.busy) return refused(m, 'busy');
      const c = m.ctx.chars.find((x) => x.slot === e.slot && x.idPlayer === e.idPlayer);
      if (!c || c.blocked) return refused(m, 'invalid');
      return ok(
        withScreen(
          m,
          { s: 'Entering', slot: e.slot, idPlayer: e.idPlayer },
          {
            autoEnter: 'off',
            expectedIdPlayer: e.idPlayer,
          },
        ),
        { kind: 'sendEnterWorld', slot: e.slot, idPlayer: e.idPlayer, auto: false },
      );
    }

    case 'back': {
      // from character select (also create); the socket is closed with 1000 and the token dropped
      if (s.s !== 'CharSelect' && s.s !== 'CharCreate') return ignored(m);
      const t = toLogin(m, undefined, true);
      return { ...t, effects: [{ kind: 'closeSocket', code: CloseCode.Normal }, ...t.effects] };
    }

    case 'quit': {
      if (!CONNECTED_SCREENS.has(s.s) && s.s !== 'Reconnecting') return ignored(m);
      const t = toLogin(m, undefined, true);
      return { ...t, effects: [{ kind: 'closeSocket', code: CloseCode.Normal }, ...t.effects] };
    }

    case 'worldReadInfo':
      if (s.s === 'Entering') {
        return ok(
          loading(m, 'enter', e.worldId, e.now),
          { kind: 'resetScope', scope: 'map' },
          { kind: 'beginJoin' },
        );
      }
      if (s.s === 'Loading') {
        return ok(withScreen(m, { ...s, worldId: e.worldId }), {
          kind: 'resetScope',
          scope: 'map',
        });
      }
      return ignored(m);

    case 'replace':
      if (s.s === 'InGame' || s.s === 'Loading') {
        return ok(
          loading(m, s.s === 'Loading' ? s.reason : 'worldChange', e.worldId, e.now),
          { kind: 'resetScope', scope: 'map' },
          { kind: 'beginJoin' },
        );
      }
      return ignored(m);

    case 'selfSpawned': {
      if (s.s !== 'Loading') return ignored(m);
      const expected = m.ctx.expectedIdPlayer;
      if (expected !== null && e.idPlayer !== expected) {
        // SM3: one resync, then fatal
        if (m.ctx.sm3Resyncs >= 1) {
          return ok(
            withScreen(m, {
              s: 'Fatal',
              error: {
                reason: `The server placed character ${String(e.idPlayer)} in the world, not ${String(expected)}.`,
              },
            }),
          );
        }
        return ok(withScreen(m, s, { sm3Resyncs: m.ctx.sm3Resyncs + 1 }), {
          kind: 'resync',
          reason: `SpawnSelf id_player ${String(e.idPlayer)} != ${String(expected)}`,
        });
      }
      const gate = { ...s.gate, selfSpawned: true };
      if (gateOpen(gate)) return openGate(m, e.idPlayer);
      return ok(withScreen(m, { ...s, gate }, { expectedIdPlayer: expected ?? e.idPlayer }));
    }

    case 'gateAssetsReady': {
      if (s.s !== 'Loading') return ignored(m);
      const gate = { ...s.gate, assetsReady: true };
      if (gate.selfSpawned && m.ctx.expectedIdPlayer !== null)
        return openGate(m, m.ctx.expectedIdPlayer);
      return ok(withScreen(m, { ...s, gate }));
    }

    case 'gateTimeout': {
      if (s.s !== 'Loading') return ignored(m);
      if (s.gate.selfSpawned && m.ctx.expectedIdPlayer !== null) {
        // open anyway: missing pieces stream in (section 9.8 step 6)
        return openGate(m, m.ctx.expectedIdPlayer, [
          { kind: 'warn', text: 'loading gate timed out after 60 s; entering the world anyway' },
        ]);
      }
      // without SpawnSelf there is no avatar to show: resync once more and wait another round
      return ok(
        withScreen(m, { ...s, gate: newGate(e.now) }),
        { kind: 'warn', text: 'loading gate timed out without SpawnSelf' },
        { kind: 'resync', reason: 'loading gate timeout' },
      );
    }

    case 'enteringTimeout':
      if (s.s !== 'Entering') return ignored(m);
      return ok(
        withScreen(m, { s: 'CharSelect', busy: false }),
        { kind: 'messageBox', tid: TID_DIAG_0043 },
        { kind: 'resync', reason: 'entering timeout' },
      );

    case 'leave':
      if (s.s !== 'InGame') return ignored(m);
      return ok(withScreen(m, { s: 'Leaving' }));

    case 'recoveryL3':
      if (s.s === 'Reloading' || s.s === 'Fatal') return ignored(m);
      return ok(withScreen(m, { s: 'Reloading', reason: e.reason }), {
        kind: 'reloadPage',
        reason: e.reason,
      });

    case 'recoveryL4':
      if (s.s === 'Fatal') return ignored(m);
      return ok(withScreen(m, { s: 'Fatal', error: e.error }));

    default:
      // an event type this build does not know (a module from a newer build): never a transition
      return ignored(m);
  }
}

/** Context changes when a connected state is left through a socket loss (R4 re-arming). */
function reconnectCtx(m: Machine, from: ScreenState): Partial<MachineCtx> {
  const armed = wasInGame(from) && m.ctx.remembered !== null;
  return armed ? { autoEnter: 'armed' } : {};
}

/** R4: the slot of the remembered character in the list, if present and not blocked. */
function autoEnterTarget(ctx: MachineCtx, chars: readonly CharSlotLite[]): CharSlotLite | null {
  if (!ctx.remembered) return null;
  const hit = chars.find((c) => c.idPlayer === ctx.remembered?.idPlayer);
  return hit && !hit.blocked ? hit : null;
}

// ---- derived store fields -----------------------------------------------------------------------

const CONNECTION: Readonly<Record<ScreenState['s'], ConnectionState>> = {
  Boot: 'idle',
  Login: 'idle',
  Connecting: 'connecting',
  CharSelect: 'open',
  CharCreate: 'open',
  Entering: 'open',
  Loading: 'open',
  InGame: 'open',
  Leaving: 'open',
  Reconnecting: 'reconnecting',
  Reloading: 'closed',
  Fatal: 'closed',
};

/** `sessionStore.connection` for a screen. */
export function connectionFor(s: ScreenState): ConnectionState {
  return CONNECTION[s.s];
}

/** `sessionStore.overlay` for a screen (the preparing-assets overlay is controlled by `prepare`). */
export function overlayFor(s: ScreenState, preparing: boolean): OverlayKind {
  if (s.s === 'Connecting' || s.s === 'Entering') return 'connecting';
  if (s.s === 'Reconnecting') return 'reconnecting';
  if (s.s === 'Loading') return 'loading';
  return preparing ? 'preparing' : 'none';
}

// ---- the wrapper with effects -------------------------------------------------------------------

/** Effects the machine cannot perform itself. Missing entries are no-ops (tests, /dev surfaces). */
export interface SessionHost {
  openSocket: (reattach: boolean) => void | Promise<void>;
  /** Closes the socket; the promise resolves when the close completed (a following open waits for it). */
  closeSocket: (code: number) => void | Promise<void>;
  dropToken: () => void;
  setInWorld: (marker: InWorldMarker | null) => void;
  fetchGatedBundles: () => void;
  reloadOnce: (reason: string) => void;
  reloadPage: (reason: string) => void;
  resync: (reason: string) => void;
  beginReconnect: (wasInGame: boolean) => void;
  scheduleReconnect: (afterMs: number) => void;
  beginJoin: () => void;
  releaseInput: () => void;
  flushPublishers: () => void;
  messageBox: (tid: number) => void;
  /** A scope was reset (after the stores and the entity table): `sim.leaveWorld()`, assets generation bump. */
  scopeReset: (scope: ResettableScope) => void;
}

export interface StateMachineDeps {
  readonly stores: CoreStores;
  readonly entities: EntityTable;
  readonly sender: NetSender;
  readonly host?: Partial<SessionHost>;
  readonly log?: Pick<Logger, 'info' | 'warn' | 'error'>;
  readonly now?: () => number;
}

export interface StateMachine {
  /** Applies one event: updates the screen, runs the effects, returns the transition. Never throws. */
  dispatch(event: MachineEvent): Transition;
  readonly screen: ScreenState;
  readonly ctx: MachineCtx;
  /** The last transitions as `{ s, at }` (probe `stateHistory()`), oldest first. */
  history(): readonly { readonly s: string; readonly at: number }[];
  /** Resolves when every asynchronous effect (a socket close) has finished. */
  idle(): Promise<void>;
  /** Calls `fn` after every screen change. */
  subscribe(fn: (screen: ScreenState, previous: ScreenState) => void): Unsubscribe;
}

const HISTORY = 200;

export function createStateMachine(deps: StateMachineDeps): StateMachine {
  const now = deps.now ?? defaultNow;
  const host = deps.host ?? {};
  const listeners = new Set<(screen: ScreenState, previous: ScreenState) => void>();
  const history: { s: string; at: number }[] = [{ s: 'Boot', at: now() }];
  let machine = initialMachine();
  let depth = 0;
  const notifications: [ScreenState, ScreenState][] = [];
  let pending: Promise<void> = Promise.resolve();
  /** Chain of the socket close, so that an `openSocket` effect waits for it (12 section 18.2). */
  let closing: Promise<void> = Promise.resolve();

  const report = (what: string, error: unknown): void => {
    deps.log?.error(
      'state',
      `${what} failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  };
  /** Runs a synchronous host call; a throwing host never breaks the machine. */
  const call = (what: string, fn: () => void): void => {
    try {
      fn();
    } catch (error) {
      report(what, error);
    }
  };
  /** Runs a host call that may be asynchronous; the returned promise never rejects. */
  const callAsync = async (what: string, fn: () => void | Promise<void>): Promise<void> => {
    try {
      await fn();
    } catch (error) {
      report(what, error);
    }
  };

  const run = (effect: Effect): void => {
    switch (effect.kind) {
      case 'openSocket': {
        // waits for a pending close first (12 section 18.2: no spurious duplicate-login on the server)
        const p = closing.then(() =>
          callAsync('openSocket', () => host.openSocket?.(effect.reattach)),
        );
        pending = p;
        return;
      }
      case 'closeSocket': {
        const p = callAsync('closeSocket', () => host.closeSocket?.(effect.code));
        closing = p;
        pending = p;
        return;
      }
      case 'dropToken':
        call('dropToken', () => {
          host.dropToken?.();
        });
        return;
      case 'sendEnterWorld':
        call('sendEnterWorld', () => {
          sendEnterWorld(deps.sender, effect.slot, effect.idPlayer);
        });
        return;
      case 'resetScope':
        call('resetScope', () => {
          deps.stores.registry.resetScope(effect.scope);
          deps.entities.clearAll();
          host.scopeReset?.(effect.scope);
        });
        return;
      case 'setInWorld':
        call('setInWorld', () => {
          host.setInWorld?.(effect.marker);
        });
        return;
      case 'fetchGatedBundles':
        call('fetchGatedBundles', () => {
          host.fetchGatedBundles?.();
        });
        return;
      case 'reloadOnce':
        call('reloadOnce', () => {
          host.reloadOnce?.(effect.reason);
        });
        return;
      case 'reloadPage':
        call('reloadPage', () => {
          host.reloadPage?.(effect.reason);
        });
        return;
      case 'resync':
        deps.log?.warn('state', `resync requested: ${effect.reason}`);
        call('resync', () => {
          host.resync?.(effect.reason);
        });
        return;
      case 'beginReconnect':
        call('beginReconnect', () => {
          host.beginReconnect?.(effect.wasInGame);
        });
        return;
      case 'scheduleReconnect':
        call('scheduleReconnect', () => {
          host.scheduleReconnect?.(effect.afterMs);
        });
        return;
      case 'beginJoin':
        call('beginJoin', () => {
          host.beginJoin?.();
        });
        return;
      case 'releaseInput':
        call('releaseInput', () => {
          host.releaseInput?.();
        });
        return;
      case 'flushPublishers':
        call('flushPublishers', () => {
          host.flushPublishers?.();
        });
        return;
      case 'messageBox':
        call('messageBox', () => {
          host.messageBox?.(effect.tid);
        });
        return;
      case 'warn':
        deps.log?.warn('state', effect.text);
        return;
    }
  };

  const publish = (screen: ScreenState): void => {
    const session = deps.stores.sessionStore;
    const preparing = session.getState().prepare?.active === true;
    session.setState({
      screen,
      connection: connectionFor(screen),
      overlay: overlayFor(screen, preparing),
    });
  };

  // the preparing-assets overlay follows `prepare.active`, which changes without a screen change
  deps.stores.sessionStore.subscribe((s, prev) => {
    const active = s.prepare?.active === true;
    if (active === (prev.prepare?.active === true)) return;
    const overlay = overlayFor(machine.screen, active);
    if (overlay !== s.overlay) deps.stores.sessionStore.setState({ overlay });
  });

  return {
    dispatch(event) {
      const before = machine.screen;
      depth += 1;
      let t: Transition;
      try {
        try {
          t = next(machine, event);
        } catch (error) {
          deps.log?.error(
            'state',
            `reducer failed for ${event.type}: ${error instanceof Error ? error.message : String(error)}`,
          );
          return { machine, effects: [], status: 'ignored' };
        }
        machine = t.machine;
        const after = machine.screen;
        if (after !== before) {
          // the history lists the path through the states; a flip of `busy` or a new notice is not a new state
          if (after.s !== before.s) {
            history.push({ s: after.s, at: now() });
            if (history.length > HISTORY) history.shift();
          }
          deps.log?.info('state', `${before.s} -> ${after.s} (${event.type})`);
          publish(after);
          notifications.push([after, before]);
        } else if (t.status !== 'ok') {
          deps.log?.info('state', `${event.type} ${t.status} in ${before.s}`);
        }
        for (const effect of t.effects) run(effect);
      } finally {
        depth -= 1;
      }
      // an effect may dispatch again (a resync closes the socket and reports it lost): the listeners hear about every
      // change in the order the changes happened, after the outermost dispatch finished, never with a stale screen
      if (depth === 0) {
        while (notifications.length > 0) {
          const [after, previous] = notifications.shift() ?? [];
          if (!after || !previous) break;
          for (const fn of [...listeners]) {
            try {
              fn(after, previous);
            } catch (error) {
              deps.log?.error(
                'state',
                `listener failed: ${error instanceof Error ? error.message : String(error)}`,
              );
            }
          }
        }
      }
      return t;
    },
    get screen() {
      return machine.screen;
    },
    get ctx() {
      return machine.ctx;
    },
    history: () => history.slice(),
    async idle() {
      await pending;
      await closing;
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
  };
}

// `HelloAck_Result` is re-exported for the connection module's convenience.
export { HelloAck_Result };
