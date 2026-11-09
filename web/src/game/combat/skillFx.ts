// The effects a skill triggers on the client (Plan/41 17.3 "Skill casting/use"; ref/20 8.1; `CMover::DoUseSkill` client branch
// MoverSkill.cpp:918-1060, `CCtrl::CreateSkillSfx` Ctrl.cpp:878-997; WP-SKILL-12 + WP-REND-24 triggers):
//
//   onCast(...)           at the start of a cast (`UseSkillNtf`): the hand-link effects of the skill line, the casting effect
//                         `sfx_obj` on the caster, the `TID_GAME_SKILL_SELF` / `TID_GAME_SKILL_MATE` chat line
//   createSkillSfx(...)   at the local hit frame (`DoApplySkillEx` -> `CreateSkillSfx`): the hit effect `sfx_obj3` on the target (or in
//                         front of the caster for a melee skill), `sfx_obj5` on the caster, the yo-yo flights; the buff loop effect
//                         `sfx_obj4` is the buff's (`buffs.ts`, `IBuff::CreateSFX`)
//
// Everything goes through `RenderFx.sfx`; the behaviour of the ids (fixed angles, hand follow, lifetimes) is `render/sfx/classes.ts`.

import type { ClientContext } from '../../app/context';
import type { ClientItem } from '../../data';
import { EXT_AROUNDATK, EXT_MAGICATK, EXT_MAGICATKSHOT, EXT_MELEEATK } from '../../gen/defs/define';
import { IK3_YOYO } from '../../gen/defs/itemkind';
import { PARTS_RWEAPON } from '../../gen/defs/neuz';
import {
  SI_ACR_SUP_SLOWSTEP,
  SI_ACR_YOYO_CROSSLINE,
  SI_ACR_YOYO_DEADLYSWING,
  SI_ASS_CHEER_CIRCLEHEALING,
  SI_GEN_FLASH,
  SI_JST_YOYO_HITOFPENYA,
  SI_JST_YOYO_VATALSTAB,
} from '../../gen/defs/skill';
import { TID_GAME_SKILL_MATE, TID_GAME_SKILL_SELF } from '../../gen/defs/text';
import type { RenderFx } from '../../render/fx';
import { EntityKind } from '../entities';
import { validEffect } from '../fx/hitSpark';
import { handLinks } from './skillLinks';
import type { SkillRuntime } from './skills';

type V3 = [number, number, number];

/** `SI_GEN_FLASH` effects are drawn at scale 1.5 (`CreateSkillSfx`). */
export const FLASH_SKILL_SCALE = 1.5;
/** `AngleToVectorXZ(&vLocal, GetAngle(), 1.3f)`: a melee hit effect stands 1.3 m in front of the caster. */
export const MELEE_EFFECT_REACH = 1.3;

export interface SkillFx {
  /** The start of a cast (see the file comment). */
  onCast(
    casterId: number,
    own: boolean,
    prop: ClientItem,
    skillId: number,
    level: number,
    targetId: number,
  ): void;
  /** `CCtrl::CreateSkillSfx(target, prop, addProp)` of the caster `srcId`; `true` when an effect was created. */
  createSkillSfx(srcId: number, targetId: number, skillId: number, level: number): boolean;
}

/** The hands a yo-yo skill throws from (`CreateYoyoSkill`): one from the right, or one from each hand. */
export function yoyoHands(skillId: number): readonly (0 | 1)[] | null {
  switch (skillId) {
    case SI_ACR_SUP_SLOWSTEP:
    case SI_JST_YOYO_HITOFPENYA:
    case SI_JST_YOYO_VATALSTAB:
      return [0];
    case SI_ACR_YOYO_DEADLYSWING:
    case SI_ACR_YOYO_CROSSLINE:
      return [0, 1];
    default:
      return null;
  }
}

/** `printf`-style `%s` replacement in order. */
export function formatText(template: string, ...args: readonly string[]): string {
  let i = 0;
  return template.replace(/%s/g, () => args[i++] ?? '');
}

/** The skills of a targeted effect: where the chat line is only printed when the target is a player and the skill is no attack. */
function isAttackExe(exeTarget: number): boolean {
  return (
    exeTarget === EXT_MELEEATK ||
    exeTarget === EXT_MAGICATK ||
    exeTarget === EXT_AROUNDATK ||
    exeTarget === EXT_MAGICATKSHOT
  );
}

export function createSkillFx(ctx: ClientContext, rt: SkillRuntime): SkillFx {
  const tmp: V3 = [0, 0, 0];
  const posOf = (id: number): V3 | null => {
    const view = ctx.sim.view;
    if (view?.valid === true && view.posOf(id, tmp)) return [tmp[0], tmp[1], tmp[2]];
    const spawn = ctx.entities.get(id)?.spawn;
    const p = spawn && 'obj' in spawn ? spawn.obj?.pos : undefined;
    return p ? [p.x, p.y, p.z] : null;
  };
  const angleOf = (id: number): number => {
    const view = ctx.sim.view;
    return view?.valid === true ? view.angleOf(id) : Number.NaN;
  };
  const fx = (): RenderFx | undefined => ctx.render?.fx;
  const selfId = (): number => ctx.entities.self()?.objId ?? 0;
  const isPlayer = (id: number): boolean => ctx.entities.get(id)?.kind === EntityKind.Player;
  const nameOf = (id: number): string => ctx.entities.get(id)?.visual?.name ?? '';

  /** `AngleToVectorXZ(&vLocal, angle, 1.3)` + the caster's position, one metre up. */
  const inFront = (src: V3, angle: number): V3 => {
    const a = ((Number.isNaN(angle) ? 0 : angle) * Math.PI) / 180;
    return [
      src[0] + Math.sin(a) * MELEE_EFFECT_REACH,
      src[1] + 1,
      src[2] - Math.cos(a) * MELEE_EFFECT_REACH,
    ];
  };

  const onCast: SkillFx['onCast'] = (casterId, own, prop, skillId, _level, targetId) => {
    const f = fx();
    const at = posOf(casterId);
    if (f && at) {
      // one effect on each hand / on one hand, by the skill line (the jester ids become the blade class, KEEP-QUIRK)
      for (const l of handLinks(skillId)) {
        f.sfx(l.xi, { src: casterId, pos: at, partLink: l.hand, cls: 'skill' });
      }
      // the casting effect on the caster (`dwSfxObj`)
      const o = prop.sfx_obj[0];
      if (validEffect(o)) f.sfx(o, { src: casterId, pos: at, cls: 'skill' });
    }
    // the chat lines of a skill that is no attack, on a player (`pTarget->IsPlayer()`): the own cast, or a cast on the own player
    if (!isAttackExe(prop.exe_target) && isPlayer(targetId)) {
      const self = selfId();
      const name = prop.name;
      if (own) {
        say(ctx, rt, TID_GAME_SKILL_SELF, name);
      } else if (
        targetId === self &&
        (casterId !== targetId || skillId === SI_ASS_CHEER_CIRCLEHEALING)
      ) {
        say(ctx, rt, TID_GAME_SKILL_MATE, nameOf(casterId), name);
      }
    }
  };

  /**
   * `CCtrl::CreateYoyoSkill`: the skills that throw the yo-yo (the effect is `sfx_obj[1]` of the equipped right-hand weapon; without a
   * weapon prop nothing flies).
   */
  const launchYoyo = (srcId: number, skillId: number, targetId: number): void => {
    const hands = yoyoHands(skillId);
    if (!hands) return;
    const weapon = ctx.entities.get(srcId)?.visual?.equip.get(PARTS_RWEAPON)?.itemId;
    const xi = weapon === undefined ? undefined : ctx.data?.item(weapon)?.sfx_obj[1];
    if (!validEffect(xi)) return;
    fx()?.skillYoyo?.(srcId, targetId, xi, hands);
  };

  const createSkillSfx: SkillFx['createSkillSfx'] = (srcId, targetId, skillId, level) => {
    const f = fx();
    if (!f) return false;
    const prop = rt.props.prop(skillId);
    const add = prop ? rt.props.add(prop, level) : undefined;
    const src = posOf(srcId);
    const tgt = posOf(targetId);
    if (!prop || !add || !src || !tgt) return false;
    const [, , o3, o4, o5] = prop.sfx_obj;
    const melee = prop.exe_target === EXT_MELEEATK;
    const yoyo = prop.link_kind === IK3_YOYO;
    const hasTime = (add.skill_time | 0) > 0;
    let created = false as boolean;
    const make = (xi: number, o: Parameters<RenderFx['sfx']>[1]): void => {
      const scale = skillId === SI_GEN_FLASH ? FLASH_SKILL_SCALE : undefined;
      f.sfx(xi, { ...o, ...(scale !== undefined ? { scale } : {}), cls: 'skill' });
      created = true;
    };
    const front = (): V3 => (yoyo ? [tgt[0], tgt[1] + 1, tgt[2]] : inFront(src, angleOf(srcId)));
    if (hasTime) {
      // a skill with a duration and no `sfx_obj4`; with one, the buff creates the loop (`IBuff::CreateSFX`)
      if (validEffect(o4)) return false;
      if (melee) {
        const local = front();
        if (validEffect(o3)) make(o3, { src: srcId, dest: targetId, pos: src, posDest: local });
      } else if (validEffect(o3)) {
        make(o3, { src: srcId, dest: targetId, pos: src, posDest: tgt });
      }
      if (validEffect(o5)) make(o5, { src: srcId, dest: srcId, pos: src, posDest: src });
      if (yoyo) launchYoyo(srcId, skillId, targetId);
    } else {
      if (melee) {
        if (validEffect(o3)) {
          const local = front();
          make(o3, { src: srcId, dest: targetId, pos: local, posDest: local });
        }
      } else if (validEffect(o3)) {
        make(o3, { src: srcId, dest: targetId, pos: src, posDest: [tgt[0], tgt[1] + 1, tgt[2]] });
      }
      if (yoyo) {
        launchYoyo(srcId, skillId, targetId);
      } else if (validEffect(o5)) {
        make(o5, { src: srcId, dest: srcId, pos: src, posDest: src });
      }
    }
    // `PLAYSND(dwSndAttack1)`: the fire sound of the skill, once an effect was created (audio trigger T09)
    if (created) ctx.audio.cues?.skillEffectFired(srcId, skillId);
    return created;
  };

  return { onCast, createSkillSfx };
}

/** A system-chat line from a text id with `%s` arguments (`PutString(str.Format(GetText(tid), ...))`). */
function say(ctx: ClientContext, rt: SkillRuntime, tid: number, ...args: string[]): void {
  const text = ctx.data?.text(tid);
  if (text === undefined || text === '') return;
  rt.hooks.chatLine?.(formatText(text, ...args), tid);
}
