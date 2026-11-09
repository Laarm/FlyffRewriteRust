// The protocol observers of the audio layer (Plan/44 §10.2, WP-AUDIO-04/07/08): `Audio.registerObservers(registry)` registers them with
// doc 40's `HandlerRegistry` at boot B5, before `registry.freeze()`. An observer READS the stores and the entity table and never writes
// them (40 §5.4 step 3: in dev builds the registry runs Pre/Fx handlers under the stores' read-only guard).
//
//   phase Pre  runs before every Core / Sim / Feature handler of the message: the OLD value is still in the stores
//              (SetExperience: level and skill points before the update; Replace: the director resets before the world changes)
//   phase Fx   runs after all of them: the new state (the entity of a `Spawn*` exists, the wearer's equipment is current)
//
// | trigger | message (phase)                         | sound                                                                        |
// | T07     | Damage (Fx)                             | the critical sound `snd_attack2` of the attacker's active-hand item           |
// | T08     | ProjectileHit (Fx), ProjectileSpawn (Fx)| the target's damage sound and the impact sound (`cues.projectileImpact`)       |
// | T12     | SpawnNpc (Fx)                           | `BGM_IN_BOSS` when a boss appears within 30 m                                 |
// | T13     | MoverDeath (Fx)                         | `BGM_IN_DEATH` for the local player                                           |
// | T14/15  | SetExperience (Pre)                     | `BGM_IN_LEVELUP` on a level up or more skill points                           |
// | T15     | InitSkillPoint (Pre)                    | `BGM_IN_LEVELUP` when the skill points grew                                   |
// | T16     | SetFxp (Pre)                            | `BGM_IN_LEVELUP` when the flight level grew                                   |
// | T17     | JobChanged (Fx)                         | `BGM_IN_LEVELUP` for the local player                                         |
// | T34     | PartyMember (Pre), PartyChangeTroup (Fx)| `BGM_IN_FITUP` when a party of 2 forms (0 -> 2 members), on the troupe change |
// | T35     | SetGuild (Fx)                           | `BGM_IN_COMPANY` when the LOCAL player joins a guild (id != 0); other movers and id 0 are silent |
// | T36     | GuildCombat gc_gc_state (Fx)            | `BGM_BA_CRISIS`, loops 0, when the siege war starts (state 2, war state 104) while in world 202 |
// | T18     | SpawnItem (Fx)                          | the item's drop sound (`sub_define`), also when the item enters view          |
// | T19     | CreateItem (Fx)                         | `SND_INF_GROUNDPICKUP`, once per touched slot                                 |
// | T20     | SetPointParam (Fx)                      | `SND_INF_TRADE` for a gold change of the local player                         |
// | T21/22  | DoEquipNtf (Fx)                         | the equip sound by item kind, `SND_INF_INVENTORYDROP` on unequip; own ride music |
// | T31     | PetLevelup (Fx), Pet (Fx)               | `PCSkillCastCheer.wav` when a pet in view grows a level (the own one arrives as `Pet` with PF_PET_LEVEL_UP) |
// | T32     | PetReleaseNtf (Fx)                      | `ItemGnCollector.wav` when the pet of any mover in view is released           |
// | T33     | UpdateItem (Fx)                         | `SND_INF_EQUIPACCESSORY` at the own player when a vis socket changes           |
// | T38     | SetQuest (Pre), RemoveQuestNtf (Pre)    | `BGM_EV_START` for a new quest, `BGM_EV_END` at state 14 and on a removal     |
// | T28     | PlaySound (Fx)                          | the server-named sound (the file branch only with the `voice` option)         |
// | T29     | PlayMusic (Fx)                          | `playMusicId(id, 1)`                                                          |
// | --      | Replace (Pre)                           | the director reset of §11.7                                                   |
//
// Only the audio side effect is here; the preloading hooks (`SpawnSelf`, own `DoEquipNtf`, `SpawnNpc` / `SpawnPlayer`) call the
// `Preloader` of §6.5 when one is given.

import { DST_GOLD, RANK_BOSS } from '../../gen/defs/attribute';
import { EntityKind } from '../../game/entities';
import {
  IK1_ARMOR,
  IK1_GENERAL,
  IK1_WEAPON,
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_CLOTH,
  IK2_JEWELRY,
} from '../../gen/defs/itemkind';
import { PARTS_RIDE } from '../../gen/defs/neuz';
import {
  BGM_BA_CRISIS,
  BGM_EV_END,
  BGM_EV_START,
  BGM_IN_BOSS,
  BGM_IN_COMPANY,
  BGM_IN_DEATH,
  BGM_IN_FITUP,
  BGM_IN_LEVELUP,
  SND_INF_EQUIPACCESSORY,
  SND_INF_EQUIPARMOR,
  SND_INF_EQUIPCLOTH,
  SND_INF_EQUIPWEAPON,
  SND_INF_GROUNDPICKUP,
  SND_INF_INVENTORYDROP,
  SND_INF_TRADE,
} from '../../gen/defs/sound';
import {
  WI_DUNGEON_BEAR,
  WI_DUNGEON_KRRR,
  WI_DUNGEON_MUSCLE,
  WI_WORLD_GUILDWAR,
} from '../../gen/defs/world';
import type { ProjectileKind } from '../../gen/proto/flyff/v1/combat_pb';
import {
  type Case,
  type HandlerRegistry,
  Phase as PHASE,
  type Phase,
  type Value,
} from '../../net/registry';
import type { AudioCues } from '../cues';
import { activeHandItemId } from '../cues';
import { type AudioData, type AudioStateReaders, isSoundId, type Vec3 } from '../deps';
import type { BgmDirector } from '../director';
import type { MusicPlayer } from '../music';
import type { AudioOptions } from '../options';
import type { SfxPlayer } from '../sfx';
import type { Preloader } from '../preload';

const OWNER = 'audio';

/** The literal files of the pet sounds (44 §9.2: `CMover::CreatePetSfx`, `PetRelease`). */
export const FILE_PET_LEVELUP = 'PCSkillCastCheer.wav';
export const FILE_PET_RELEASE = 'ItemGnCollector.wav';
/** `Pet.flag`: the pet leveled up (`PF_PET_LEVEL_UP`). */
const PF_PET_LEVEL_UP = 1;
/** `UpdateItem.param`: `UI_PETVIS_SIZE`, `UI_PETVIS_ITEM`, `UI_PETVIS_ITEMSWAP` (`Mover.h`). */
const UI_PETVIS_PARAMS: ReadonlySet<number> = new Set([15, 16, 17]);

/** `AF_CRITICAL1 | AF_CRITICAL2` (`flyff-sim types.rs`; ref/11 §3): any of the two bits. */
export const AF_CRITICAL = 0xc0;
/** A boss appearing closer than this many metres starts the boss music (`DPClient.cpp:1285-1300`). */
export const BOSS_MUSIC_DISTANCE = 30;
/** The worlds where the own death plays no jingle (`DPClient.cpp:2066-2090`). */
const NO_DEATH_JINGLE_WORLDS: ReadonlySet<number> = new Set([
  WI_WORLD_GUILDWAR,
  WI_DUNGEON_MUSCLE,
  WI_DUNGEON_KRRR,
  WI_DUNGEON_BEAR,
]);
/** `GcGcState.state` WAR (`CGuildCombat::WAR_STATE`) and `war_state` WAR_WAR (`WAR_WAR_STATE`): the siege fight begins. */
const GC_STATE_WAR = 2;
const GC_WAR_STATE_WAR_WAR = 104;
/** `MAX_QUEST`: the active records a character can hold (`__SetQuest` refuses the 101st, before its jingle). */
export const MAX_QUEST = 100;
/** `QS_END`: the quest state that finishes a quest. */
export const QS_END = 14;
/** The value of an empty quest slot on the wire (`0xffff`): ui-quest ignores it, so does the jingle. */
const EMPTY_QUEST_ID = 0xffff;
/** Spawned projectiles remembered for their impact (a spawn whose hit never arrives must not leak). */
const MAX_REMEMBERED_PROJECTILES = 256;

/** The registry surface the observers need. */
export type ObserverRegistry = Pick<HandlerRegistry, 'on'>;

export interface NetBindingDeps {
  readonly sfx: Pick<SfxPlayer, 'playId' | 'playFile'>;
  readonly music: Pick<MusicPlayer, 'playMusicId'>;
  readonly director: Pick<BgmDirector, 'onReplace' | 'rideEquipped'>;
  readonly cues: Pick<AudioCues, 'projectileImpact'>;
  readonly state: AudioStateReaders;
  readonly data: () => AudioData | null;
  readonly options: () => Pick<AudioOptions, 'voice'>;
  /** The prefetch policy (44 §6.5); optional (the tests of single triggers omit it). */
  readonly preload?: Pick<Preloader, 'onSelfSpawned' | 'onOwnEquip' | 'onMoverKind'>;
}

/** Registers the audio observers; every handler catches and counts its own errors (44 §2.3). */
export function registerNetObservers(
  registry: ObserverRegistry,
  deps: NetBindingDeps,
  onError: (what: string, error: unknown) => void = () => undefined,
): { readonly projectiles: ReadonlyMap<number, unknown> } {
  const { sfx, music, state, director } = deps;
  const projectiles = new Map<number, { kind: ProjectileKind; hitSoundId: number }>();
  const tmp: [number, number, number] = [0, 0, 0];

  /** Registers `fn` guarded: an error is counted (`onError`), never thrown into the registry's H2 path. */
  const on = <K extends Case>(
    c: K,
    phase: Phase,
    fn: (objId: number, v: Value<K>) => void,
  ): void => {
    registry.on(c, phase, OWNER, (objId, v) => {
      try {
        fn(objId, v);
      } catch (error) {
        onError(c, error);
      }
    });
  };

  const posOf = (id: number): Vec3 | undefined =>
    state.posOf(id, tmp) ? [tmp[0], tmp[1], tmp[2]] : undefined;

  // -- world changes ------------------------------------------------------------------------------------------------
  on('replace', PHASE.Pre, () => {
    director.onReplace();
    projectiles.clear();
  });

  // -- T29 / T28: server-driven music and sounds ----------------------------------------------------------------------
  on('playMusic', PHASE.Fx, (_obj, m) => {
    music.playMusicId(m.musicId, 1);
  });
  on('playSound', PHASE.Fx, (_obj, m) => {
    const s = m.sound;
    if (s.case === 'id') {
      sfx.playId(s.value);
    } else if (s.case === 'file') {
      // the file branch needs a local player and the `voice` option (DPClient.cpp:11823-11845)
      if (state.self().objId !== 0 && deps.options().voice) sfx.playFile(s.value);
    }
  });

  // -- T07: the critical hit sound -----------------------------------------------------------------------------------
  on('damage', PHASE.Fx, (_obj, m) => {
    if ((m.atkFlags & AF_CRITICAL) === 0) return;
    const attacker = state.mover(m.attacker);
    if (attacker === undefined || attacker.removed) return;
    if (attacker.kind !== EntityKind.Player && attacker.kind !== EntityKind.Npc) return;
    const data = deps.data();
    if (data === null) return;
    const found = activeHandItemId(attacker, data);
    const item = found === null ? undefined : data.item(found.itemId);
    if (item !== undefined && isSoundId(item.snd_attack2)) {
      sfx.playId(item.snd_attack2, posOf(m.attacker));
    }
  });

  // -- T08: projectile impacts ---------------------------------------------------------------------------------------
  on('projectileSpawn', PHASE.Fx, (_obj, m) => {
    if (projectiles.size >= MAX_REMEMBERED_PROJECTILES) {
      const oldest = projectiles.keys().next();
      if (oldest.done !== true) projectiles.delete(oldest.value);
    }
    projectiles.set(m.projectileId, { kind: m.kind, hitSoundId: m.hitSoundId });
  });
  on('projectileHit', PHASE.Fx, (attackerId, m) => {
    const spawn = projectiles.get(m.projectileId);
    projectiles.delete(m.projectileId);
    if (m.expired) return; // a timeout or a vanished target: silent
    const p = m.pos;
    deps.cues.projectileImpact({
      kind: spawn?.kind ?? 0,
      attacker: attackerId,
      target: m.targetObjId,
      pos: p === undefined ? [0, 0, 0] : [p.x, p.y, p.z],
      hitSoundId: spawn?.hitSoundId ?? 0,
    });
  });

  // -- spawns: T12 (boss), T18 (item drop sound), the prefetch hooks -----------------------------------------------------
  on('spawnNpc', PHASE.Fx, (objId, m) => {
    const prop = state.mover(objId)?.propId ?? m.obj?.index ?? 0;
    deps.preload?.onMoverKind(prop);
    const data = deps.data();
    if (data === null || data.mover(prop)?.class !== RANK_BOSS) return;
    const at = m.obj?.pos;
    const me: [number, number, number] = [0, 0, 0];
    if (at === undefined || !state.selfPos(me)) return;
    const d = Math.hypot(at.x - me[0], at.y - me[1], at.z - me[2]);
    if (d < BOSS_MUSIC_DISTANCE) music.playMusicId(BGM_IN_BOSS, 1);
  });
  on('spawnPlayer', PHASE.Fx, (objId) => {
    const rec = state.mover(objId);
    if (rec !== undefined) deps.preload?.onMoverKind(rec.propId);
  });
  on('spawnSelf', PHASE.Fx, () => {
    deps.preload?.onSelfSpawned();
  });
  on('spawnItem', PHASE.Fx, (objId, m) => {
    const id = m.item?.itemId ?? state.mover(objId)?.propId ?? 0;
    const sound = deps.data()?.item(id)?.sub_define;
    if (!isSoundId(sound)) return;
    const p = m.obj?.pos;
    sfx.playId(sound, p === undefined ? undefined : [p.x, p.y, p.z]);
  });

  // -- T31 / T32 / T33: pets -----------------------------------------------------------------------------------------
  // `CMover::CreatePetSfx` plays the cheer when the model of a pet that grew is rebuilt; the other players' pets arrive as `PetLevelup` (KEEP-QUIRK: audible from
  // anywhere in view), the own one as `Pet` with the level flag. `PetRelease` plays whenever a mover in view loses its pet.
  on('petLevelup', PHASE.Fx, (objId) => {
    const owner = state.mover(objId);
    if (owner !== undefined && !owner.removed) sfx.playFile(FILE_PET_LEVELUP);
  });
  on('pet', PHASE.Fx, (_obj, m) => {
    if (m.flag === PF_PET_LEVEL_UP && state.self().objId !== 0) sfx.playFile(FILE_PET_LEVELUP);
  });
  on('petReleaseNtf', PHASE.Fx, (objId) => {
    const owner = state.mover(objId);
    if (owner !== undefined && !owner.removed) sfx.playFile(FILE_PET_RELEASE);
  });
  on('updateItem', PHASE.Fx, (objId, m) => {
    if (!UI_PETVIS_PARAMS.has(m.param)) return;
    const self = state.self().objId;
    if (self === 0 || (objId !== 0 && objId !== self)) return;
    sfx.playId(SND_INF_EQUIPACCESSORY, posOf(self));
  });

  // -- T38: the quest jingles (WP-AUDIO-13; `CMover::SetQuest` / `__SetQuest` / `RemoveQuest`, `MoverParam.cpp:3494-3645`, `OnSetQuest` of `DPClient.cpp:8533`) ----------------------
  // Own character only. Phase Pre: the book still holds the OLD records (ui-quest applies the message in the normal phase). `SetQuest` of a quest that is not active and already
  // completed returns FALSE before any jingle; a new quest plays START unless the book is full (the 300-completed cap that stopped after START is gone with PL-160); then, in the same
  // call, state 14 plays END, so a new quest that arrives finished plays START then END and END wins (KEEP); an active quest updated to another state is silent. `RemoveQuest`
  // always plays END, also for an unknown quest; the modes 1 (all) and 2 (completed list) never reach it.
  const isOwn = (objId: number): boolean => {
    const self = state.self().objId;
    return self !== 0 && (objId === 0 || objId === self);
  };
  on('setQuest', PHASE.Pre, (objId, m) => {
    const book = state.quests;
    const q = m.quest;
    if (book === undefined || q === undefined || q.questId === EMPTY_QUEST_ID) return;
    if (!isOwn(objId)) return;
    if (book.isActive(q.questId)) {
      if (q.state === QS_END) music.playMusicId(BGM_EV_END, 1);
      return;
    }
    if (book.isCompleted(q.questId)) return;
    if (book.activeCount() >= MAX_QUEST) return;
    music.playMusicId(BGM_EV_START, 1);
    if (q.state === QS_END) music.playMusicId(BGM_EV_END, 1);
  });
  on('removeQuestNtf', PHASE.Pre, (objId, m) => {
    if (!isOwn(objId)) return;
    if (m.mode === -1 || m.mode === 0) music.playMusicId(BGM_EV_END, 1);
  });

  // -- T13: the own death -------------------------------------------------------------------------------------------
  on('moverDeath', PHASE.Fx, (objId) => {
    const self = state.self().objId;
    if (self === 0 || objId !== self) return;
    if (NO_DEATH_JINGLE_WORLDS.has(state.world().worldId)) return;
    if (state.inGuildCombat1to1?.() === true) return;
    music.playMusicId(BGM_IN_DEATH, 1);
  });

  // -- T14 / T15: level and skill points (the old values are still in the store) -----------------------------------------------
  on('setExperience', PHASE.Pre, (_obj, m) => {
    const self = state.self();
    if (self.objId === 0) return;
    if (m.level > self.level) music.playMusicId(BGM_IN_LEVELUP, 1);
    // a second call in the same message is a same-file no-op
    if (self.skillPoints < m.skillPoint) music.playMusicId(BGM_IN_LEVELUP, 1);
  });

  // -- T15: the skill points grew (the old value is still in the store) -----------------------------------------------------
  on('initSkillPoint', PHASE.Pre, (_obj, m) => {
    const self = state.self();
    if (self.objId === 0) return;
    if (self.skillPoints < m.skillPoint) music.playMusicId(BGM_IN_LEVELUP, 1);
  });

  // -- T16: the flight level grew (`CMover::SetFxp`) ------------------------------------------------------------------------
  on('setFxp', PHASE.Pre, (_obj, m) => {
    const self = state.self();
    if (self.objId === 0) return;
    if (m.flightLevel > (self.flightLevel ?? 0)) music.playMusicId(BGM_IN_LEVELUP, 1);
  });

  // -- T17: the own job changed (`OnSetChangeJob`, `IsActiveMover` only) ----------------------------------------------------------
  on('jobChanged', PHASE.Fx, (objId) => {
    const self = state.self().objId;
    if (self !== 0 && objId === self) music.playMusicId(BGM_IN_LEVELUP, 1);
  });

  // -- T34: a party formed (0 -> 2 members, `nOldSize == 0 && nSizeofMember == 2`) or turned into a troupe ----------------------------
  // The old member count is this observer's own: every `PartyMember` carries the whole record (absent = the receiver is in no party).
  let partySize = 0;
  // back at the character list the previous session's party is gone
  on('charList', PHASE.Pre, () => {
    partySize = 0;
  });
  on('partyMember', PHASE.Pre, (_obj, m) => {
    const old = partySize;
    partySize = m.party?.members.length ?? 0;
    if (old === 0 && partySize === 2) music.playMusicId(BGM_IN_FITUP, 1);
  });
  on('partyChangeTroup', PHASE.Fx, () => {
    music.playMusicId(BGM_IN_FITUP, 1);
  });

  // -- T35: the local player joined a guild (`OnSetGuild`: `PlayMusic( BGM_IN_COMPANY, 1 )` for the active mover when the guild is known) ----------
  // No directory check: the server orders the guild's brief before `SetGuild`. Nothing plays at login (the join batch carries `Guild`, not `SetGuild`),
  // for another mover, or for id 0 (the player left).
  on('setGuild', PHASE.Fx, (objId, m) => {
    const self = state.self().objId;
    if (self !== 0 && objId === self && m.idGuild !== 0) music.playMusicId(BGM_IN_COMPANY, 1);
  });

  // -- T36: the siege war starts (`OnGuildCombatState`, `GC_GCSTATE`): the crisis music loops until the next jingle or world change replaces it ------------
  // KEEP: no other state, other world or later state stops it. (`music.playMusicId` rule M3: a call while the same file plays is a no-op.)
  on('guildCombat', PHASE.Fx, (_obj, m) => {
    if (m.sub.case !== 'gcGcState') return;
    if (m.sub.value.state !== GC_STATE_WAR || m.sub.value.warState !== GC_WAR_STATE_WAR_WAR) return;
    if (state.world().worldId !== WI_WORLD_GUILDWAR) return;
    music.playMusicId(BGM_BA_CRISIS, 0);
  });

  // -- T19 / T20: pickups and gold ----------------------------------------------------------------------------------
  on('createItem', PHASE.Fx, (_obj, m) => {
    const self = state.self().objId;
    if (self === 0) return;
    const at = posOf(self);
    // once per affected slot of the message (KEEP-QUIRK: n identical voices)
    m.objIds.forEach(() => {
      sfx.playId(SND_INF_GROUNDPICKUP, at);
    });
  });
  on('setPointParam', PHASE.Fx, (objId, m) => {
    if (m.dst !== DST_GOLD) return;
    const self = state.self().objId;
    if (self === 0 || (objId !== 0 && objId !== self)) return;
    sfx.playId(SND_INF_TRADE);
  });

  // -- T21 / T22: equip and unequip -------------------------------------------------------------------------------------
  on('doEquipNtf', PHASE.Fx, (objId, m) => {
    const wearer = state.mover(objId);
    const self = state.self().objId;
    const isSelf = self !== 0 && objId === self;
    if (isSelf) deps.preload?.onOwnEquip();
    if (wearer === undefined || wearer.removed) return;
    const at = posOf(objId);
    if (!m.equip) {
      sfx.playId(SND_INF_INVENTORYDROP, at);
      return;
    }
    const item = deps.data()?.item(m.info?.itemId ?? 0);
    if (item === undefined) return;
    const sound = equipSound(item.item_kind1, item.item_kind2);
    if (sound !== 0) sfx.playId(sound, at);
    if (isSelf && item.parts === PARTS_RIDE) director.rideEquipped();
  });

  return { projectiles };
}

/** The equip sound of an item's kinds, first match in the legacy order; 0 = none (C precedence of `a && b || c` kept). */
export function equipSound(kind1: number, kind2: number): number {
  if (kind1 === IK1_WEAPON) return SND_INF_EQUIPWEAPON;
  if (kind1 === IK1_GENERAL && kind2 === IK2_JEWELRY) return SND_INF_EQUIPACCESSORY;
  if (kind1 === IK1_ARMOR && kind2 === IK2_CLOTH) return SND_INF_EQUIPCLOTH;
  if ((kind1 === IK1_ARMOR && kind2 === IK2_ARMOR) || kind2 === IK2_ARMORETC) {
    return SND_INF_EQUIPARMOR;
  }
  return 0;
}
