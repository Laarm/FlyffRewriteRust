// The actions of the map (Plan/25 section 3.10, 3.11, 3.12): ONE list per kind of object, used by the inspector card (buttons) and by the context menu
// (items), so a card and a right click never disagree. An action is shown only when the session holds its capability; the server decides for real
// (rank protection, stale targets, the typed confirmation) and the dialogs ask again when it does. The dialogs are the ones of the players views
// (`views/players/characterActions`) where the action exists there; the map adds the ones that pick a point, the kill with its options and the
// mute and ban with the chips that start chosen (the last choice of the operator, so a moderation from the map is two clicks).
import { useMemo, useState } from 'react';
import { DURATION_CHIPS } from '../../app/confirm';
import { copyText } from '../../app/clipboard';
import { runConfirmed } from '../../app/mutate';
import { Checkbox } from '../../components';
import type { ConfirmResult } from '../../components/ConfirmDialog';
import type { ActionResult, ActingCharacter } from '../../gen/dto';
import { openGrantDialog } from '../../grant';
import { safeStorage } from '../../../platform/storage';
import { toast } from '../../stores/toasts';
import { useCaps } from '../../stores/session';
import type { MapApi, PickedPoint } from '../../world/api';
import { Kind } from '../../world/types';
import { kick, message, resurrect, returnToTown, type Target } from '../players/characterActions';
import { banCharacter, muteCharacter, unmuteCharacter } from '../players/api';
import { openCharacter } from '../players/rowMenu';
import { I } from './InspectorStrings';
import {
  despawn,
  fetchObject,
  goToCharacter,
  goToPoint,
  heal,
  kill,
  skippedBecause,
  summon,
  teleport,
} from './InspectorApi';
import { useActingCharacter } from './InspectorData';
import { useWorldNames } from './worldNames';

// ---- the environment of an action ------------------------------------------------------------------------------------------------

export interface ActionEnv {
  readonly api: MapApi;
  readonly caps: ReadonlySet<string>;
  /** The acting GM character of this session, `null` without one. */
  readonly acting: ActingCharacter | null;
  readonly worldName: (id: number) => string;
}

/** The environment of the actions of a map: capabilities, acting character, world names. */
export function useActionEnv(api: MapApi): ActionEnv {
  const caps = useCaps();
  const acting = useActingCharacter();
  const names = useWorldNames();
  return useMemo(
    () => ({
      api,
      caps,
      acting,
      worldName: (id: number) => names.get(id) ?? `World ${String(id)}`,
    }),
    [api, caps, acting, names],
  );
}

/** One thing the operator can do with an object or a point. */
export interface MapAction {
  readonly id: string;
  readonly label: string;
  /** Destructive: drawn in the danger colour. */
  readonly danger?: boolean;
  /** Why the action cannot run now (it is shown, disabled, with the reason). */
  readonly disabled?: string;
  /** A thin line above it in a menu. */
  readonly separator?: boolean;
  readonly run: () => unknown;
}

/** An object of the map as the actions need it. */
export interface ObjRef {
  /** The object id of the frames. */
  readonly id: number;
  /** `EntityKind`. */
  readonly kind: number;
  readonly name: string;
  readonly world: number;
  readonly layer: number;
  readonly x: number;
  readonly z: number;
}

/** A player: the character id may be `0` while only the frames are known (it is read from the card when an action needs it). */
export interface PlayerRef extends ObjRef {
  readonly characterId: number;
  readonly account?: string;
}

// ---- small helpers -------------------------------------------------------------------------------------------------------------

const where = (o: Pick<ObjRef, 'world' | 'layer'>): { world: number; layer: number } => ({
  world: o.world,
  layer: o.layer,
});

/** The text of a finished group action: "3 done, 1 skipped (stale_target)" or the plain success text. */
export function summarize(result: ActionResult, done: (n: number) => string): string {
  if (result.failed > 0)
    return I.dialogs.groupSkipped(result.ok, result.failed, skippedBecause(result) ?? 'skipped');
  return done(result.ok);
}

function who(list: readonly ObjRef[], noun = 'objects'): string {
  const first = list[0];
  return list.length === 1 && first ? first.name : `${String(list.length)} ${noun}`;
}

/** The character id of a player: from the roster when known, else from the card of the object. */
export async function characterIdOf(p: PlayerRef): Promise<number> {
  if (p.characterId > 0) return p.characterId;
  try {
    return (await fetchObject(p.id)).character_id;
  } catch (error) {
    toast.error(error, I.actions.needsCharacter);
    return 0;
  }
}

async function targetOf(p: PlayerRef): Promise<Target | null> {
  const id = await characterIdOf(p);
  if (id === 0) return null;
  return {
    id,
    name: p.name,
    online: true,
    objId: p.id,
    world: p.world,
    layer: p.layer,
    x: p.x,
    z: p.z,
    ...(p.account ? { account: p.account } : {}),
  };
}

/** `/te <world> <x> <z>` for the clipboard. */
export function copyTe(world: number, x: number, z: number): void {
  const text = `/te ${String(world)} ${String(Math.round(x))} ${String(Math.round(z))}`;
  if (copyText(text)) toast.success(I.copied(text));
  else toast.error(new Error(I.copyFailed));
}

export function copyCoordinates(x: number, z: number): void {
  const text = `${String(Math.round(x))}, ${String(Math.round(z))}`;
  if (copyText(text)) toast.success(I.copied(text));
  else toast.error(new Error(I.copyFailed));
}

// ---- moving players -----------------------------------------------------------------------------------------------------------

/** Moves players to a point: the confirmation names the destination; above ten players, or when `typed` asks, the operator types `CONFIRM`. */
export async function teleportPlayers(
  env: ActionEnv,
  players: readonly PlayerRef[],
  to: PickedPoint,
  options: { readonly label?: 'point' | 'location'; readonly typed?: boolean } = {},
): Promise<boolean> {
  const ids = (await Promise.all(players.map((p) => characterIdOf(p)))).filter((id) => id > 0);
  if (ids.length === 0) return false;
  const name = who(players, 'players');
  const first = players[0];
  const out = await runConfirmed({
    title: I.dialogs.teleportTitle(name),
    message: I.dialogs.teleportMessage(name, env.worldName(to.world), to.x, to.z),
    confirmLabel: I.dialogs.teleportConfirm,
    facts: {
      targets: ids.length,
      ...(ids.length === 1 && first ? { targetName: first.name } : {}),
      ...(options.typed === true ? { irreversible: true } : {}),
    },
    run: (given) =>
      teleport(
        ids,
        { world: to.world, layer: to.layer, pos: [to.x, 0, to.z] },
        options.label ?? 'point',
        given,
      ),
    success: (r) => summarize(r, () => I.dialogs.teleportDone),
  });
  return out.ok;
}

/** "Teleport to point...": the next click on the map is the destination. */
export async function teleportToPoint(
  env: ActionEnv,
  players: readonly PlayerRef[],
  options: { readonly typed?: boolean } = {},
): Promise<boolean> {
  const point = await env.api.pickPoint(I.prompts.teleport(who(players, 'players')));
  if (!point) return false;
  return await teleportPlayers(env, players, point, { label: 'point', ...options });
}

export async function summonPlayer(env: ActionEnv, p: PlayerRef): Promise<boolean> {
  const id = await characterIdOf(p);
  if (id === 0) return false;
  const out = await runConfirmed({
    title: I.dialogs.summonTitle(p.name),
    message: I.dialogs.summonMessage,
    facts: { readOnly: true },
    run: () => summon(id, env.acting?.id ?? null),
    success: I.dialogs.summonDone(p.name),
  });
  return out.ok;
}

export async function goToPlayer(env: ActionEnv, p: PlayerRef): Promise<boolean> {
  const id = await characterIdOf(p);
  if (id === 0) return false;
  const out = await runConfirmed({
    title: I.actions.goTo,
    facts: { readOnly: true },
    run: () => goToCharacter(id, env.acting?.id ?? null),
    success: I.dialogs.goToDone,
  });
  return out.ok;
}

export async function goToMapPoint(env: ActionEnv, at: PickedPoint): Promise<boolean> {
  const out = await runConfirmed({
    title: I.actions.goHere,
    facts: { readOnly: true },
    run: () => goToPoint(at, env.acting?.id ?? null),
    success: I.dialogs.goToDone,
  });
  return out.ok;
}

// ---- combat actions ---------------------------------------------------------------------------------------------------------------

function KillOptions({ onChange }: { readonly onChange: (drops: boolean) => void }) {
  const [drops, setDrops] = useState(true);
  return (
    <div className="adm-stack">
      <p>{I.dialogs.killMessage}</p>
      <Checkbox
        label={I.dialogs.killDrops}
        checked={drops}
        onChange={(e) => {
          setDrops(e.target.checked);
          onChange(e.target.checked);
        }}
      />
    </div>
  );
}

/** Kills objects through the damage path; the typed confirmation is asked for players and above ten targets. */
export async function killObjects(env: ActionEnv, objs: readonly ObjRef[]): Promise<boolean> {
  const first = objs[0];
  if (!first) return false;
  const players = objs.filter((o) => o.kind === Kind.PLAYER).length;
  const choice = { drops: true };
  const out = await runConfirmed({
    title: I.dialogs.killTitle(who(objs)),
    message: (
      <KillOptions
        onChange={(d) => {
          choice.drops = d;
        }}
      />
    ),
    confirmLabel: 'Kill',
    danger: true,
    facts: {
      targets: objs.length,
      ...(objs.length === 1 ? { targetName: first.name } : {}),
      ...(players > 0 ? { irreversible: true } : {}),
    },
    run: (given) =>
      kill(
        objs.map((o) => o.id),
        choice.drops,
        where(first),
        given,
      ),
    success: (r) => summarize(r, I.dialogs.killDone),
  });
  if (out.ok) pruneSelection(env.api, objs);
  return out.ok;
}

export async function despawnObjects(env: ActionEnv, objs: readonly ObjRef[]): Promise<boolean> {
  const first = objs[0];
  if (!first) return false;
  const out = await runConfirmed({
    title: I.dialogs.despawnTitle(who(objs)),
    message: I.dialogs.despawnMessage,
    confirmLabel: 'Despawn',
    facts: { targets: objs.length, ...(objs.length === 1 ? { targetName: first.name } : {}) },
    run: (given) =>
      despawn(
        objs.map((o) => o.id),
        where(first),
        given,
      ),
    success: (r) => summarize(r, I.dialogs.despawnDone),
  });
  if (out.ok) pruneSelection(env.api, objs);
  return out.ok;
}

/** Heals movers; it asks nothing (Plan/25 section 3.12). */
export async function healObjects(objs: readonly ObjRef[]): Promise<boolean> {
  const first = objs[0];
  if (!first) return false;
  const out = await runConfirmed({
    title: I.dialogs.healTitle(who(objs)),
    facts: { readOnly: true },
    run: () =>
      heal(
        objs.map((o) => o.id),
        where(first),
      ),
    success: (r) => summarize(r, I.dialogs.healDone),
  });
  return out.ok;
}

/** The objects that were removed leave the selection. */
function pruneSelection(api: MapApi, gone: readonly ObjRef[]): void {
  const ids = new Set(gone.map((o) => o.id));
  const left = api.store.getState().selection.filter((id) => !ids.has(id));
  if (left.length !== api.store.getState().selection.length) api.select(left, 'set');
}

// ---- moderation from the map: the chips start on the last choice ---------------------------------------------------------------

const MOD_KEY = 'flyff.admin.map.moderation.v1';

interface ModChoice {
  readonly preset: string;
  readonly duration: string;
}

/** What the dialog starts with before the operator chose anything: a short mute for spam, a day for abuse. */
export const MOD_DEFAULTS: Readonly<Record<'mute' | 'ban', ModChoice>> = {
  mute: { preset: 'Spam', duration: '1h' },
  ban: { preset: 'Abuse', duration: '1d' },
};

export function lastChoice(kind: 'mute' | 'ban'): ModChoice {
  try {
    const raw = safeStorage.local.get(MOD_KEY);
    const all =
      raw === null ? {} : (JSON.parse(raw) as Partial<Record<string, Partial<ModChoice>>>);
    const mine = all[kind];
    return {
      preset: typeof mine?.preset === 'string' ? mine.preset : MOD_DEFAULTS[kind].preset,
      duration: typeof mine?.duration === 'string' ? mine.duration : MOD_DEFAULTS[kind].duration,
    };
  } catch {
    return MOD_DEFAULTS[kind];
  }
}

function rememberChoice(kind: 'mute' | 'ban', given: ConfirmResult): void {
  const preset = given.reason?.preset;
  const duration = given.duration?.id;
  if (preset === undefined && duration === undefined) return;
  try {
    const raw = safeStorage.local.get(MOD_KEY);
    const all = raw === null ? {} : (JSON.parse(raw) as Record<string, unknown>);
    const now = lastChoice(kind);
    safeStorage.local.set(
      MOD_KEY,
      JSON.stringify({
        ...all,
        [kind]: { preset: preset ?? now.preset, duration: duration ?? now.duration },
      }),
    );
  } catch {
    // the choice is a convenience
  }
}

export async function muteFromMap(p: PlayerRef): Promise<boolean> {
  const id = await characterIdOf(p);
  if (id === 0) return false;
  const start = lastChoice('mute');
  const out = await runConfirmed({
    title: I.dialogs.muteTitle(p.name),
    confirmLabel: 'Mute',
    reason: 'required',
    durations: DURATION_CHIPS,
    durationRequired: true,
    initial: start,
    facts: { targetName: p.name },
    run: (given) => {
      rememberChoice('mute', given);
      return muteCharacter(id, given.duration?.seconds ?? null, given);
    },
    success: I.dialogs.muted(p.name),
  });
  return out.ok;
}

export async function unmuteFromMap(p: PlayerRef): Promise<boolean> {
  const id = await characterIdOf(p);
  if (id === 0) return false;
  const out = await runConfirmed({
    title: I.actions.unmute,
    message: p.name,
    confirmLabel: I.actions.unmute,
    run: (given) => unmuteCharacter(id, given),
    success: I.dialogs.unmuted(p.name),
  });
  return out.ok;
}

export async function banFromMap(p: PlayerRef): Promise<boolean> {
  const id = await characterIdOf(p);
  if (id === 0) return false;
  const start = lastChoice('ban');
  const out = await runConfirmed({
    title: I.dialogs.banTitle(p.name),
    confirmLabel: 'Ban',
    danger: true,
    reason: 'required',
    durations: DURATION_CHIPS,
    durationRequired: true,
    initial: start,
    message: I.dialogs.banMessage,
    facts: { targetName: p.name },
    run: (given) => {
      rememberChoice('ban', given);
      const seconds = given.duration?.seconds;
      const until =
        seconds === null || seconds === undefined ? null : Math.floor(Date.now() / 1000) + seconds;
      return banCharacter(id, until, given);
    },
    success: I.dialogs.banned(p.account ?? p.name),
  });
  return out.ok;
}

/** The players among the selected objects that the frames know (the targets of "Teleport selected player here"). */
export function selectedPlayers(api: MapApi): PlayerRef[] {
  const { selection, worldId, layer } = api.store.getState();
  const out: PlayerRef[] = [];
  for (const id of selection) {
    const e = api.entity(id);
    if (e?.kind !== Kind.PLAYER) continue;
    out.push({
      id,
      kind: e.kind,
      name: e.name,
      world: worldId,
      layer,
      x: e.x,
      z: e.z,
      characterId: e.characterId,
    });
  }
  return out;
}

// ---- the lists of actions --------------------------------------------------------------------------------------------------------

interface Extras {
  /** The card is following this object. */
  readonly following?: boolean;
  /** The player is muted (the card knows from the character). */
  readonly muted?: boolean;
  /** Opens the teleport-to-location chooser (it lives in its own module: a dialog with the locations list). */
  readonly teleportToLocation?: (players: readonly PlayerRef[]) => void;
}

/** The actions of a player card, in the order of Plan/25 section 3.10; the ones the capabilities do not allow are left out. */
export function playerActions(env: ActionEnv, p: PlayerRef, extras: Extras = {}): MapAction[] {
  const has = (cap: string): boolean => env.caps.has(cap);
  const out: MapAction[] = [];
  const needActing = env.acting === null ? I.actions.needsActing : undefined;
  if (has('act.teleport.others')) {
    out.push({
      id: 'teleport-point',
      label: I.actions.teleportPoint,
      run: () => teleportToPoint(env, [p]),
    });
    if (extras.teleportToLocation) {
      const open = extras.teleportToLocation;
      out.push({
        id: 'teleport-location',
        label: I.actions.teleportLocation,
        run: () => {
          open([p]);
        },
      });
    }
  }
  if (has('act.teleport.self')) {
    out.push(
      {
        id: 'summon',
        label: I.actions.summon,
        ...(needActing ? { disabled: needActing } : {}),
        run: () => summonPlayer(env, p),
      },
      {
        id: 'goto',
        label: I.actions.goTo,
        ...(needActing ? { disabled: needActing } : {}),
        run: () => goToPlayer(env, p),
      },
    );
  }
  if (has('act.unstuck')) {
    out.push({
      id: 'return-to-town',
      label: I.actions.returnToTown,
      run: async () => {
        const t = await targetOf(p);
        if (t) await returnToTown(t);
      },
    });
  }
  if (has('act.items')) {
    out.push({
      id: 'grant',
      label: I.actions.grant,
      separator: true,
      run: async () => {
        const id = await characterIdOf(p);
        if (id > 0) openGrantDialog({ recipients: [{ id, name: p.name, online: true }] });
      },
    });
  }
  if (has('act.message.player')) {
    out.push({
      id: 'message',
      label: I.actions.message,
      run: async () => {
        const t = await targetOf(p);
        if (t) await message([t]);
      },
    });
  }
  if (has('act.kick')) {
    out.push({
      id: 'kick',
      label: I.actions.kick,
      danger: true,
      separator: true,
      run: async () => {
        const t = await targetOf(p);
        if (t) await kick(t);
      },
    });
  }
  if (has('act.mute')) {
    out.push(
      extras.muted === true
        ? { id: 'unmute', label: I.actions.unmute, run: () => unmuteFromMap(p) }
        : { id: 'mute', label: I.actions.mute, danger: true, run: () => muteFromMap(p) },
    );
  }
  if (has('act.ban')) {
    out.push({ id: 'ban', label: I.actions.ban, danger: true, run: () => banFromMap(p) });
  }
  if (has('act.combat')) {
    out.push(
      {
        id: 'heal',
        label: I.actions.heal,
        separator: true,
        run: () => healObjects([p]),
      },
      {
        id: 'resurrect',
        label: I.actions.resurrect,
        run: async () => {
          const t = await targetOf(p);
          if (t) await resurrect(t);
        },
      },
    );
  }
  if (has('view.inventory')) {
    out.push({
      id: 'inventory',
      label: I.actions.inventory,
      separator: true,
      run: async () => {
        const id = await characterIdOf(p);
        if (id > 0) openCharacter(id, 'items');
      },
    });
  }
  if (has('view.players')) {
    out.push({
      id: 'profile',
      label: I.actions.profile,
      ...(has('view.inventory') ? {} : { separator: true }),
      run: async () => {
        const id = await characterIdOf(p);
        if (id > 0) openCharacter(id);
      },
    });
  }
  out.push(
    {
      id: 'follow',
      label: extras.following === true ? I.actions.unfollow : I.actions.follow,
      separator: true,
      run: () => {
        env.api.follow(extras.following === true ? null : p.id);
      },
    },
    {
      id: 'copy-te',
      label: I.actions.copyTe,
      run: () => {
        copyTe(p.world, p.x, p.z);
      },
    },
  );
  return out;
}

/** The actions of a monster card. "Show spawn region" and "Respawn this region now" need the card (the region line); they are added by it. */
export function monsterActions(env: ActionEnv, m: ObjRef): MapAction[] {
  const out: MapAction[] = [];
  if (env.caps.has('act.combat')) {
    out.push(
      { id: 'kill', label: I.actions.kill, danger: true, run: () => killObjects(env, [m]) },
      { id: 'despawn', label: I.actions.despawn, run: () => despawnObjects(env, [m]) },
      { id: 'heal', label: I.actions.heal, run: () => healObjects([m]) },
    );
  }
  out.push(
    {
      id: 'follow',
      label:
        env.api.store.getState().follow?.objId === m.id ? I.actions.unfollow : I.actions.follow,
      separator: true,
      run: () => {
        env.api.follow(env.api.store.getState().follow?.objId === m.id ? null : m.id);
      },
    },
    {
      id: 'copy-te',
      label: I.actions.copyTe,
      run: () => {
        copyTe(m.world, m.x, m.z);
      },
    },
  );
  return out;
}

/** NPCs, pets, ground items and ctrls: removal for the ones the server allows, and the position. */
export function otherActions(env: ActionEnv, o: ObjRef): MapAction[] {
  const out: MapAction[] = [];
  if (env.caps.has('act.combat')) {
    out.push({
      id: 'despawn',
      label: o.kind === Kind.ITEM ? I.actions.remove : I.actions.despawn,
      danger: true,
      run: () => despawnObjects(env, [o]),
    });
  }
  out.push({
    id: 'copy-te',
    label: o.kind === Kind.NPC ? I.actions.copyPosition : I.actions.copyTe,
    run: () => {
      copyTe(o.world, o.x, o.z);
    },
  });
  return out;
}

/** The group actions of a multi-selection (Plan/25 section 3.10): at most 500 targets, the server counts. */
export function groupActions(env: ActionEnv, objs: readonly ObjRef[]): MapAction[] {
  const out: MapAction[] = [];
  const players = objs.filter((o): o is PlayerRef => o.kind === Kind.PLAYER && 'characterId' in o);
  const movers = objs.filter((o) => o.kind === Kind.MONSTER || o.kind === Kind.PLAYER);
  if (env.caps.has('act.combat')) {
    out.push(
      {
        id: 'kill-all',
        label: I.actions.killAll,
        danger: true,
        run: () => killObjects(env, movers),
      },
      {
        id: 'despawn-all',
        label: I.actions.despawnAll,
        run: () =>
          despawnObjects(
            env,
            objs.filter((o) => o.kind !== Kind.PLAYER),
          ),
      },
    );
    if (players.length > 0) {
      out.push({
        id: 'heal-all',
        label: I.actions.healAll,
        run: () => healObjects(players),
      });
    }
  }
  if (players.length > 0 && env.caps.has('act.message.player')) {
    out.push({
      id: 'message-all',
      label: I.actions.messageAll,
      run: async () => {
        const targets = (await Promise.all(players.map((p) => targetOf(p)))).filter(
          (t): t is Target => t !== null,
        );
        if (targets.length > 0) await message(targets);
      },
    });
  }
  if (players.length > 0 && env.caps.has('act.teleport.others')) {
    out.push({
      id: 'teleport-all',
      label: I.actions.teleportAll,
      run: () => teleportToPoint(env, players, { typed: true }),
    });
  }
  return out;
}

/** An empty point of the map (Plan/25 section 3.11); the spawn and drop dialogs come from the tools module through `extras`. */
export function pointActions(
  env: ActionEnv,
  at: PickedPoint,
  selected: readonly PlayerRef[],
  extras: {
    readonly spawn?: (at: PickedPoint) => void;
    readonly bookmark?: (at: PickedPoint) => void;
  } = {},
): MapAction[] {
  const out: MapAction[] = [];
  const has = (cap: string): boolean => env.caps.has(cap);
  if (has('act.teleport.others')) {
    out.push({
      id: 'teleport-selected',
      label: I.menu.teleportSelected(Math.max(1, selected.length)),
      ...(selected.length === 0 ? { disabled: I.menu.noSelection } : {}),
      run: () => teleportPlayers(env, selected, at, { label: 'point' }),
    });
  }
  if (has('act.teleport.self')) {
    out.push({
      id: 'go-here',
      label: I.menu.goHere,
      ...(env.acting === null ? { disabled: I.actions.needsActing } : {}),
      run: () => goToMapPoint(env, at),
    });
  }
  if (has('act.combat') && extras.spawn) {
    const open = extras.spawn;
    out.push({
      id: 'spawn-here',
      label: I.menu.spawnHere,
      separator: true,
      run: () => {
        open(at);
      },
    });
  }
  if (has('act.items')) {
    out.push({
      id: 'drop-here',
      label: I.menu.dropHere,
      ...(has('act.combat') && extras.spawn ? {} : { separator: true }),
      run: () => {
        openGrantDialog({ ground: { world: at.world, layer: at.layer, x: at.x, y: 0, z: at.z } });
      },
    });
  }
  if (has('act.world') && extras.bookmark) {
    const open = extras.bookmark;
    out.push({
      id: 'bookmark-here',
      label: I.menu.bookmarkHere,
      run: () => {
        open(at);
      },
    });
  }
  out.push(
    {
      id: 'copy-coordinates',
      label: I.menu.copyCoords,
      separator: true,
      run: () => {
        copyCoordinates(at.x, at.z);
      },
    },
    {
      id: 'copy-te',
      label: I.menu.copyTe,
      run: () => {
        copyTe(at.world, at.x, at.z);
      },
    },
    {
      id: 'measure',
      label: I.menu.measure,
      run: () => {
        env.api.setTool('ruler');
        env.api.rulerPoint(at.x, at.z);
      },
    },
  );
  return out;
}
