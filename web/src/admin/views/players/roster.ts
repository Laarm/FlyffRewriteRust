// The online roster (Plan/25 section 4.1): the `RosterSubscribe` stream of the tab's one admin socket, kept in a store that every
// view of the players category shares (the Online table, the drawer's "online" state and the object id that "Heal" and "Show on map"
// need). The stream is reference counted: the first user subscribes, the last one unsubscribes, and a new socket (`useLive` epoch)
// subscribes again with a full table.
import { useEffect } from 'react';
import { create } from 'zustand';
import type { RosterDelta, RosterEntry } from '../../../gen/proto/admin';
import { n64 } from '../../../net/int64';
import type { Subscription } from '../../api/ws';
import { adminSocket, useLive } from '../../stores/live';

/** One avatar in the world. */
export interface RosterRow {
  readonly characterId: number;
  readonly objId: number;
  readonly name: string;
  readonly level: number;
  readonly job: number;
  readonly world: number;
  readonly layer: number;
  readonly x: number;
  readonly z: number;
  readonly guildId: number;
  /** The authority letter (`F` player ... `P`). */
  readonly authority: string;
}

export interface RosterState {
  readonly rows: ReadonlyMap<number, RosterRow>;
  /** Avatars in the world / connected sockets that are not. */
  readonly inWorld: number;
  readonly lobby: number;
  /** `idle`: nobody uses it; `loading`: subscribed, no table yet; `live`; `error`. */
  readonly phase: 'idle' | 'loading' | 'live' | 'error';
  readonly error: string | null;
}

const INITIAL: RosterState = {
  rows: new Map(),
  inWorld: 0,
  lobby: 0,
  phase: 'idle',
  error: null,
};

export const useRoster = create<RosterState>(() => INITIAL);

/** The letter of the authority byte of the wire (`76` = `L`). */
export function letterOf(code: number): string {
  return code === 0 ? 'F' : String.fromCharCode(code);
}

export function rowOf(e: RosterEntry): RosterRow {
  return {
    characterId: n64(e.characterId),
    objId: e.objId,
    name: e.name,
    level: e.level,
    job: e.job,
    world: e.world,
    layer: e.layer,
    x: e.x,
    z: e.z,
    guildId: e.guildId,
    authority: letterOf(e.authority),
  };
}

/** Applies one `RosterDelta` to the table (`full` replaces it). */
export function applyDelta(
  rows: ReadonlyMap<number, RosterRow>,
  d: Pick<RosterDelta, 'full' | 'upserts' | 'removes'>,
): Map<number, RosterRow> {
  const next = d.full ? new Map<number, RosterRow>() : new Map(rows);
  for (const e of d.upserts) {
    const row = rowOf(e);
    next.set(row.characterId, row);
  }
  for (const id of d.removes) next.delete(n64(id));
  return next;
}

let users = 0;
let sub: Subscription | null = null;

function subscribe(): void {
  sub?.close();
  useRoster.setState({ phase: 'loading', error: null });
  sub = adminSocket().subscribeRoster(
    (d) => {
      useRoster.setState((s) => ({
        rows: applyDelta(s.rows, d),
        inWorld: d.inWorld,
        lobby: d.lobby,
        phase: 'live',
        error: null,
      }));
    },
    {
      onError: (e) => {
        useRoster.setState({ phase: 'error', error: e.message });
      },
    },
  );
}

function release(): void {
  users = Math.max(0, users - 1);
  if (users > 0) return;
  sub?.close();
  sub = null;
  useRoster.setState(INITIAL);
}

/** Keeps the roster subscription open while the calling view is mounted (`enabled`: the session may see players). */
export function useRosterFeed(enabled = true): void {
  const epoch = useLive((s) => s.epoch);
  useEffect(() => {
    if (!enabled) return;
    users += 1;
    if (sub === null) subscribe();
    return () => {
      // the socket of this epoch is gone or going: closing a subscription of a dead socket is harmless
      release();
    };
  }, [enabled, epoch]);
}

/** Tests: forgets the subscription state. */
export function resetRosterForTests(): void {
  users = 0;
  sub = null;
  useRoster.setState(INITIAL);
}
