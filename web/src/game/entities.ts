// EntityTable (Plan/40 section 6.3, G5, 5.5): ObjId -> EntityRecord, outside React. Per-frame positions
// and matrices are NOT stored here (they come from the sim render view by id); the table holds what the
// protocol handlers know about an object: kind, last spawn message, hp/state, visual description, the
// asset handles the entity holds and the handles of the sim and the renderer.
//
// `DelObj` only MARKS an entity removed (`markRemoved`); `sweep()` at the end of a drain deletes it, so later
// messages of the same frame that name the entity (a `MoverDeath` attacker) still find it (G5).

import type {
  SpawnCtrl,
  SpawnItem,
  SpawnNpc,
  SpawnPlayer,
  SpawnSelf,
} from '../gen/proto/flyff/v1/world_pb';
import type { GuildBrief } from '../gen/proto/flyff/v1/social_types_pb';

export const EntityKind = { Player: 1, Npc: 2, Item: 3, Ctrl: 4, LocalPet: 5 } as const;
export type EntityKind = (typeof EntityKind)[keyof typeof EntityKind];

/** The part of an asset handle (Plan/40 section 9.3) the entity table needs: releasing it. */
export interface AssetHandleLike {
  release(): void;
}

/** `MoverVisualState` (Plan/41 section 12.1, owned by 41; this is its shape until 41's module lands). */
export interface MoverVisualState {
  id: number;
  kind: 'player' | 'monster' | 'npc' | 'pet';
  /** MoverProp id (movers bundle). */
  moverIndex: number;
  sex: 0 | 1;
  hairMesh: number;
  /** 0x00RRGGBB */
  hairColor: number;
  headMesh: number;
  skinSet: number;
  /** Key: PARTS_*. */
  equip: Map<number, { itemId: number; flag: number; option: number }>;
  hideCoat: [boolean, boolean, boolean, boolean, boolean];
  mode: number;
  authorization: number;
  disguiseMover: number | null;
  rideItem: number | null;
  scale: number;
  dmgCnt: number;
  name: string;
  title: string;
  guild: GuildBrief | null;
  /** `m_idWar`: the guild war the mover fights in (0 = none); an enemy war member is `idWar != 0 && idWar == own war && guild != own`. */
  idWar: number;
  /**
   * `m_nLevel` and `m_nJob` of a PLAYER other than the own one: set by the spawn, kept current by `SetLevel` / `JobChanged` (`features/guildmarks`).
   * The own values live in `selfStore` (its level-ups never reach `visual`). The master / hero icon above the name reads them.
   */
  level?: number;
  job?: number;
  pk: { chaotic: boolean; pink: boolean };
  partyMember: boolean;
  vendorTitle: string | null;
  activeAttack: boolean;
  peaceful: boolean;
  structure: string | null;
}

/** The last spawn message of an entity (immutable). */
export type EntitySpawn = SpawnSelf | SpawnPlayer | SpawnNpc | SpawnItem | SpawnCtrl;

export interface EntityRecord {
  readonly objId: number;
  kind: EntityKind;
  /** MoverProp / ItemProp / CtrlProp id. */
  propId: number;
  isSelf: boolean;
  idPlayer?: number;
  spawn: EntitySpawn;
  /** Movers only (filled by the spawn handlers, kept current by the feature handlers). */
  visual?: MoverVisualState;
  hp: number;
  maxHp?: number;
  state: number;
  /** 17's BuffSet mirror (Plan/17 section 11.1). */
  buffs?: unknown;
  /** Chat balloon (45 writes, 41 draws). */
  balloon?: { text: string; untilMs: number };
  /** ADD_MOVER was sent to the sim (movers only). */
  simHandle: boolean;
  /** 41's object handle. */
  renderHandle: unknown;
  /** Ref-counted, released when the entity is removed. */
  assets: AssetHandleLike[];
  /** `DelObj` was processed; the record is deleted by `sweep()` (G5). */
  removed: boolean;
  /** A local system pet (`EntityKind.LocalPet`): the object id of the player it follows (`SET_PARAMS.pet_owner`). */
  petOwner?: number;
}

/** What `spawn` needs; everything else defaults. A full `Omit<EntityRecord, 'objId'>` is accepted too. */
export interface EntitySpawnInit {
  kind: EntityKind;
  propId: number;
  spawn: EntitySpawn;
  isSelf?: boolean;
  idPlayer?: number;
  visual?: MoverVisualState;
  hp?: number;
  maxHp?: number;
  state?: number;
  buffs?: unknown;
  balloon?: { text: string; untilMs: number };
  simHandle?: boolean;
  renderHandle?: unknown;
  assets?: AssetHandleLike[];
  removed?: boolean;
  petOwner?: number;
}

/** The read side the renderer and the probe use (`EntityViewSource`, Plan/41 section 2.2). */
export interface EntityViewSource {
  get(id: number): EntityRecord | undefined;
  /** `0` iterates every kind. Entities marked removed are skipped. */
  forEach(kind: EntityKind | 0, fn: (r: EntityRecord) => void): void;
  /** Bumps whenever an entity is added or deleted (lists poll it). */
  readonly version: number;
}

export type EntityRemoveReason = 'sweep' | 'clear';

export interface EntityTable extends EntityViewSource {
  /** `true` while a record exists, including one that is marked removed. */
  has(id: number): boolean;
  self(): EntityRecord | undefined;
  /** Creates the record, or updates the existing one IN PLACE (same object; cancels a pending removal). */
  spawn(id: number, init: EntitySpawnInit): EntityRecord;
  markRemoved(id: number): void;
  /** Deletes every entity marked removed (end of a drain). Returns how many were deleted. */
  sweep(): number;
  clearAll(): void;
  /** Number of live (not removed) entities of a kind; `0` counts all. */
  count(kind: EntityKind | 0): number;
  /** Called for each deleted record (sweep and clear), before its assets are released. */
  onRemove(listener: (rec: EntityRecord, reason: EntityRemoveReason) => void): () => void;
}

export function createEntityTable(): EntityTable {
  const records = new Map<number, EntityRecord>();
  const pending = new Set<number>();
  const counts: Record<number, number> = {};
  const listeners = new Set<(rec: EntityRecord, reason: EntityRemoveReason) => void>();
  let version = 0;
  let selfId = 0;

  const bump = (kind: EntityKind, delta: number): void => {
    counts[kind] = (counts[kind] ?? 0) + delta;
  };

  const dispose = (rec: EntityRecord, reason: EntityRemoveReason): void => {
    for (const listener of [...listeners]) {
      try {
        listener(rec, reason);
      } catch {
        // a listener must not break the sweep; the loop's error accounting sees nothing here by design
      }
    }
    for (const handle of rec.assets) {
      try {
        handle.release();
      } catch {
        // releasing is best effort
      }
    }
    rec.assets = [];
  };

  return {
    get: (id) => records.get(id),
    has: (id) => records.has(id),
    self: () => (selfId === 0 ? undefined : records.get(selfId)),
    spawn(id, init) {
      const existing = records.get(id);
      if (existing) {
        // update in place (Plan/40 section 5.5): the identity, loaded models and the sim mover are kept
        if (existing.kind !== init.kind) {
          if (!existing.removed) bump(existing.kind, -1);
          bump(init.kind, 1);
        } else if (existing.removed) {
          bump(existing.kind, 1);
        }
        existing.kind = init.kind;
        existing.propId = init.propId;
        existing.spawn = init.spawn;
        if (init.isSelf !== undefined) existing.isSelf = init.isSelf;
        if (init.idPlayer !== undefined) existing.idPlayer = init.idPlayer;
        if (init.visual !== undefined) existing.visual = init.visual;
        if (init.hp !== undefined) existing.hp = init.hp;
        if (init.maxHp !== undefined) existing.maxHp = init.maxHp;
        if (init.state !== undefined) existing.state = init.state;
        if (init.buffs !== undefined) existing.buffs = init.buffs;
        if (init.balloon !== undefined) existing.balloon = init.balloon;
        if (init.petOwner !== undefined) existing.petOwner = init.petOwner;
        if (init.simHandle !== undefined) existing.simHandle = init.simHandle;
        if (init.renderHandle !== undefined) existing.renderHandle = init.renderHandle;
        if (init.assets !== undefined) existing.assets = init.assets;
        existing.removed = false; // an ADD_OBJ cancels a pending deletion (ref/09a section 6.1)
        pending.delete(id);
        if (existing.isSelf) selfId = id;
        return existing;
      }
      const rec: EntityRecord = {
        objId: id,
        kind: init.kind,
        propId: init.propId,
        isSelf: init.isSelf ?? false,
        spawn: init.spawn,
        hp: init.hp ?? 0,
        state: init.state ?? 0,
        simHandle: init.simHandle ?? false,
        renderHandle: init.renderHandle ?? null,
        assets: init.assets ?? [],
        removed: false,
      };
      if (init.idPlayer !== undefined) rec.idPlayer = init.idPlayer;
      if (init.visual !== undefined) rec.visual = init.visual;
      if (init.maxHp !== undefined) rec.maxHp = init.maxHp;
      if (init.buffs !== undefined) rec.buffs = init.buffs;
      if (init.balloon !== undefined) rec.balloon = init.balloon;
      if (init.petOwner !== undefined) rec.petOwner = init.petOwner;
      records.set(id, rec);
      bump(rec.kind, 1);
      version += 1;
      if (rec.isSelf) selfId = id;
      return rec;
    },
    markRemoved(id) {
      const rec = records.get(id);
      if (!rec || rec.removed) return;
      rec.removed = true;
      bump(rec.kind, -1);
      pending.add(id);
    },
    sweep() {
      let n = 0;
      for (const id of pending) {
        const rec = records.get(id);
        if (!rec?.removed) continue;
        records.delete(id);
        if (selfId === id) selfId = 0;
        dispose(rec, 'sweep');
        n += 1;
      }
      pending.clear();
      if (n > 0) version += 1;
      return n;
    },
    clearAll() {
      const all = [...records.values()];
      records.clear();
      pending.clear();
      selfId = 0;
      for (const kind of Object.keys(counts)) counts[Number(kind)] = 0;
      for (const rec of all) dispose(rec, 'clear');
      if (all.length > 0) version += 1;
    },
    forEach(kind, fn) {
      for (const rec of records.values()) {
        if (rec.removed) continue;
        if (kind === 0 || rec.kind === kind) fn(rec);
      }
    },
    count(kind) {
      if (kind !== 0) return counts[kind] ?? 0;
      let n = 0;
      for (const k of Object.keys(counts)) n += counts[Number(k)] ?? 0;
      return n;
    },
    onRemove(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    get version() {
      return version;
    },
  };
}
