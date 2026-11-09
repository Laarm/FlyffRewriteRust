// The game side of the skill effect classes (Plan/41 section 17.2, WP-REND-24): the answers `render/sfx/skillClasses` asks of the game
// (`SkillFxHooks`) and the one trigger of the classes that no message drives - the duel flag over the opponents.
//
//   hooks.stretchingSfx   `prj.GetPartySkill(ST_STRETCHING)->dwSfxObj2`
//   hooks.duelEnded       `CSfxDuelParty::Process`: the duel that put the flag there is over (the duel fields of `features/pvp`); a guild-war flag ends when the
//                         mover's `idWar` is 0
//   hooks.flagHidden      `IsMode(TRANSPARENT_MODE)` of the flag's mover (the Dark Illusion buff of OTHER players is not mirrored)
//   hooks.hotAirPainTick  the audio cue of the Hot Air effect (44 T08h)
//   frame()               `CMover::RenderFlag`: one `XI_GEN_PVP_FLAG01` (scale 1.5, `nSec = -1`) over every duel opponent and every enemy of the
//                         declared guild war that has none (WP-REND-26)
//
// `partyModeOn` is not answered: the party feature does not exist yet and the effect then assumes the mode is on.

import { eventOf } from '../features/events/api';
import { duelOf } from '../features/pvp/duel/duelModule';
import { DUEL_STATE_OUT } from '../features/pvp/duel/book';
import { EntityKind } from '../game/entities';
import { ST_STRETCHING } from '../gen/defs/skill';
import { XI_GEN_PVP_FLAG01 } from '../gen/defs/obj';
import { validEffect } from '../game/fx/hitSpark';
import type { SkillFxHooks } from '../render/sfx/classSpec';
import { ENEMY } from '../render/sfx/skillBehaviors';
import type { ClientContext } from './context';

/** `m_dwMode` bit of `TRANSPARENT_MODE` (`MoverVisualState.mode`). */
const MODE_TRANSPARENT = 0x2;
/** `CMover::RenderFlag`: the flag stands at 1.5 times the model scale. */
export const DUEL_FLAG_SCALE = 1.5;
/** `EVE_GUILDWAR`: `g_eLocal.GetState( EVE_GUILDWAR ) == 1` (the event flag `guild_war`, Plan/22 §3). */
export const EVE_GUILDWAR = 3;

export interface SkillFxHost {
  readonly hooks: SkillFxHooks;
  /** Once per rendered frame: the duel flags. */
  frame(): void;
  /** The movers that carry a flag (probe, tests). */
  flagged(): readonly number[];
  /** Forgets the flags (a world change). */
  reset(): void;
}

export function createSkillFxHost(ctx: ClientContext): SkillFxHost {
  /** mover -> `ENEMY_*` kind of its live flag. */
  const flags = new Map<number, number>();
  const tmp: [number, number, number] = [0, 0, 0];

  const hooks: SkillFxHooks = {
    stretchingSfx: () => {
      const xi = ctx.data?.partySkill(ST_STRETCHING)?.sfx_obj[1];
      return validEffect(xi) ? xi : undefined;
    },
    duelEnded: (mover, kind) => {
      // `CSfxDuelParty::Process`: a guild-war flag ends when the mover is in no war any more (it may be created again later)
      if (kind === ENEMY.WAR) return (ctx.entities.get(mover)?.visual?.idWar ?? 0) === 0;
      const f = duelOf(ctx)?.fieldsOf(mover);
      if (!f) return false;
      switch (kind) {
        case ENEMY.PVP_PARTY:
          // `m_nDuel != 2 || m_nDuelState == 300 || m_nDuelState == 0`
          return f.duel !== 2 || f.state === DUEL_STATE_OUT || f.state === 0;
        case ENEMY.PVP:
          return f.duel !== 1;
        default:
          // PK flags (`IsPKing`; `events.pk` is off) have no mirror here: they stay until the mover goes
          return false;
      }
    },
    flagHidden: (mover) => ((ctx.entities.get(mover)?.visual?.mode ?? 0) & MODE_TRANSPARENT) !== 0,
    hotAirPainTick: (pos, first, attacker, target) => {
      ctx.audio.cues?.hotAirPainTick([pos[0], pos[1], pos[2]], first, attacker, target);
    },
    // ---- render-audio-m7 (WP-REND-27, 44 T41): the effects the renderer makes itself play their table sound through the same cue as `withAudioFx`
    sfxCreated: (xi, pos) => {
      ctx.audio.cues?.sfxCreated(xi, pos);
    },
    // the boss quakes: only while the owner turned the shake on (`features.no_quake = false`; the V15 look, F-333 / PL-134)
    quake: (frames, size) => {
      if (!ctx.features.features.no_quake) ctx.input.quake?.(frames, size);
    },
    // `GetSpeed( m_fSpeed )` of a mover: the data speed of its model through the rules (the event balloon's follow speed)
    speedOf: (mover) => {
      const rec = ctx.entities.get(mover);
      const base = rec ? ctx.data?.mover(rec.propId)?.speed : undefined;
      const rules = ctx.rules as {
        moveSpeed?: (obj: number, base: number, f: number) => number;
      } | null;
      return base === undefined || !rules?.moveSpeed ? undefined : rules.moveSpeed(mover, base, 1);
    },
    // `GetSkillProp( SI_GEN_BURN, 1 )` of the Meteonyker: the client has the skill's data (the level rows are not mirrored separately)
    hasSkillRow: (skillId) => ctx.data?.skill(skillId) !== undefined,
  };

  /**
   * The enemies of the declared guild war (`CMover::RenderName`, `MoverRender.cpp:1829-1845`): while the event `guild_war` is on, a player of the own war
   * (`m_idWar != 0 && m_idWar == own`) in another guild gets the flag. Allies of the same war are not flagged. DEVIATION: the original creates the flag
   * from inside the name drawing (name options on, within 100 m); here it does not depend on the name options.
   */
  const warEnemies = (wanted: Map<number, number>): void => {
    if (eventOf(ctx)?.isEventOn(EVE_GUILDWAR) !== true) return;
    const self = ctx.entities.self();
    const war = self?.visual?.idWar ?? 0;
    if (war === 0) return;
    const ownGuild = self?.visual?.guild?.idGuild;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      const v = rec.visual;
      if (!v || rec.isSelf || v.idWar !== war || v.guild === null) return;
      if (v.guild.idGuild === ownGuild) return;
      // a duel opponent keeps its single flag (the original shares the `MVRF_WARMARK` bit)
      if (!wanted.has(rec.objId)) wanted.set(rec.objId, ENEMY.WAR);
    });
  };

  const frame = (): void => {
    const fx = ctx.render?.fx;
    const duel = duelOf(ctx);
    if (!fx) {
      flags.clear();
      return;
    }
    const wanted = new Map<number, number>();
    if (duel) {
      const markers = duel.markers();
      for (const id of markers.solo) wanted.set(id, ENEMY.PVP);
      for (const id of markers.party) wanted.set(id, ENEMY.PVP_PARTY);
    }
    warEnemies(wanted);
    if (!duel && wanted.size === 0 && flags.size === 0) return;
    // a mover that is no opponent any more, or that left the world, may get a flag again later
    for (const id of [...flags.keys()]) {
      if (!wanted.has(id) || !ctx.entities.get(id)) flags.delete(id);
    }
    const view = ctx.sim.view;
    for (const [id, kind] of wanted) {
      const old = flags.get(id);
      if (old !== undefined) {
        // one flag per mover (the shared `MVRF_WARMARK` bit); a flag of another kind that has ended by its own rule (the duel is over while the
        // war goes on) makes room for the kind that is wanted now
        if (old === kind || hooks.duelEnded?.(id, old) !== true) continue;
        flags.delete(id);
      }
      if (view?.valid !== true || !view.posOf(id, tmp)) continue;
      fx.sfx(XI_GEN_PVP_FLAG01, {
        src: id,
        pos: [tmp[0], tmp[1], tmp[2]],
        nSec: -1,
        scale: DUEL_FLAG_SCALE,
        duelKind: kind as 0 | 1 | 2 | 3,
        cls: 'skill',
      });
      flags.set(id, kind);
    }
  };

  return {
    hooks,
    frame,
    flagged: () => [...flags.keys()],
    reset: () => {
      flags.clear();
    },
  };
}
