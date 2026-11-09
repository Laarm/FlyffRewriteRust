// The data a behaviour class of the `CreateSfx` factory is made of (Plan/41 section 17.2): where the effect stands, whether it turns with its
// object, how long it lives, and - for the classes that do more than that (`render/sfx/skillBehaviors.ts`) - the hooks `SfxSystem` calls.
// Split out of `classes.ts` so that the skill classes (`skillClasses.ts`) and the generic ones can both build specs without importing each
// other.

import type { SfxHandle, SfxOptions } from '../fx';
import type { SfxInstance, WorldQuery } from './SfxSystem';

export type FollowKind = 'base' | 'src' | 'fixed' | 'none';

export interface ClassSpec {
  readonly follow: FollowKind;
  /** Added to the position's y when drawn. */
  readonly lift: number;
  /** The model's yaw is the object angle (false: 0). */
  readonly yaw: boolean;
  /** Drawn through the 3-axis path (`Render2`) with the instance's `rot3`. */
  readonly render2: boolean;
  /** Plays once whatever `nSec` says. */
  readonly once: boolean;
  /** A new one replaces the old (the move mark). */
  readonly singleton: boolean;
  /** The server's `RemoveSfxObj` of the anchor ends it (`XI_SKILL_PSY_HERO_STONE02`, ...). */
  readonly removable: boolean;
  /** The classes with a `Process` / `Render` of their own (skill specials, hand links, rain, ...). */
  readonly behavior?: SfxBehavior;
}

export const BASE_SPEC: ClassSpec = {
  follow: 'base',
  lift: 0.2,
  yaw: true,
  render2: false,
  once: false,
  singleton: false,
  removable: false,
};

/** What a behaviour may do besides reading its instance: ask the world, make other effects, draw random numbers. */
export interface BehaviorHost {
  readonly world: WorldQuery;
  /** `CreateSfx` for a child effect. */
  create(xi: number, o: SfxOptions): SfxHandle;
  /** The effect of `handle` still exists. */
  alive(handle: SfxHandle): boolean;
  /** `xRandomF(1)`: uniform in [0, 1] (`rand() / RAND_MAX`). */
  rand(): number;
  readonly hooks: SkillFxHooks;
}

/** What the effect classes ask of the game (`app/renderHost.ts` supplies it; every member is optional). */
export interface SkillFxHooks {
  /** `prj.GetPartySkill(ST_STRETCHING)->dwSfxObj2`: the lasting effect the stretching cast leaves when it ends. */
  stretchingSfx?(): number | undefined;
  /** `g_Party.m_nModeTime[mode] != FALSE` (the troupe modes: `PARTY_STRETCHING_MODE` = 2). Unknown: on. */
  partyModeOn?(mode: number): boolean;
  /** The duel / war / PK that put the flag over `mover` is over (`CSfxDuelParty::Process`: `kind` is `ENEMY_*`). */
  duelEnded?(mover: number, kind: number): boolean;
  /** `HasBuff(SI_ACR_SUP_DARKILLUSION) || IsMode(TRANSPARENT_MODE)`: the duel flag is not drawn. */
  flagHidden?(mover: number): boolean;
  /**
   * Called by the render-internal creations (the boss effects, WP-REND-27) BEFORE the effect is made: `withAudioFx` wraps only `RenderFx.sfx`, so these would be silent otherwise
   * (44 T41: the `sound` column of the `sfx-table` plays whether or not the file exists). `posSrc` is the position the sound plays at.
   */
  sfxCreated?(xi: number, posSrc?: readonly [number, number, number]): void;
  /** `CCamera::SetQuake( frames, size )` of a boss attack; the host calls the input's camera quake only while `features.no_quake === false` (PL-134, F-333). */
  quake?(frames: number, size: number): void;
  /** `GetSpeed( m_fSpeed )` of a mover (`rules.moveSpeed`): the event balloon's follow speed. */
  speedOf?(mover: number): number | undefined;
  /** The client has the skill row `(skillId, level)` (the Meteonyker's burn effect needs `SI_GEN_BURN` lv 1: `GetSkillProp` fails without it). Unknown: yes. */
  hasSkillRow?(skillId: number, level: number): boolean;
  /** `hotAirPainTick(pos, first, attacker, target)` of the audio cues (44 T08h). */
  hotAirPainTick?(
    pos: readonly [number, number, number],
    first: boolean,
    attacker: number,
    target: number,
  ): void;
}

/** `Process` / `Render` of a class that is not the base `CSfx`. */
export interface SfxBehavior {
  /** After the factory built the instance (post-creation fixes, the first-frame placement). */
  init?(inst: SfxInstance, host: BehaviorHost, o: SfxOptions): void;
  /** One 60 Hz step, before the model step (`inst.frame` counts the steps run before this one). */
  tick?(inst: SfxInstance, host: BehaviorHost): 'end' | undefined;
  /** The per-frame placement (the part of `Render` that sets the model's position / yaw); `false`: the effect is not drawn this frame. */
  place?(inst: SfxInstance, host: BehaviorHost): boolean;
  /** The model finished and the class (without `ownLifetime`) is going to die with it: the last words of the class. */
  finish?(inst: SfxInstance, host: BehaviorHost): void;
  /** The model is the class's own business (the rain spawners have none to draw): the instance needs no `.sfx` to run. */
  readonly modelless?: boolean;
  /**
   * The class ends itself: the system steps the model and asks `afterModel` what to do with the result (`end`, `restart` the model,
   * or `spent`: the instance stays but is no longer drawn). Without it the base `CSfx` lifetime rules apply.
   */
  readonly ownLifetime?: boolean;
  afterModel?(
    inst: SfxInstance,
    host: BehaviorHost,
    finished: boolean,
  ): 'end' | 'restart' | 'spent' | undefined;
}
