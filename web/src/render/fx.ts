// `RenderFx`: the effect requests of the feature handlers of docs 17/43/45 (Plan/41 section 2.2). The base renderer
// only counts the requests; WP-REND-18/20/21/24 and 14 implement the visuals behind the same interface.

import type { ObjId, Vec3LH } from './types';

/** Opaque handle of a created SFX instance. */
export type SfxHandle = number;

export type SfxClass = 'normal' | 'skill' | 'hit' | 'ui';

export interface SfxOptions {
  pos?: Vec3LH;
  src?: ObjId;
  dest?: ObjId;
  posDest?: Vec3LH;
  nSec?: number;
  angle?: number;
  /** `m_vRotate` of the instance in degrees `(x = pitch, y = yaw, z = roll)`: the rain splashes tilt to the terrain (Plan/41 15). */
  rotateDeg?: Vec3LH;
  scale?: number;
  directional?: boolean;
  cls?: SfxClass;
  /**
   * The hand of `src` the effect is linked to (`CSfxPartsLink::m_nPartsLink`, Plan/41 17.2): 0 = the right hand, 1 = the left hand. The
   * class of the effect id decides the rest (`render/sfx/skillClasses.ts`).
   */
  partLink?: 0 | 1;
  /** `CSfxDuelParty::m_nType`: why the flag stands (`ENEMY_PVP_PARTY` 0, `ENEMY_WAR` 1, `ENEMY_PK` 2, `ENEMY_PVP` 3). */
  duelKind?: 0 | 1 | 2 | 3;
}

export interface RenderFx {
  /** `CreateSfx` factory (Plan/41 section 17.2). */
  sfx(xi: number, o: SfxOptions): SfxHandle;
  removeSfx(h: SfxHandle): void;
  /** `RemoveSfxObj` semantics (section 17.1). */
  markRemoveSfx(anchor: ObjId, xi: number): void;
  /** Section 19.5 (the display gate is decided by doc 17). */
  damageNumber(posLH: Vec3LH, value: number, style: 0 | 1): void;
  /** `dmgCnt = 10` (section 12.8). */
  hitShake(id: ObjId): void;
  /** `target`: the homing target (the wand dots of a flying caster are red when it is the own character). */
  projectileVisual(
    projectileId: number,
    sfxId: number,
    kind: number,
    target?: ObjId,
    /** The attacker of the spawn (render-audio-m7: the steamwalker's jet hangs on it). */
    attacker?: ObjId,
  ): void;
  projectileEnd(projectileId: number, at: Vec3LH | null, hitSfx: number): void;
  /**
   * The sparks of a melee hit (ActionMoverState.cpp:957-1030, Plan/41 18.2): five ticks of one particle each at the attacker, for the
   * players near the own character. `option` is the upgrade level of the weapon (`nOption & 0xFF`). Optional: a renderer without
   * the particle manager does not have it.
   */
  meleeSparks?(attacker: ObjId, option: number): void;
  /**
   * `SpawnItem` outside the join batch (Plan/41 11.9): the item DROPPED, it bounces when it lands near the ground; the items of the
   * join batch do not. Optional: a renderer without ground items does not have it.
   */
  itemDropped?(id: ObjId): void;
  /**
   * The flying yo-yos of a yo-yo skill (`CCtrl::CreateYoyoSkill`, Plan/41 17.4; `CSfxItemYoyoAtk`): `src` throws the yo-yo of each of
   * `hands` (0 = right, 1 = left) at `target`; `sfxId` is the effect of the equipped yo-yo (`sfx_obj[1]`). SLOWSTEP, HITOFPENYA and
   * VATALSTAB throw one from the right hand, DEADLYSWING and CROSSLINE one from each hand. Optional: a renderer without skill effects
   * does not have it.
   */
  skillYoyo?(src: ObjId, target: ObjId, sfxId: number, hands: readonly (0 | 1)[]): void;
  /**
   * The Hot Air effect of a `MAG_HOT_AIR` projectile (`CSfxSkillMagHotAir`, Plan/41 17.4): two effects for `skillMs` and the pain-tick
   * timer that calls the audio cue every `painMs`. It takes the place of `projectileVisual` for that kind. Optional.
   */
  hotAir?(o: {
    projectileId: number;
    sfxId: number;
    attacker: ObjId;
    target: ObjId;
    painMs: number;
    skillMs: number;
  }): void;
  /**
   * `CommonPlace` of a ctrl (Plan/41 11.8, 17.2): `kind` 0 action (the effect of `CtrlProp.dwSfxCtrl`), 1 alpha (the fade), 2 quake.
   * Optional: a renderer without ctrls does not have it.
   */
  ctrlPlace?(id: ObjId, kind: number): void;
  // ---- render-audio-m7 (WP-REND-27): all optional, like the other M5+ members
  /**
   * One melee hit of a boss swing (`AttrHit{Melee}` of `MI_CLOCKWORK1` / `MI_LOADCLOCKWORK` / `MI_BIGMUSCLE` / `MI_KRRR` / `MI_MUSHMOOT` / `MI_DU_METEONYKER*`): `state` is the `OBJSTA_ATK*` word and `hitCount`
   * the swing's `m_nMotionHitCount` before the handler's increment (`render/sfx/bossFx.ts`).
   */
  bossMeleeHit?(attacker: ObjId, target: ObjId, state: number, hitCount: number): void;
  /** `BossChargeFx{state}`: a boss starts to charge (ClockWorks ATK3 also arms the laser). */
  bossCharge?(id: ObjId, state: number): void;
  /**
   * The balloon of a player (`IK3_BALLOON` buff, Plan/22 17.2): `itemId` is the item whose model floats beside the player, `null` removes it
   * (`render/movers/eventBalloon.ts`).
   */
  eventBalloon?(playerId: ObjId, itemId: number | null): void;
  /**
   * The wallpaper and carpet of the room the player stands in (`CHousing` / `CGuildHouse` papering, `render/objects/papering.ts`): `wall` / `floor` are `text_file` names under
   * `Model/Texture` (`null` = the default of the room), `world` 0 = left the room, restore everything.
   */
  roomPapering?(o: { world: number; wall: string | null; floor: string | null }): void;
  /** Section 19.6 (requested by 45 section 8.5). */
  balloon(id: ObjId, b: { text: string; kind: 0 | 1 | 2 | 3 } | null): void;
  emoticon(id: ObjId, set: 'chat' | 'dialog', index: number): void;
  vendorTitle(id: ObjId, title: string | null): void;
  /** `XI_GEN_MOVEMARK01` singleton (43 section 5.5). */
  moveMark(posLH: Vec3LH, normalLH: Vec3LH | null): void;
}

/** What the stub implementation recorded. */
export interface FxCounters {
  sfx: number;
  removeSfx: number;
  markRemoveSfx: number;
  damageNumber: number;
  hitShake: number;
  projectileVisual: number;
  projectileEnd: number;
  balloon: number;
  emoticon: number;
  vendorTitle: number;
  moveMark: number;
}

/** The base-renderer implementation: counts every call, draws nothing (later WPs replace the pieces). */
export class StubRenderFx implements RenderFx {
  readonly counters: FxCounters = {
    sfx: 0,
    removeSfx: 0,
    markRemoveSfx: 0,
    damageNumber: 0,
    hitShake: 0,
    projectileVisual: 0,
    projectileEnd: 0,
    balloon: 0,
    emoticon: 0,
    vendorTitle: 0,
    moveMark: 0,
  };
  private next = 1;

  sfx(): SfxHandle {
    this.counters.sfx++;
    return this.next++;
  }
  removeSfx(): void {
    this.counters.removeSfx++;
  }
  markRemoveSfx(): void {
    this.counters.markRemoveSfx++;
  }
  damageNumber(): void {
    this.counters.damageNumber++;
  }
  hitShake(): void {
    this.counters.hitShake++;
  }
  projectileVisual(): void {
    this.counters.projectileVisual++;
  }
  projectileEnd(): void {
    this.counters.projectileEnd++;
  }
  balloon(): void {
    this.counters.balloon++;
  }
  emoticon(): void {
    this.counters.emoticon++;
  }
  vendorTitle(): void {
    this.counters.vendorTitle++;
  }
  moveMark(): void {
    this.counters.moveMark++;
  }
}
