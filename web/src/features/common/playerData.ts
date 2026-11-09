// The player-data cache (`playerDataStore`, Plan/45 §2.8; WP-CFEAT-34; ref/09b §3.8, §7.5; Plan/21 §3.2): the name, level, job, sex and
// online flag of every character the client has heard of - party members out of view, guild members, friends. The legacy
// `CPlayerDataCenter` (`src:_Common/playerdata.cpp`) with its two bulk rules:
//
//   * `need(id)` = `GetPlayerData(id, bQuery = TRUE)`: a miss inserts a placeholder and sends `QueryPlayerDataReq{id, 0}` ONCE (the
//     placeholder stops the repeated queries); the answer is `QueryPlayerDataNtf`.
//   * `needAll(ids)` = the list form (guild members, friends, campus members, rankings): ONE `QueryPlayerData2` per chunk of at most
//     1024 entries `(id, cached version or 0)`; the server only answers the stale ones (Plan/21 §3.2, a larger request is ignored whole).
//
// Writers: `QueryPlayerDataNtf` and `UpdatePlayerData` replace the fields of an entry; `version == 0` keeps the cached version and
// online flag (`AddPlayerData`, `playerdata.cpp:62-88`); an older version than the cached one is stale and ignored (`OnUpdatePlayerData`
// copies only a NEWER version; an equal one is taken, which fills the name of a placeholder that an update created).
// `PresenceLogout` sets the entry offline; `QuerySetPlayerNameNtf` renames it. The store lives for the session (reset on the next
// login, not on a world change).

import { create } from '@bufbuild/protobuf';
import type { StoreApi } from 'zustand/vanilla';
import { PlayerDataVersionSchema, type PlayerData } from '../../gen/proto/flyff/v1/social_types_pb';
import {
  QueryPlayerData2Schema,
  QueryPlayerDataReqSchema,
} from '../../gen/proto/flyff/v1/messenger_pb';
import type { ClientContext } from '../../app/context';
import type { ClientMsg } from '../../net/codec';
import type { StoreRegistry } from '../../stores/registry';
import type { FeatureContext } from './module';

/** One character as the client knows it (`PlayerData`: `szPlayer` + `sPlayerData`). */
export interface PlayerDataEntry {
  readonly name: string;
  readonly level: number;
  /** The job id (`JOB_*`). */
  readonly job: number;
  /** 0 male, 1 female. */
  readonly sex: number;
  /** `uLogin != 0`. */
  readonly online: boolean;
  /** The data version (`nVer`); 0 = never answered (a placeholder). */
  readonly ver: number;
}

export interface PlayerDataState {
  readonly entries: ReadonlyMap<number, PlayerDataEntry>;
}

export type PlayerDataStore = StoreApi<PlayerDataState>;

/** At most this many ids per `QueryPlayerData2` (the server ignores a larger request, Plan/21 §3.2). */
export const QUERY_CHUNK = 1024;

/** The entry a miss inserts (`new PlayerData`, level 1). */
export function placeholder(): PlayerDataEntry {
  return { name: '', level: 1, job: 0, sex: 0, online: false, ver: 0 };
}

export function initialPlayerDataState(): PlayerDataState {
  return { entries: new Map() };
}

/** The store of a registry (created on first use). Scope `session`: kept across world changes, reset on `Login`. */
export function definePlayerDataStore(registry: StoreRegistry): PlayerDataStore {
  const existing = registry.get('playerData');
  if (existing) return existing as unknown as PlayerDataStore;
  return registry.defineStore<PlayerDataState>({
    name: 'playerData',
    scope: 'session',
    owner: '45',
    initial: initialPlayerDataState,
    fixtures: {
      party: {
        entries: new Map([
          [1, { name: 'Aibatt', level: 15, job: 1, sex: 0, online: true, ver: 3 }],
          [2, { name: 'Mirabelle', level: 60, job: 7, sex: 1, online: false, ver: 2 }],
        ]),
      },
    },
  });
}

export interface PlayerDataApi {
  readonly store: PlayerDataStore;
  /** `GetPlayerData(id, FALSE)`: never queries. */
  get(id: number): PlayerDataEntry | undefined;
  /** `GetPlayerData(id, TRUE)`: a miss inserts the placeholder and queries once. Never call it while React renders. */
  need(id: number): PlayerDataEntry;
  /** The list form: `QueryPlayerData2` chunks of at most 1024 entries. */
  needAll(ids: readonly number[]): void;
  /** The name of a character, `''` when unknown (no query). */
  nameOf(id: number): string;
  /** `OnQueryPlayerData`. */
  onQuery(id: number, name: string, data: PlayerData | undefined): void;
  /** `OnUpdatePlayerData`. */
  onUpdate(id: number, data: PlayerData | undefined): void;
  /** `OnLogout` (`PresenceLogout`). */
  onLogout(id: number): void;
  /** `QuerySetPlayerNameNtf`: the new name of a known character. */
  onRename(id: number, name: string): void;
}

/** The sender port: one `ClientEnvelope`. */
export type PlayerDataSend = (msg: ClientMsg) => void;

/** The `PlayerData` wire record as an entry; `prev` supplies the version and the online flag of a `version == 0` record. */
function merged(
  prev: PlayerDataEntry | undefined,
  name: string | null,
  data: PlayerData,
): PlayerDataEntry {
  const keep = data.version === 0 && prev !== undefined;
  return {
    name: name ?? prev?.name ?? '',
    level: data.level,
    job: data.job,
    sex: data.sex,
    online: keep ? prev.online : data.login !== 0,
    ver: keep ? prev.ver : data.version,
  };
}

/** The cache over a store; `send` is the sender of the client (`ctx.net.sender.send`). */
export function createPlayerData(store: PlayerDataStore, send: PlayerDataSend): PlayerDataApi {
  const set = (id: number, entry: PlayerDataEntry): void => {
    const next = new Map(store.getState().entries);
    next.set(id, entry);
    store.setState({ entries: next });
  };
  const get = (id: number): PlayerDataEntry | undefined => store.getState().entries.get(id);

  const need = (id: number): PlayerDataEntry => {
    const hit = get(id);
    if (hit) return hit;
    const entry = placeholder();
    set(id, entry);
    // legacy-send: SendQueryPlayerData
    send({
      case: 'queryPlayerDataReq',
      value: create(QueryPlayerDataReqSchema, { idPlayer: id, version: 0 }),
    });
    return entry;
  };

  const needAll = (ids: readonly number[]): void => {
    const seen = new Set<number>();
    const wanted: { idPlayer: number; version: number }[] = [];
    const entries = new Map(store.getState().entries);
    let inserted = false;
    for (const id of ids) {
      if (seen.has(id)) continue;
      seen.add(id);
      let entry = entries.get(id);
      if (!entry) {
        entry = placeholder();
        entries.set(id, entry);
        inserted = true;
      }
      wanted.push({ idPlayer: id, version: entry.ver });
    }
    if (inserted) store.setState({ entries });
    for (let i = 0; i < wanted.length; i += QUERY_CHUNK) {
      // `SendQueryPlayerData(vector<PDVer>)`: the batch form (QUERY_PLAYER_DATA2)
      // legacy-send: SendQueryPlayerData
      send({
        case: 'queryPlayerData2',
        value: create(QueryPlayerData2Schema, {
          entries: wanted
            .slice(i, i + QUERY_CHUNK)
            .map((e) =>
              create(PlayerDataVersionSchema, { idPlayer: e.idPlayer, version: e.version }),
            ),
        }),
      });
    }
  };

  /** A version older than the cached one is stale (an equal one is taken). */
  const stale = (prev: PlayerDataEntry | undefined, data: PlayerData): boolean =>
    prev !== undefined && data.version !== 0 && data.version < prev.ver;

  return {
    store,
    get,
    need,
    needAll,
    nameOf: (id) => get(id)?.name ?? '',
    onQuery(id, name, data) {
      if (!data) return;
      const prev = get(id);
      if (stale(prev, data)) return;
      set(id, merged(prev, name, data));
    },
    onUpdate(id, data) {
      if (!data) return;
      // an update of an unknown character queries its name (`GetPlayerData(id)` with `bQuery = TRUE`)
      const prev = get(id) ?? need(id);
      if (stale(prev, data)) return;
      set(id, merged(prev, null, data));
    },
    onLogout(id) {
      const prev = need(id);
      if (prev.online) set(id, { ...prev, online: false });
    },
    onRename(id, name) {
      const prev = get(id);
      if (prev && prev.name !== name) set(id, { ...prev, name });
    },
  };
}

const apis = new WeakMap<object, PlayerDataApi>();

/** The cache of a client context (created on first use; the store is the `playerData` store of its registry). */
export function playerDataOf(ctx: ClientContext): PlayerDataApi {
  const known = apis.get(ctx);
  if (known) return known;
  const api = createPlayerData(definePlayerDataStore(ctx.stores.registry), (msg) => {
    ctx.net.sender.send(msg);
  });
  apis.set(ctx, api);
  return api;
}

/**
 * Registers the four writers of the cache with the handler registry (`QueryPlayerDataNtf`, `UpdatePlayerData`, `PresenceLogout`,
 * `QuerySetPlayerNameNtf`). Called by the party feature; the guild and messenger features read the same cache through `playerDataOf`.
 */
export function registerPlayerData(fc: FeatureContext): PlayerDataApi {
  const api = playerDataOf(fc.app);
  fc.net.on('queryPlayerDataNtf', (m) => {
    api.onQuery(m.idPlayer, m.name, m.data);
  });
  fc.net.on('updatePlayerData', (m) => {
    api.onUpdate(m.idPlayer, m.data);
  });
  fc.net.on('presenceLogout', (m) => {
    api.onLogout(m.idPlayer);
  });
  fc.net.on('querySetPlayerNameNtf', (m) => {
    api.onRename(m.idPlayer, m.name);
  });
  return api;
}
