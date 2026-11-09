// Session actions (Plan/40 section 4.4, WP-CLIENT-05): what the logout/quit windows, the character-select
// screen and the probe call. Each action asks the state machine first (SM2: a lobby request in flight is
// rejected locally) and sends through the one `NetSender`. The windows and countdowns are 45's.

import { create } from '@bufbuild/protobuf';
import {
  EnterWorldSchema,
  ReturnToCharSelectSchema,
  ReqLeaveSchema,
} from '../../gen/proto/flyff/v1/lobby_pb';
import type { StateMachine, TransitionStatus } from '../../app/stateMachine';
import type { NetClient } from '../../net/client';
import type { NetSender, SendResult } from '../../net/sender';

/** The slice of `ClientContext` the actions use (a `ClientContext` satisfies it). */
export interface SessionActionContext {
  readonly machine: StateMachine;
  readonly net: Pick<NetClient, 'sender'>;
}

/** Writes `EnterWorld` (the machine's `sendEnterWorld` effect; also used by the R4 auto-enter). */
export function sendEnterWorld(sender: NetSender, slot: number, idPlayer: number): SendResult {
  // legacy-send: SendJoin
  const result = sender.send({
    case: 'enterWorld',
    value: create(EnterWorldSchema, { slot, idPlayer }),
  });
  sender.flush(); // lobby actions flush at once (section 5.2 S4)
  return result;
}

/**
 * The user pressed Start. `true` when the request was accepted (state `Entering`, `EnterWorld` queued);
 * `false` when it was refused locally (a lobby request is in flight, SM2, or the slot is not offered).
 */
export function enterWorld(ctx: SessionActionContext, slot: number, idPlayer: number): boolean {
  const t = ctx.machine.dispatch({ type: 'start', slot, idPlayer });
  return t.status === 'ok';
}

/** `ReqLeave`: starts the 10 s logout timer on the server (the countdown window is 45's). */
export function requestLeave(ctx: SessionActionContext): SendResult {
  // legacy-send: SendReqLeave
  const result = ctx.net.sender.send({ case: 'reqLeave', value: create(ReqLeaveSchema) });
  ctx.net.sender.flush();
  return result;
}

/**
 * `ReturnToCharSelect`: leave the world to character select. The state becomes `Leaving` only when the
 * message was queued (not while disconnected); `CharList` completes the transition.
 */
export function returnToCharSelect(ctx: SessionActionContext): SendResult {
  const result = ctx.net.sender.send({
    case: 'returnToCharSelect',
    value: create(ReturnToCharSelectSchema),
  });
  if (result === 'queued') {
    ctx.net.sender.flush();
    ctx.machine.dispatch({ type: 'leave' });
  }
  return result;
}

/**
 * Quit: close the WebSocket with 1000, wait for the close, drop the token and show the login window (a
 * browser cannot close its own tab). Resolves when the socket is closed.
 */
export async function quit(ctx: SessionActionContext): Promise<void> {
  ctx.machine.dispatch({ type: 'back' });
  await ctx.machine.idle();
}

/**
 * Quit from a session in any connected state (45 section 7.13: the logout countdown ended, a dialog logs out): close the
 * socket with 1000, wait for the close, drop the token and show the login window.
 */
export async function quitSession(ctx: SessionActionContext): Promise<void> {
  ctx.machine.dispatch({ type: 'quit' });
  await ctx.machine.idle();
}

/** Back from character select: the same as `quit` (12 section 18.2). */
export const back = quit;

/** Marks a delete (CharSelect) or create (CharCreate) request as sent; `false` while one is in flight (SM2). */
export function beginLobbyRequest(ctx: SessionActionContext, kind: 'create' | 'delete'): boolean {
  return ctx.machine.dispatch({ type: 'lobbyRequest', kind }).status === 'ok';
}

export type { TransitionStatus };
