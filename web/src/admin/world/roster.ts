// The online roster of the map (Plan/25 section 3.2, 3.16 `RosterDelta`): every online character with its world, layer and position, a full table
// first and deltas every 2 s. The minimap draws its dots from it without a map subscription, and "follow" uses it to see a player change world
// or layer. Needs `view.players`; a session without it gets `FORBIDDEN` and the map simply has no dots.
import type { CommonHandlers, Subscription } from '../api/ws';
import type { RosterDelta, RosterEntry } from '../../gen/proto/admin';

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
  readonly authority: number;
}

export interface RosterSocket {
  subscribeRoster(onDelta: (d: RosterDelta) => void, handlers?: CommonHandlers): Subscription;
}

function rowOf(e: RosterEntry): RosterRow {
  return {
    characterId: Number(e.characterId),
    objId: e.objId,
    name: e.name,
    level: e.level,
    job: e.job,
    world: e.world,
    layer: e.layer,
    x: e.x,
    z: e.z,
    guildId: e.guildId,
    authority: e.authority,
  };
}

export class RosterFeed {
  readonly rows = new Map<number, RosterRow>();
  readonly #byObj = new Map<number, RosterRow>();
  #sub: Subscription | null = null;
  /** Increments on every delta. */
  version = 0;
  /** `false` once the server refused the subscription. */
  allowed = true;
  readonly #listeners = new Set<() => void>();

  /** Calls `fn` after every delta; returns the remover. */
  onChange(fn: () => void): () => void {
    this.#listeners.add(fn);
    return () => this.#listeners.delete(fn);
  }

  get open(): boolean {
    return this.#sub !== null;
  }

  start(socket: RosterSocket): void {
    if (this.#sub) return;
    this.allowed = true;
    this.#sub = socket.subscribeRoster(
      (d) => {
        this.apply(d);
      },
      {
        onError: () => {
          this.allowed = false;
          this.#changed();
        },
      },
    );
  }

  stop(): void {
    const s = this.#sub;
    this.#sub = null;
    if (s) s.close();
    this.rows.clear();
    this.#byObj.clear();
    this.version += 1;
    this.#changed();
  }

  apply(d: RosterDelta): void {
    if (d.full) {
      this.rows.clear();
      this.#byObj.clear();
    }
    for (const e of d.upserts) {
      const row = rowOf(e);
      const old = this.rows.get(row.characterId);
      if (old && old.objId !== row.objId && this.#byObj.get(old.objId) === old)
        this.#byObj.delete(old.objId);
      this.rows.set(row.characterId, row);
      this.#byObj.set(row.objId, row);
    }
    for (const id of d.removes) {
      const key = Number(id);
      const old = this.rows.get(key);
      this.rows.delete(key);
      if (old && this.#byObj.get(old.objId) === old) this.#byObj.delete(old.objId);
    }
    this.version += 1;
    this.#changed();
  }

  /** The online players of a world (any layer). */
  inWorld(world: number): RosterRow[] {
    const out: RosterRow[] = [];
    for (const r of this.rows.values()) if (r.world === world) out.push(r);
    return out;
  }

  byObj(objId: number): RosterRow | undefined {
    return this.#byObj.get(objId);
  }

  #changed(): void {
    for (const fn of [...this.#listeners]) fn();
  }
}
