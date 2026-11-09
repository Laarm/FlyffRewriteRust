// The adapter from the entity table to the overlay's subjects (`OverlaySubject`). The entity table (doc 40 §6.3) holds
// what the protocol handlers know (name, authority, mode bits, PK state, guild); the position, the model height and the
// distance come from the sim render view and the model, which only the world render host can read: it passes them in.
// The shapes below are structural subsets of `EntityRecord` / `MoverVisualState` so that `render/` does not import `game/`.

import type { Vec3LH } from '../types';
import { distance, headAnchor } from './anchor';
import type { LegendClass, NameDisguise, OverlaySubject, SubjectKind } from './types';

/** `CMover::m_dwMode` bits the overlay reads (23 §2.1). */
export const MODE_TRANSPARENT = 0x00000002;
export const MODE_GCWAR_RENDER_SKIP = 0x00100000;
export const MODE_QUIZ_RENDER_SKIP = 0x00200000;

/** `JTYPE_MASTER` and `JTYPE_HERO` of `etc.inc` (the job TYPE that `GetLegendChar()` reports). */
export const JTYPE_MASTER = 5;
export const JTYPE_HERO = 6;

/** The subset of `GuildBrief` the name line needs. */
export interface GuildBriefLike {
  readonly idGuild: number;
  readonly idMaster: number;
  readonly level: number;
  readonly name: string;
  /** `m_dwLogo` (1..27). */
  readonly logo?: number;
}

/** `GetLegendChar()`: the legend class of a job TYPE (`JobRow.type`), `null` for every other type or an unknown job. */
export function legendOf(jobType: number | undefined): LegendClass | null {
  if (jobType === JTYPE_MASTER) return 'master';
  if (jobType === JTYPE_HERO) return 'hero';
  return null;
}

/** The subset of `MoverVisualState` the overlay reads. */
export interface VisualLike {
  readonly kind: 'player' | 'monster' | 'npc' | 'pet';
  readonly name: string;
  readonly title: string;
  readonly authorization: number;
  readonly mode: number;
  readonly scale: number;
  readonly guild: GuildBriefLike | null;
  readonly pk: { readonly chaotic: boolean; readonly pink: boolean };
  readonly partyMember: boolean;
  readonly activeAttack: boolean;
  readonly peaceful: boolean;
  readonly structure: string | null;
}

/** The subset of `EntityRecord` the overlay reads. */
export interface EntityLike {
  readonly objId: number;
  readonly isSelf: boolean;
  readonly idPlayer?: number;
  readonly hp: number;
  readonly maxHp?: number;
  readonly visual?: VisualLike;
  readonly removed: boolean;
}

/** What the world render host knows about the object that the entity table does not. */
export interface Placement {
  /** The logical position (LH). A pet passes its smoothed screen position. */
  readonly pos: Vec3LH;
  /** `bb.max.y` of the current model (unscaled). */
  readonly headY: number;
  /** The object is outside the frustum. */
  readonly culled: boolean;
  /** `m_nQuestEmoticonIndex` of an NPC (render-audio-m7; absent = none). */
  readonly questEmoticon?: number;
  /** `CHS_STUN`, `IK3_TEXT_DISGUISE`, `IsStateDbuff()` and the disguise model: from the buff and state mirrors. */
  readonly stun?: boolean;
  readonly textDisguise?: boolean;
  readonly debuffed?: boolean;
  readonly disguise?: NameDisguise | null;
  /** `GetProp()->szName`. */
  readonly propName?: string;
  /** `m_nLevel` of a player: the own level from `selfStore`, another player's from the mirrored `visual.level` (`features/guildmarks`). */
  readonly level?: number;
  /** The job TYPE (`JobRow.type`, `JTYPE_*`) of a player: the master / hero icon. */
  readonly jobType?: number;
}

/** The viewer's position, for the distance. */
export function subjectOf(
  rec: EntityLike,
  place: Placement,
  cameraPos: Vec3LH,
): OverlaySubject | null {
  const v = rec.visual;
  if (!v || rec.removed) return null;
  const kind: SubjectKind = v.kind;
  const g = v.guild;
  return {
    id: rec.objId,
    kind,
    isSelf: rec.isSelf,
    name: v.name,
    ...(rec.idPlayer !== undefined ? { idPlayer: rec.idPlayer } : {}),
    ...(place.level !== undefined ? { level: place.level } : {}),
    ...(v.kind === 'player' && legendOf(place.jobType) !== null
      ? { legend: legendOf(place.jobType) }
      : {}),
    title: v.title,
    authorization: v.authorization,
    transparent: (v.mode & MODE_TRANSPARENT) !== 0,
    renderSkip: !rec.isSelf && (v.mode & (MODE_QUIZ_RENDER_SKIP | MODE_GCWAR_RENDER_SKIP)) !== 0,
    chaotic: v.pk.chaotic,
    pink: v.pk.pink,
    partyMember: v.partyMember,
    guild: g
      ? {
          id: g.idGuild,
          name: g.name,
          level: g.level,
          master: rec.idPlayer !== undefined && g.idMaster === rec.idPlayer,
          ...(g.logo !== undefined ? { logo: g.logo } : {}),
        }
      : null,
    textDisguise: place.textDisguise ?? false,
    peaceful: v.peaceful,
    activeAttack: v.activeAttack,
    structure: v.structure,
    propName: place.propName ?? '',
    disguise: place.disguise ?? null,
    stun: place.stun ?? false,
    hp: rec.hp,
    maxHp: rec.maxHp ?? 0,
    debuffed: place.debuffed ?? false,
    head: headAnchor(place.pos, place.headY, v.scale),
    foot: place.pos,
    distCamera: distance(place.pos, cameraPos),
    culled: place.culled,
    ...(place.questEmoticon !== undefined ? { questEmoticon: place.questEmoticon } : {}),
  };
}
