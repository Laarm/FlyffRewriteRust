// The `Process` / `Render` of the skill effect classes that are more than a placement rule (Plan/41 section 17.2, 17.5; WP-REND-24;
// Sfx.cpp:225-385, 1533-1760, 2172-2330, 2421-2470, 2833-2910, 3412-3470, 3750-3870). `SfxSystem` calls the hooks:
//
//   init       after the factory built the instance
//   tick       one 60 Hz step before the model step; `inst.frame` is the number of steps run before this one
//   place      the per-frame position / yaw; `false` = not drawn this frame
//   afterModel after the model step, for the classes that end themselves (`ownLifetime`)
//   finish     the model of a once-class finished and the instance is going to die
//
// All positions are LEFT-HANDED game space. The classes read the world through `WorldQuery` (positions, yaw, the OBJSTA word, hand and
// event points of the render view) and the game through `SkillFxHooks`; nothing here touches three.js.

import {
  XI_NAT_WINGANGEL01,
  XI_SKILL_CIRCLE_DUST,
  XI_SKILL_DROP_DUST,
  XI_SKILL_MER_ONE_SPLMASH02,
  XI_SKILL_RAG_BOW_ARROWRAIN,
  XI_SKILL_RAG_BOW_ARROWRAIN01,
} from '../../gen/defs/obj';
import type { BehaviorHost, SfxBehavior } from './classSpec';
import type { SfxInstance, V3 } from './SfxSystem';

/** The child effects the skill classes create. */
export const CHILD_XI = {
  splmash2: XI_SKILL_MER_ONE_SPLMASH02,
  arrowRain: XI_SKILL_RAG_BOW_ARROWRAIN,
  arrowRainHit: XI_SKILL_RAG_BOW_ARROWRAIN01,
  stoneDust: XI_SKILL_DROP_DUST,
  circleDust: XI_SKILL_CIRCLE_DUST,
  wingAngel: XI_NAT_WINGANGEL01,
} as const;

/** `OBJSTA_ATK_ALL` and `OBJSTA_ATK_MELEESKILL` (flyff-sim `types`). */
export const OBJSTA_ATK_ALL = 0x00ff_0000;
export const OBJSTA_ATK_MELEESKILL = 0x0009_0000;
/** `OBJSTA_MOVE_ALL` and `OBJSTA_SIT`. */
const OBJSTA_MOVE_ALL = 0x0000_00ff;
const OBJSTA_SIT = 0x0000_0003;

/** `ENEMY_*` of `CSfxDuelParty::m_nType`. */
export const ENEMY = { PVP_PARTY: 0, WAR: 1, PK: 2, PVP: 3 } as const;
/** `PARTY_STRETCHING_MODE`. */
export const PARTY_STRETCHING_MODE = 2;

/** `m_nFrame` of `CSfxSkillMerSplmash` after which the second model plays (`m_nFrame > 85`). */
export const SPLMASH_SECOND_AFTER = 85;
/** The scale `CSfxSkillAssBurstcrack` forces (`D3DXVECTOR3(0.6, 0.6, 0.6)`). */
export const BURSTCRACK_SCALE = 0.6;
/** `MAX_ARROW` / `MAX_STONE`: the iterations of a rain spawner. */
export const RAIN_ITERATIONS = 50;
/** `m_nFrame >= SEC1 * 0.07f` with an integer `m_nFrame` and a float threshold: the count that first passes is 5 (see NOTES.md). */
export const ARROW_RAIN_PERIOD = Math.fround(60 * Math.fround(0.07));
/** `m_nFrame >= SEC1 * 0.2f`. */
export const STONE_RAIN_PERIOD = Math.fround(60 * Math.fround(0.2));
/** The ticks a blade-link effect waits for the caster to enter the melee-skill state before it gives up (the sim answers a frame later). */
export const BLADE_ARM_GRACE_TICKS = 60;
/** The ticks a source the render view has never shown may stay unknown before the effect gives up. */
export const UNSEEN_GRACE_TICKS = 30;
/** A falling rain drop that never finds the ground (no height source) ends after this many ticks. */
export const DROP_MAX_TICKS = 300;

const tmp: V3 = [0, 0, 0];

function setPos(inst: SfxInstance, p: ArrayLike<number>): void {
  inst.pos[0] = p[0] ?? 0;
  inst.pos[1] = p[1] ?? 0;
  inst.pos[2] = p[2] ?? 0;
}

/** The position of the hand an effect is linked to; the body centre when the render view has no attachment record. */
function handOf(inst: SfxInstance, host: BehaviorHost, out: V3): boolean {
  if (host.world.handPos?.(inst.idSrc, inst.partLink, out) === true) return true;
  if (!host.world.posOf(inst.idSrc, out)) return false;
  out[1] += 1;
  return true;
}

function srcYaw(inst: SfxInstance, host: BehaviorHost): number {
  return inst.idSrc !== 0 ? host.world.angleOf(inst.idSrc) : Number.NaN;
}

/**
 * `IsValidObj(pObjSrc)`. The render view may not know a caster for a frame or two after the effect was requested (the first tick of an
 * effect can run before the first frame that sees its mover), so a source that was never seen is given {@link UNSEEN_GRACE_TICKS}.
 */
function srcAlive(inst: SfxInstance, host: BehaviorHost): boolean {
  if (inst.idSrc === 0) return false;
  if (host.world.posOf(inst.idSrc, tmp)) {
    inst.bh.seen = 1;
    return true;
  }
  return inst.bh.seen === 0 && inst.frame < UNSEEN_GRACE_TICKS;
}

// ---- casting, specials ---------------------------------------------------------------------------------------------------------

/** `CSfxSkillMagFireCasting::Render`: drawn only while the caster exists, a metre up, turned against the caster. */
const castFire: SfxBehavior = {
  place(inst, host) {
    const a = srcYaw(inst, host);
    if (Number.isNaN(a)) return false;
    inst.angle = 180 - a;
    return true;
  },
};

/**
 * `CSfxSkillMerSplmash`: at its first step it moves onto the caster and turns `180 - angle`; after step 85 a second model
 * (`XI_SKILL_MER_ONE_SPLMASH02`) plays at the same place and the effect ends with it.
 */
const splmash: SfxBehavior = {
  ownLifetime: true,
  tick(inst, host) {
    // the first step with a known caster (`m_nFrame == 0 && pObjSrc`): the effect stands where the caster stands
    if (!inst.bh.flag && inst.idSrc !== 0 && host.world.posOf(inst.idSrc, tmp)) {
      inst.bh.flag = true;
      setPos(inst, tmp);
      inst.angle = 180 - host.world.angleOf(inst.idSrc);
    }
    if (inst.frame > SPLMASH_SECOND_AFTER) {
      if (inst.bh.child === 0) {
        // `SetPos` of the original is not lifted by the 0.01 the factory adds
        inst.bh.child = host.create(CHILD_XI.splmash2, {
          pos: [inst.pos[0], inst.pos[1] - 0.01, inst.pos[2]],
          angle: inst.angle,
          scale: inst.scale[0],
          cls: 'skill',
        });
      } else if (!host.alive(inst.bh.child)) {
        return 'end';
      }
    }
    return undefined;
  },
  afterModel: (_inst, _host, finished) => (finished ? 'spent' : undefined),
};

/** `CSfxSkillAssBurstcrack`: 0.1 above the spot, turned `180 - angle`, scale 0.6; gone with the caster. */
const burstcrack: SfxBehavior = {
  tick(inst, host) {
    return srcAlive(inst, host) ? undefined : 'end';
  },
  place(inst, host) {
    const a = srcYaw(inst, host);
    if (Number.isNaN(a)) return false;
    inst.angle = 180 - a;
    inst.scale[0] = BURSTCRACK_SCALE;
    inst.scale[1] = BURSTCRACK_SCALE;
    inst.scale[2] = BURSTCRACK_SCALE;
    return true;
  },
};

/** `CSfxSkillAssTampinghole`: a metre up and a metre ahead of the spot along the caster's `180 - angle`; gone with the caster. */
const tampinghole: SfxBehavior = {
  tick(inst, host) {
    return srcAlive(inst, host) ? undefined : 'end';
  },
  place(inst, host) {
    const a = srcYaw(inst, host);
    if (Number.isNaN(a)) return false;
    const theta = 180 - a;
    const r = (theta * Math.PI) / 180;
    inst.angle = theta;
    inst.pos[0] = inst.origin[0] + Math.sin(r);
    inst.pos[1] = inst.origin[1] + 1;
    inst.pos[2] = inst.origin[2] + Math.cos(r);
    return true;
  },
};

// ---- hand links -----------------------------------------------------------------------------------------------------------------

function placeOnHand(inst: SfxInstance, host: BehaviorHost): boolean {
  if (!handOf(inst, host, tmp)) return false;
  setPos(inst, tmp);
  return true;
}

/** `CSfxAllowPartsLink` (bow and support casts): follows the hand, gone with the caster or when its model ends. */
const handLink: SfxBehavior = {
  tick(inst, host) {
    return srcAlive(inst, host) ? undefined : 'end';
  },
  place: placeOnHand,
};

/**
 * `CSfxPartsLinkBlade` (the Blade and, by the missing `break`, the Jester ids): follows the hand; ends when the caster leaves the
 * melee-skill state. The sim answers a frame after the cast started, so the end test only counts once the state was seen (or after
 * {@link BLADE_ARM_GRACE_TICKS}). A finished model stays unseen until then.
 */
const bladeLink: SfxBehavior = {
  ownLifetime: true,
  tick(inst, host) {
    if (!srcAlive(inst, host)) return 'end';
    const state = host.world.stateOf?.(inst.idSrc);
    if (state === undefined) return inst.frame >= BLADE_ARM_GRACE_TICKS ? 'end' : undefined;
    if ((state & OBJSTA_ATK_ALL) === OBJSTA_ATK_MELEESKILL) {
      inst.bh.armed = true;
    } else if (inst.bh.armed || inst.frame >= BLADE_ARM_GRACE_TICKS) {
      return 'end';
    }
    return undefined;
  },
  place: placeOnHand,
  afterModel: (_inst, _host, finished) => (finished ? 'spent' : undefined),
};

// ---- wings, troupe, duel ------------------------------------------------------------------------------------------------------

/** `CSfxRotate` (`XI_NAT_WINGANGEL01`): at the back event point of the caster, turned `-angle`, looping for `nSec` seconds (-1 forever). */
const wingAngel: SfxBehavior = {
  ownLifetime: true,
  tick(inst, host) {
    if (!srcAlive(inst, host)) return 'end';
    // `++m_nFrame` first, then `m_nFrame > m_nSec * 60` (so `nSec = 0` ends at the first step)
    if (inst.nSec !== -1 && inst.frame + 1 > inst.nSec * 60) return 'end';
    return undefined;
  },
  place(inst, host) {
    const a = srcYaw(inst, host);
    if (Number.isNaN(a)) return false;
    if (host.world.eventPos?.(inst.idSrc, 0, tmp) !== true) {
      if (!host.world.posOf(inst.idSrc, tmp)) return false;
      tmp[1] += 1;
    }
    setPos(inst, tmp);
    inst.angle = -a;
    return true;
  },
  afterModel: (_inst, _host, finished) => (finished ? 'restart' : undefined),
};

/** `CSfxTroStretching01`: on the caster (+0.1 y, turned `-angle`); when it ends the lasting stretching effect starts. */
const stretching01: SfxBehavior = {
  place(inst, host) {
    const a = srcYaw(inst, host);
    if (Number.isNaN(a) || !host.world.posOf(inst.idSrc, tmp)) return true; // keeps the last place (`if (pObjSrc)`)
    tmp[1] += 0.1;
    setPos(inst, tmp);
    inst.angle = -a;
    return true;
  },
  finish(inst, host) {
    if (!host.world.posOf(inst.idSrc, tmp)) return;
    const xi = host.hooks.stretchingSfx?.();
    if (xi === undefined || xi <= 0) return;
    host.create(xi, { src: inst.idSrc, pos: [tmp[0], tmp[1], tmp[2]], nSec: -1, cls: 'skill' });
  },
};

/** The `nSec` rule of the troupe classes: 0 plays once, -1 loops forever, N loops and ends after N seconds. */
function troupeAfterModel(inst: SfxInstance, finished: boolean): 'end' | 'restart' | undefined {
  if (!finished) return undefined;
  return inst.nSec === 0 ? 'end' : 'restart';
}

function troupeLifetime(inst: SfxInstance): boolean {
  // `if (m_nFrame > m_nSec * 60) Delete()` after `++m_nFrame`, only for nSec != 0 and != -1
  return inst.nSec !== 0 && inst.nSec !== -1 && inst.frame + 1 > inst.nSec * 60;
}

/** `CSfxTroStretching02`: on the caster while it sits and the party's stretching mode lasts. */
const stretching02: SfxBehavior = {
  ownLifetime: true,
  tick(inst, host) {
    if (troupeLifetime(inst)) return 'end';
    if (!srcAlive(inst, host)) return 'end';
    const state = host.world.stateOf?.(inst.idSrc) ?? OBJSTA_SIT;
    const sitting = (state & OBJSTA_MOVE_ALL) === OBJSTA_SIT;
    const modeOn = host.hooks.partyModeOn?.(PARTY_STRETCHING_MODE) ?? true;
    return sitting && modeOn ? undefined : 'end';
  },
  place(inst, host) {
    const a = srcYaw(inst, host);
    if (Number.isNaN(a) || !host.world.posOf(inst.idSrc, tmp)) return false;
    tmp[1] += 0.1;
    setPos(inst, tmp);
    inst.angle = -a;
    return true;
  },
  afterModel: (inst, _host, finished) => troupeAfterModel(inst, finished),
};

/** `CSfxTroBlitz`: over the head of the target, at the target's scale. */
const blitz: SfxBehavior = {
  ownLifetime: true,
  tick(inst) {
    return troupeLifetime(inst) ? 'end' : undefined;
  },
  place(inst, host) {
    if (inst.idDest === 0 || !host.world.posOf(inst.idDest, tmp)) return false;
    tmp[1] += host.world.headOf?.(inst.idDest) ?? BLITZ_FALLBACK_HEAD;
    setPos(inst, tmp);
    return true;
  },
  afterModel: (inst, _host, finished) => troupeAfterModel(inst, finished),
};
/** Without a model height the mark stands where a man-sized target's head is (`GetMaxHeight() * s - 0.7 * s` at `s = 1` for a player). */
export const BLITZ_FALLBACK_HEAD = 1.2;

/** `CSfxDuelParty`: a base effect on the duelist that ends with the duel and hides under a transparency buff. */
const duelParty: SfxBehavior = {
  tick(inst, host) {
    if (!srcAlive(inst, host)) return 'end';
    return host.hooks.duelEnded?.(inst.idSrc, inst.duelKind) === true ? 'end' : undefined;
  },
  place(inst, host) {
    return host.hooks.flagHidden?.(inst.idSrc) !== true;
  },
};

// ---- rain -----------------------------------------------------------------------------------------------------------------------

function groundAt(host: BehaviorHost, x: number, z: number, fallback: number): number {
  return host.world.heightAt?.(x, z) ?? fallback;
}

/** `CSfxItemRangeAtk1_AllowRain`: two arrows every 5th step (4.2 compared with the integer counter), 50 times. */
const arrowRain: SfxBehavior = {
  modelless: true,
  tick(inst, host) {
    const bh = inst.bh;
    const ground = groundAt(host, inst.origin[0], inst.origin[2], inst.origin[1]);
    // `vPos.y = fHeight + xRandomF(2) + 9` is drawn every step, spawn or not
    const y = ground + host.rand() * 2 + 9;
    bh.timer += 1;
    if (bh.count >= RAIN_ITERATIONS) return 'end';
    if (bh.timer >= ARROW_RAIN_PERIOD) {
      bh.timer = 0;
      let x = inst.origin[0] + (host.rand() * 6 - 3);
      let z = inst.origin[2] + (host.rand() * 6 - 3);
      host.create(CHILD_XI.arrowRain, { pos: [x, y, z], cls: 'skill' });
      // the second arrow is jittered from the first one's place
      x += host.rand() * 6 - 3;
      z += host.rand() * 6 - 3;
      host.create(CHILD_XI.arrowRain, { pos: [x, y, z], cls: 'skill' });
      bh.count += 1;
    }
    return undefined;
  },
};

/** `CSfxItemRangeAtk1_StoneRain`: one stone every 12th step within +-20 m, 50 times. */
const stoneRain: SfxBehavior = {
  modelless: true,
  tick(inst, host) {
    const bh = inst.bh;
    const ground = groundAt(host, inst.origin[0], inst.origin[2], inst.origin[1]);
    const y = ground + host.rand() * 2 + 40;
    bh.timer += 1;
    if (bh.count >= RAIN_ITERATIONS) return 'end';
    if (bh.timer >= STONE_RAIN_PERIOD) {
      bh.timer = 0;
      const x = inst.origin[0] + (host.rand() * 40 - 20);
      const z = inst.origin[2] + (host.rand() * 40 - 20);
      host.create(CHILD_XI.stoneDust, { pos: [x, y, z], scale: 8, cls: 'skill' });
      bh.count += 1;
    }
    return undefined;
  },
};

/** A falling drop: fixed speed drawn at its birth, straight down, lands at the ground (plus `clearance`) and leaves its splash. */
function drop(
  speedBase: number,
  speedSpan: number,
  clearance: number,
  splash: number,
  splashScale: number | undefined,
): SfxBehavior {
  return {
    ownLifetime: true,
    init(inst, host) {
      // `m_fSpeed = xRandomF(span) + base`, drawn in the constructor
      inst.bh.timer = host.rand() * speedSpan + speedBase;
    },
    tick(inst, host) {
      if (inst.frame >= DROP_MAX_TICKS) return 'end';
      const speed = inst.bh.timer;
      inst.pos[1] -= speed;
      const ground = host.world.heightAt?.(inst.pos[0], inst.pos[2]);
      if (ground !== undefined && inst.pos[1] < ground + clearance) {
        inst.pos[1] = ground;
        host.create(splash, {
          pos: [inst.pos[0], inst.pos[1], inst.pos[2]],
          ...(splashScale !== undefined ? { scale: splashScale } : {}),
          cls: 'skill',
        });
        return 'end';
      }
      return undefined;
    },
    afterModel: (_inst, _host, finished) => (finished ? 'restart' : undefined),
  };
}

/** `CSfxItemRangeAtk1_Allow`: speed U(0.2, 0.9), lands on the ground and leaves `XI_SKILL_RAG_BOW_ARROWRAIN01`. */
const arrowDrop = drop(0.2, 0.7, 0, CHILD_XI.arrowRainHit, undefined);
/** `CSfxItemRangeAtk1_Stone`: speed U(0.7, 1.4), splashes 2.5 above the ground with `XI_SKILL_CIRCLE_DUST` at scale 3. */
const stoneDrop = drop(0.7, 0.7, 2.5, CHILD_XI.circleDust, 3);

export const BEHAVIORS = {
  castFire,
  splmash,
  burstcrack,
  tampinghole,
  handLink,
  bladeLink,
  wingAngel,
  stretching01,
  stretching02,
  blitz,
  duelParty,
  arrowRain,
  stoneRain,
  arrowDrop,
  stoneDrop,
} as const;
