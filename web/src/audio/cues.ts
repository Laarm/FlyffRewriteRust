// `AudioCues` (Plan/44 §10.1, WP-AUDIO-07/08): the typed trigger functions other client modules call. Every cue is fire-and-forget,
// cheap and safe in any state (no-ops while the engine is `unavailable` / `closed`; while locked only `ui` sounds start and the
// music state still updates). The protocol observers (`bindings/net.ts`) and the sim-event consumer (`bindings/sim.ts`) call them
// too, so each rule of the §9 catalogue lives in exactly one place.
//
//   T06/T10 meleeHit        T08 projectileImpact     T23 groundDrop        T25 uiClick / click      T26 moverClicked
//   T24 shopConfirm         T27 dialogOpened         T30 upgradeResult     T37 instantMessage       T39 miniGame
//   T40 localAttackStarted  T41/T42 sfxCreated       T02-T04 objectAttrSound (reserved)
//
// Cues of later milestones whose rule is a single line (T24, T27, T30, T37, T39) are complete here so the windows that call them can
// be written now; the work packages that own those windows (AUDIO-10..13) wire and test the call sites. `skillEffectFired` (T09) and
// `hotAirPainTick` (T08h) are the M5 cues of WP-AUDIO-11 (called by the skill runtime and the HotAir effect); `bossMeleeSpecial`
// (T11) is the M7 cue of WP-AUDIO-13 (the boss melee handlers of `MoverActEvent.cpp`, called by `bindings/sim.ts`).

import { ProjectileKind } from '../gen/proto/flyff/v1/combat_pb';
import { II_WEA_HAN_HAND } from '../gen/defs/item';
import { IK3_YOYO } from '../gen/defs/itemkind';
import {
  MI_BIGMUSCLE,
  MI_CLOCKWORK1,
  MI_DU_METEONYKER,
  MI_DU_METEONYKER2,
  MI_DU_METEONYKER3,
  MI_DU_METEONYKER4,
  MI_KRRR,
  MI_LOADCLOCKWORK,
  MI_MUSHMOOT,
  XI_GEN_CURE01,
  XI_GEN_REF01,
  XI_GEN_WATERCROWN01,
} from '../gen/defs/obj';
import { PARTS_RWEAPON } from '../gen/defs/neuz';
import {
  BGM_IN_FITUP,
  SND_GEN_LEVELUP,
  SND_GEN_WATER,
  SND_INF_CLICK,
  SND_INF_GROUNDDROP,
  SND_INF_MESSENGERRING,
  SND_INF_TRADE,
  SND_PC_BOSS_CRASH,
  SND_PC_BOSS_STUN,
  SND_PC_BOSS_SUMMONS,
  SND_PC_SKILLD_HITOFPENYA,
  SND_PC_SKILLM_FIREBOOMERANG2,
  SND_PC_SKILLM_HOTAIR2,
  SND_PC_SKILLM_STRONGWIND2,
  SND_PC_SKILLM_SWORDWIND2,
} from '../gen/defs/sound';
import { EntityKind } from '../game/entities';
import { OBJSTA_ATK3, OBJSTA_ATK4 } from '../game/fx/bossIds';
import {
  type AudioData,
  type AudioFeatures,
  type AudioMover,
  type AudioStateReaders,
  isSoundId,
  NO_FEATURES,
  type Vec3,
} from './deps';
import type { BgmDirector } from './director';
import type { MusicPlayer } from './music';
import type { SfxPlayer, VoiceClass } from './sfx';
import type { MusicId, SoundId } from './tables';

export type ObjId = number;
export type SfxId = number;

/** `OBJSTA_DIE_ALL`: the target is dying or dead (`OnDamageMsgC` does not play for it). */
export const OBJSTA_DIE_ALL = 0x0800_0000;

/**
 * The movers whose melee hits have their own handlers (T11, `MoverActEvent.cpp:1020-1460`): T06 skips them. `MI_LOADCLOCKWORK` has one only with
 * `__BETTER_CLOCKWORK_ALPHA` (`features.better_clockwork_alpha`, `OnAttackMelee`): without the feature it is an ordinary monster and plays the generic hit sound.
 */
const SPECIAL_MELEE_MOVERS: ReadonlySet<number> = new Set([
  MI_CLOCKWORK1,
  MI_LOADCLOCKWORK,
  MI_BIGMUSCLE,
  MI_KRRR,
  MI_MUSHMOOT,
  MI_DU_METEONYKER,
  MI_DU_METEONYKER2,
  MI_DU_METEONYKER3,
  MI_DU_METEONYKER4,
]);

/** Impact sounds by projectile kind (44 §10.3; `m_dwSndHit` is never assigned in this build). */
export const IMPACT_SOUND: Readonly<Record<number, SoundId>> = {
  [ProjectileKind.MAG_STRONG_WIND]: SND_PC_SKILLM_STRONGWIND2,
  [ProjectileKind.MAG_SWORD_WIND]: SND_PC_SKILLM_SWORDWIND2,
  [ProjectileKind.MAG_FIRE_BOOMERANG]: SND_PC_SKILLM_FIREBOOMERANG2,
  [ProjectileKind.MAG_ICE_MISSILE]: SND_PC_SKILLM_FIREBOOMERANG2,
  [ProjectileKind.MAG_LIGHTNING_BALL]: SND_PC_SKILLM_FIREBOOMERANG2,
  [ProjectileKind.MAG_SPIKE_STONE]: SND_PC_SKILLM_FIREBOOMERANG2,
};

/** The literal files of the legacy UI sounds (§9). */
const FILE_SMELT_START = 'PcSkillD-Counter01.wav';
const FILE_UPGRADE_SUCCESS = 'InfUpgradeSuccess.wav';
const FILE_UPGRADE_FAIL = 'InfUpgradeFail.wav';
const FILE_MINIGAME_TICK = 'InfOpen.wav';

export interface AudioCues {
  // generic (thin wrappers)
  sound(id: SoundId, pos?: Vec3, cls?: VoiceClass): void;
  soundFile(file: string, pos?: Vec3, cls?: VoiceClass): void;
  /** Jingles: `loops` defaults to 1. */
  music(id: MusicId, loops?: number): void;
  // engine-level cues (the §9 rules in one place)
  /** T41/T42: the SFX factory created effect `xi` at `posSrc`. */
  sfxCreated(xi: SfxId, posSrc?: Vec3): void;
  /** Reserved: an animation sound outside the sim's decor set (none in v1). */
  objectAttrSound(sound: SoundId, pos?: Vec3): void;
  /** T06 (ground) / T10 (flying): a melee hit frame of `attacker` on `target`. */
  meleeHit(attacker: ObjId, target: ObjId, kind: 'ground' | 'fly'): void;
  /**
   * T11 (M7, WP-AUDIO-13): the sounds of the boss melee handlers (BigMuscle, Krrr, Mushmoot; the ClockWorks and the Meteonykers play nothing here). `atkState` is the
   * `OBJSTA_ATK*` word and `hitCount` the swing's `m_nMotionHitCount` before the handler's increment.
   */
  bossMeleeSpecial(attacker: ObjId, target: ObjId, atkState: number, hitCount: number): void;
  /** T09 (M5, WP-AUDIO-11): the skill runtime created the effect of skill `skillId` (or the shot of a shot skill) at `caster`. */
  skillEffectFired(caster: ObjId, skillId: number): void;
  /** T08: a server-simulated projectile reached its target. */
  projectileImpact(p: ProjectileImpact): void;
  /**
   * T08h (M5, WP-AUDIO-11): a pain tick of the HotAir effect at effect frames 0, P, 2P, ...: the pain sound at `pos`, and on the
   * `first` tick the target's damage sound of T08 (a).
   */
  hotAirPainTick(pos: Vec3, first: boolean, attacker: ObjId, target: ObjId): void;
  /** T40: the local player starts an attack (the combat music of §11.6). */
  localAttackStarted(): void;
  /** T26: a mover other than the local player was clicked in the world. */
  moverClicked(obj: ObjId): void;
  /** T25: a Button-type control was pressed (also `click`, the name `UiHost.sound` calls). */
  uiClick(): void;
  click(): void;
  /** T23: an inventory item was dropped on the world view. */
  groundDrop(): void;
  /** T24: the shop buy / sell confirmation. */
  shopConfirm(): void;
  /** T30: the smelt-jewel / change-weapon windows. */
  upgradeResult(kind: 'smeltStart' | 'success' | 'fail'): void;
  /** T37: a line was added to an instant-message popup. */
  instantMessage(): void;
  /** T27: the NPC dialog opened (`npcKey` of the `npcs` bundle). */
  dialogOpened(npcKey: string): void;
  /** T39: a mini-game spin step or a win. */
  miniGame(e: 'tick' | 'win'): void;
}

export interface ProjectileImpact {
  /** `ProjectileKind` of the spawn (`UNSPECIFIED` when the spawn was not seen). */
  readonly kind: number;
  readonly attacker: ObjId;
  readonly target: ObjId;
  readonly pos: Vec3;
  /** `ProjectileSpawn.hit_sound_id` (0 = use the table). */
  readonly hitSoundId: SoundId;
}

export interface CuesDeps {
  readonly sfx: Pick<SfxPlayer, 'playId' | 'playFile'>;
  readonly music: Pick<MusicPlayer, 'playMusicId'>;
  readonly director: Pick<BgmDirector, 'combatMusic'>;
  readonly data: () => AudioData | null;
  readonly state: AudioStateReaders;
  readonly features?: AudioFeatures;
}

/** The active-hand item of a mover (`CMover::GetActiveHandItemProp`, `src:_Common/Mover.cpp:2994-3030`). */
export function activeHandItemId(
  mover: AudioMover,
  data: AudioData,
): { readonly itemId: number } | null {
  if (mover.kind === EntityKind.Player) {
    const worn = mover.visual?.equip.get(PARTS_RWEAPON)?.itemId;
    return { itemId: worn !== undefined && worn > 0 ? worn : II_WEA_HAN_HAND };
  }
  const prop = data.mover(mover.propId);
  return prop === undefined ? null : { itemId: prop.atk1 };
}

export function createCues(deps: CuesDeps): AudioCues {
  const { sfx, music, state } = deps;
  const features = deps.features ?? NO_FEATURES;
  const tmp: [number, number, number] = [0, 0, 0];

  /** The LH position of a mover as a fresh tuple (a position is kept by the sound until its buffer arrives). */
  const posOf = (id: ObjId): Vec3 | undefined =>
    state.posOf(id, tmp) ? [tmp[0], tmp[1], tmp[2]] : undefined;

  const activeHandItem = (mover: AudioMover) => {
    const data = deps.data();
    if (data === null) return undefined;
    const found = activeHandItemId(mover, data);
    return found === null ? undefined : data.item(found.itemId);
  };

  /** `CSfx::DamageToTarget`: the target's `snd_dmg[1]` at the target (else at `fallback`) when it is a mover in view and the attacker exists. */
  const damageToTarget = (attackerId: ObjId, targetId: ObjId, fallback: Vec3): void => {
    const target = state.mover(targetId);
    const attacker = state.mover(attackerId);
    if (target === undefined || target.removed || attacker === undefined || attacker.removed)
      return;
    const dmg = deps.data()?.mover(target.propId)?.snd_dmg[1];
    if (isSoundId(dmg)) sfx.playId(dmg, posOf(targetId) ?? fallback);
  };

  const cues: AudioCues = {
    sound: (id, pos, cls) => {
      sfx.playId(id, pos, cls);
    },
    soundFile: (file, pos, cls) => {
      sfx.playFile(file, pos, cls);
    },
    music: (id, loops = 1) => {
      music.playMusicId(id, loops);
    },

    sfxCreated: (xi, posSrc) => {
      if (!(xi > 0)) return; // xi == 0 creates nothing and plays nothing
      // T42: the special creation sounds, before the table sound (CreateObj.cpp:82, 180-181)
      if (xi === XI_GEN_WATERCROWN01) sfx.playId(SND_GEN_WATER, posSrc);
      else if (xi === XI_GEN_CURE01 || xi === XI_GEN_REF01) sfx.playId(SND_GEN_LEVELUP, posSrc);
      // T41: the `sound` column of the sfx table plays whether or not the `.sfx` file exists and whatever the SFX options say
      const sound = deps.data()?.sfx(xi)?.sound;
      if (typeof sound === 'string' && sound !== '') sfx.playFile(sound, posSrc);
    },

    objectAttrSound: (sound, pos) => {
      sfx.playId(sound, pos);
    },

    meleeHit: (attackerId, targetId, kind) => {
      const attacker = state.mover(attackerId);
      const target = state.mover(targetId);
      // the client "damage succeeded" test: both exist and the target is not dying or dead (OnDamageMsgC)
      if (attacker === undefined || attacker.removed) return;
      if (target === undefined || target.removed || (target.state & OBJSTA_DIE_ALL) !== 0) return;
      const item = activeHandItem(attacker);
      if (kind === 'ground') {
        // T06 exclusions: the movers with their own handlers (T11), and a player's yo-yo auto-attack while the
        // splash path is on (the splash path returns "no damage" on the client, KQ-44)
        if (
          attacker.kind !== EntityKind.Player &&
          SPECIAL_MELEE_MOVERS.has(attacker.propId) &&
          (attacker.propId !== MI_LOADCLOCKWORK || features.betterClockworkAlpha())
        ) {
          return;
        }
        if (
          attacker.kind === EntityKind.Player &&
          features.splashAutoattacks() &&
          item?.item_kind3 === IK3_YOYO
        ) {
          return;
        }
      }
      if (item === undefined || !isSoundId(item.snd_attack1)) return;
      sfx.playId(item.snd_attack1, posOf(attackerId));
    },

    // T11 (`OnAttackMelee_BigMuscle / Krrr / Bear`, `PLAYSND` under `__CLIENT`): all non-positional except the Krrr crash, which plays at the hit target (`&pHitObj->GetPos()`; the
    // BigMuscle's `&v2` is commented out in the original: KEEP). The handler runs whatever the damage did, so only the attacker has to be a boss in view.
    bossMeleeSpecial: (attackerId, targetId, atkState, hitCount) => {
      const attacker = state.mover(attackerId);
      if (attacker === undefined || attacker.removed || attacker.kind === EntityKind.Player) return;
      switch (attacker.propId) {
        case MI_BIGMUSCLE:
          if (atkState === OBJSTA_ATK3) {
            sfx.playId(SND_PC_SKILLD_HITOFPENYA);
            if (hitCount !== 1) sfx.playId(SND_PC_BOSS_STUN);
          } else if (atkState === OBJSTA_ATK4 && hitCount === 1) {
            sfx.playId(SND_PC_SKILLD_HITOFPENYA);
          }
          break;
        case MI_KRRR:
          if (atkState === OBJSTA_ATK3) {
            // a target the view does not know is out of earshot: nothing rather than a global crash
            const at = posOf(targetId);
            if (at !== undefined) sfx.playId(SND_PC_BOSS_CRASH, at);
          } else if (atkState === OBJSTA_ATK4) {
            if (hitCount === 0) sfx.playId(SND_PC_BOSS_SUMMONS);
            else if (hitCount === 1) sfx.playId(SND_PC_BOSS_STUN);
          }
          break;
        case MI_MUSHMOOT:
          if (atkState === OBJSTA_ATK3) {
            if (hitCount === 0) sfx.playId(SND_PC_SKILLD_HITOFPENYA);
            else if (hitCount === 1) sfx.playId(SND_PC_BOSS_STUN);
          }
          break;
        default:
          break; // the ClockWorks and the Meteonykers have no sound in their handlers
      }
    },

    // T09: `CCtrl::CreateSkillSfx` / `ShootSkill` made an effect: the skill's own sound at the caster (`PLAYSND(dwSndAttack1, &pos)`)
    skillEffectFired: (caster, skillId) => {
      const sound = deps.data()?.skill?.(skillId)?.snd_attack1;
      if (isSoundId(sound)) sfx.playId(sound, posOf(caster));
    },

    // T08h: the pain tick of the HotAir effect (`Sfx.cpp:2795-2810`); (a) of T08 only on the first tick
    hotAirPainTick: (pos, first, attacker, target) => {
      if (first) damageToTarget(attacker, target, pos);
      sfx.playId(SND_PC_SKILLM_HOTAIR2, pos);
    },

    projectileImpact: (p) => {
      // (a) the target is a mover in view and the attacker still exists: the target's damage sound at the target (CSfx::DamageToTarget)
      damageToTarget(p.attacker, p.target, p.pos);
      // (b) the impact sound: the one the server named, else the table of the kind
      const id = isSoundId(p.hitSoundId) ? p.hitSoundId : IMPACT_SOUND[p.kind];
      if (id !== undefined && id > 0) sfx.playId(id, p.pos);
    },

    localAttackStarted: () => {
      deps.director.combatMusic();
    },

    moverClicked: (id) => {
      const mover = state.mover(id);
      if (mover === undefined || mover.removed || mover.isSelf) return;
      const idle = deps.data()?.mover(mover.propId)?.snd_idle[0];
      if (isSoundId(idle)) sfx.playId(idle); // PLAYSND(dwSndIdle1): non-positional
    },

    uiClick: () => {
      sfx.playId(SND_INF_CLICK, undefined, 'ui');
    },
    click: () => {
      sfx.playId(SND_INF_CLICK, undefined, 'ui');
    },
    groundDrop: () => {
      sfx.playId(SND_INF_GROUNDDROP, undefined, 'ui');
    },
    shopConfirm: () => {
      sfx.playId(SND_INF_TRADE, undefined, 'ui');
    },
    upgradeResult: (kind) => {
      const file =
        kind === 'smeltStart'
          ? FILE_SMELT_START
          : kind === 'success'
            ? FILE_UPGRADE_SUCCESS
            : FILE_UPGRADE_FAIL;
      sfx.playFile(file, undefined, 'ui');
    },
    instantMessage: () => {
      sfx.playId(SND_INF_MESSENGERRING, undefined, 'ui');
    },
    dialogOpened: (npcKey) => {
      const m = deps.data()?.npc(npcKey)?.music;
      if (m !== undefined && m > 0) music.playMusicId(m, 1); // 0 = no dialog music
    },
    miniGame: (e) => {
      if (e === 'tick') sfx.playFile(FILE_MINIGAME_TICK, undefined, 'ui');
      else music.playMusicId(BGM_IN_FITUP, 1);
    },
  };
  return cues;
}

export const NULL_CUES: AudioCues = {
  sound: () => undefined,
  soundFile: () => undefined,
  music: () => undefined,
  sfxCreated: () => undefined,
  objectAttrSound: () => undefined,
  meleeHit: () => undefined,
  bossMeleeSpecial: () => undefined,
  skillEffectFired: () => undefined,
  projectileImpact: () => undefined,
  hotAirPainTick: () => undefined,
  localAttackStarted: () => undefined,
  moverClicked: () => undefined,
  uiClick: () => undefined,
  click: () => undefined,
  groundDrop: () => undefined,
  shopConfirm: () => undefined,
  upgradeResult: () => undefined,
  instantMessage: () => undefined,
  dialogOpened: () => undefined,
  miniGame: () => undefined,
};
