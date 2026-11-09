// The live state of one character (Plan/25 section 4.3, 4.4): the profile and the containers from the REST routes, then the stream of
// the `CharacterWatch` on the tab's admin socket (details every 2 s, container snapshots when a container changes). The reducer is
// pure and tested; the hook only wires the sources to it.
//
// * Containers start from `GET /characters/{id}/items` (live from the game when the character or a sibling is online, else saved) and
//   are replaced by every snapshot of the watch. A changed cell flashes for one second, so an admin can watch a trade or an upgrade.
// * Banks are ACCOUNT-scoped (ADM-D8): when the character is offline and a character of its account is online, the banks are live
//   through that sibling, and the hook watches the sibling for them. When the character or the sibling goes offline the saved state is
//   read again.
// * A character that is offline is read again every 10 s: it may log in, or a sibling may.
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import type { CharacterDetail } from '../../../gen/proto/admin';
import { n64 } from '../../../net/int64';
import { shouldPoll } from '../../app/activity';
import type { CharacterProfile } from '../../gen/dto';
import { adminSocket, useLive } from '../../stores/live';
import { fetchContainers, fetchProfile } from './api';
import { changedSlots, modelOf, modelOfView, type ContainerModel } from './containers';

/** How long a changed cell stays marked (Plan/25 section 4.4). */
export const FLASH_MS = 1000;
/** The saved state of an offline character is read again this often. */
export const OFFLINE_REFRESH_MS = 10_000;

export interface LiveData {
  readonly containers: ReadonlyMap<string, ContainerModel>;
  /** Container key -> display slot -> change counter (its parity restarts the CSS animation). */
  readonly flash: ReadonlyMap<string, ReadonlyMap<number, number>>;
  /** Container key -> change counter of its penya. */
  readonly goldFlash: ReadonlyMap<string, number>;
  readonly counter: number;
}

export const EMPTY_LIVE: LiveData = {
  containers: new Map(),
  flash: new Map(),
  goldFlash: new Map(),
  counter: 0,
};

export type LiveAction =
  /** A one-shot read: replaces every container, nothing flashes. */
  | { readonly type: 'rest'; readonly models: readonly ContainerModel[] }
  /** A snapshot of the watch: replaces one container, the cells that changed flash. */
  | { readonly type: 'live'; readonly model: ContainerModel }
  /** The flash time is over. */
  | { readonly type: 'quiet' };

export function liveReducer(state: LiveData, action: LiveAction): LiveData {
  switch (action.type) {
    case 'rest':
      return {
        containers: new Map(action.models.map((m) => [m.key, m])),
        flash: new Map(),
        goldFlash: new Map(),
        counter: state.counter,
      };
    case 'quiet':
      return state.flash.size === 0 && state.goldFlash.size === 0
        ? state
        : { ...state, flash: new Map(), goldFlash: new Map() };
    case 'live': {
      const prev = state.containers.get(action.model.key);
      // a live snapshot keeps the label of the first load (the REST route names the owner, the stream does not)
      const model = prev
        ? {
            ...action.model,
            ownerName: action.model.ownerName === '' ? prev.ownerName : action.model.ownerName,
            liveViaName:
              action.model.liveViaName === '' && action.model.liveVia === prev.liveVia
                ? prev.liveViaName
                : action.model.liveViaName,
          }
        : action.model;
      const slots = changedSlots(prev, model);
      const goldChanged = prev !== undefined && prev.gold !== model.gold;
      const containers = new Map(state.containers);
      containers.set(model.key, model);
      if (slots.length === 0 && !goldChanged) return { ...state, containers };
      let counter = state.counter;
      const flash = new Map(state.flash);
      if (slots.length > 0) {
        const cells = new Map(flash.get(model.key));
        for (const slot of slots) {
          counter += 1;
          cells.set(slot, counter);
        }
        flash.set(model.key, cells);
      }
      const goldFlash = new Map(state.goldFlash);
      if (goldChanged) {
        counter += 1;
        goldFlash.set(model.key, counter);
      }
      return { containers, flash, goldFlash, counter };
    }
  }
}

/** The profile's overview with the numbers of the live detail laid over it. */
export function overview(
  profile: CharacterProfile | null,
  live: CharacterDetail | null,
): CharacterProfile['detail'] | null {
  if (!profile) return null;
  const base = profile.detail;
  if (!live?.online) return base;
  return {
    ...base,
    online: true,
    name: live.name,
    level: live.level,
    job: live.job,
    exp_permille: live.expPermille,
    str: live.str,
    sta: live.sta,
    dex: live.dex,
    int: live.int,
    stat_points: live.statPoints,
    skill_points: live.skillPoints,
    hp: live.hp,
    hp_max: live.hpMax,
    mp: live.mp,
    mp_max: live.mpMax,
    fp: live.fp,
    fp_max: live.fpMax,
    skills: live.skills.map((s) => ({ skill: s.skill, level: s.level })),
    buffs: live.buffs.map((b) => ({ buff: b.buff, level: b.level, remaining_ms: b.remainingMs })),
    quests: live.quests.map((q) => ({ quest: q.quest, state: q.state })),
    mode_flags: live.modeFlags,
    mute_until: n64(live.muteUntil),
    world: live.world,
    layer: live.layer,
    x: live.x,
    z: live.z,
    guild_id: live.guildId,
    party_id: live.partyId,
  };
}

export type SocketPhase = 'connecting' | 'live' | 'refused';

export interface CharacterLive {
  readonly status: 'loading' | 'ready' | 'error';
  readonly error: unknown;
  /** The containers could not be read (the profile could). */
  readonly containersError: unknown;
  readonly profile: CharacterProfile | null;
  /** The profile with the live numbers of the watch. */
  readonly detail: CharacterProfile['detail'] | null;
  readonly online: boolean;
  readonly containers: ReadonlyMap<string, ContainerModel>;
  readonly flash: LiveData['flash'];
  readonly goldFlash: LiveData['goldFlash'];
  readonly socket: SocketPhase;
  /** The reason a watch was refused (the capability, a limit), for a banner. */
  readonly refusal: string | null;
  /** Reads the profile and the containers again. */
  readonly reload: () => void;
  /** The online sibling through which the banks are live (the character itself when it is online), `0` = none. */
  readonly bankVia: number;
}

/**
 * Follows one character. `inventory`: the session may read containers (`view.inventory`); without it only the profile and the details
 * are read.
 */
export function useCharacterLive(characterId: number, inventory: boolean): CharacterLive {
  const epoch = useLive((s) => s.epoch);
  const [profile, setProfile] = useState<CharacterProfile | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loaded, setLoaded] = useState(false);
  const [containersError, setContainersError] = useState<unknown>(null);
  const [live, dispatch] = useReducer(liveReducer, EMPTY_LIVE);
  const [detail, setDetail] = useState<CharacterDetail | null>(null);
  // the phase of the watch belongs to the character and the socket it was opened on: another character starts again at `connecting`
  const watchKey = `${String(characterId)}:${String(epoch)}`;
  const [watch, setWatch] = useState<{
    readonly key: string;
    readonly phase: SocketPhase;
    readonly refusal: string | null;
  }>({ key: '', phase: 'connecting', refusal: null });
  const socket: SocketPhase = watch.key === watchKey ? watch.phase : 'connecting';
  const refusal = watch.key === watchKey ? watch.refusal : null;
  const [round, setRound] = useState(0);
  const reload = useCallback(() => {
    setRound((n) => n + 1);
  }, []);

  // the one-shot reads
  useEffect(() => {
    const controller = new AbortController();
    void (async () => {
      try {
        // the profile is the page; the containers are a part of it that may fail alone (and say so)
        const [p, c] = await Promise.all([
          fetchProfile(characterId),
          inventory
            ? fetchContainers(characterId).then(
                (v) => ({ ok: true as const, v }),
                (e: unknown) => ({ ok: false as const, e }),
              )
            : Promise.resolve(null),
        ]);
        if (controller.signal.aborted) return;
        setProfile(p);
        if (c?.ok) {
          dispatch({
            type: 'rest',
            models: c.v.containers.flatMap((v) => {
              const m = modelOfView(v);
              return m ? [m] : [];
            }),
          });
        }
        setContainersError(c && !c.ok ? c.e : null);
        setError(null);
      } catch (e) {
        if (!controller.signal.aborted) setError(e);
      } finally {
        if (!controller.signal.aborted) setLoaded(true);
      }
    })();
    return () => {
      controller.abort();
    };
  }, [characterId, inventory, round]);

  // the watch of the character
  const online = useRef<boolean | null>(null);
  useEffect(() => {
    online.current = null;
    const ws = adminSocket();
    const sub = ws.watchCharacter(characterId, {
      onDetail: (d) => {
        setWatch({ key: watchKey, phase: 'live', refusal: null });
        setDetail(d);
        if (online.current !== null && online.current !== d.online) reload();
        online.current = d.online;
      },
      onContainer: (c) => {
        const m = modelOf(c);
        if (m && inventory) dispatch({ type: 'live', model: m });
      },
      onError: (e) => {
        setWatch({ key: watchKey, phase: 'refused', refusal: e.message });
      },
    });
    return () => {
      sub.close();
    };
  }, [characterId, watchKey, inventory, reload]);

  const isOnline = detail?.online ?? profile?.detail.online ?? false;

  // the banks of an offline character are live through an online sibling
  const bankVia = useMemo(() => {
    if (isOnline) return characterId;
    for (const m of live.containers.values())
      if (m.kind === 'bank' && m.liveVia !== 0) return m.liveVia;
    return 0;
  }, [live.containers, isOnline, characterId]);
  useEffect(() => {
    if (!inventory || isOnline || bankVia === 0 || bankVia === characterId) return;
    const ws = adminSocket();
    let seen: boolean | null = null;
    const sub = ws.watchCharacter(bankVia, {
      onContainer: (c) => {
        const m = modelOf(c);
        if (m?.kind === 'bank') dispatch({ type: 'live', model: m });
      },
      onDetail: (d) => {
        // the sibling went away: the banks are the saved ones again
        if (seen === true && !d.online) reload();
        seen = d.online;
      },
    });
    return () => {
      sub.close();
    };
  }, [bankVia, characterId, epoch, inventory, isOnline, reload]);

  // an offline character may log in, a sibling may come online: read again now and then
  const idle = loaded && !isOnline && bankVia === 0;
  useEffect(() => {
    if (!idle) return;
    const id = setInterval(() => {
      if (shouldPoll()) reload();
    }, OFFLINE_REFRESH_MS);
    return () => {
      clearInterval(id);
    };
  }, [idle, reload]);

  // the flash is over after a second of quiet
  useEffect(() => {
    if (live.flash.size === 0 && live.goldFlash.size === 0) return;
    const id = setTimeout(() => {
      dispatch({ type: 'quiet' });
    }, FLASH_MS + 100);
    return () => {
      clearTimeout(id);
    };
  }, [live.counter, live.flash.size, live.goldFlash.size]);

  return {
    status: loaded ? (profile ? 'ready' : 'error') : 'loading',
    error,
    containersError,
    profile,
    detail: overview(profile, detail),
    online: isOnline,
    containers: live.containers,
    flash: live.flash,
    goldFlash: live.goldFlash,
    socket,
    refusal,
    reload,
    bankVia,
  };
}
