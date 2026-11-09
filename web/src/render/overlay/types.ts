// Types of the overlay renderer (Plan/41 §19): what it is told about the movers around the camera, the textures it
// draws with, and the draw items the layout functions produce.

import type { Vec3LH } from '../types';
import type { OverlayFont } from './GlyphAtlas';

/** `AUTH_GAMEMASTER` ('L') and `AUTH_ADMINISTRATOR` ('P'): the authority letters compare as numbers (23 §1.1). */
export const AUTH_GAMEMASTER = 76;
export const AUTH_ADMINISTRATOR = 80;

export type SubjectKind = 'player' | 'monster' | 'npc' | 'pet';

/** The disguise models a GM can wear whose name line the original replaces (`MI_CROWN*`, `MI_DWARPET*`). */
export type NameDisguise = 'crown' | 'dwarpet';

export interface SubjectGuild {
  /** `GetGuildId()`. */
  id: number;
  name: string;
  level: number;
  /** `IsMaster(m_idPlayer)`. */
  master: boolean;
  /** `m_dwLogo`: 1..27 is a logo picture, 0 none; anything else draws nothing (WP-REND-26; absent = 0). */
  logo?: number;
}

/** `GetLegendChar()` of a player: the job TYPE master or hero (`JTYPE_MASTER`, `JTYPE_HERO`); `null` for every other job. */
export type LegendClass = 'master' | 'hero';

/**
 * What the overlay needs of the guild siege (ui-siege's `eventStore.siege`, read by `features/guildmarks/siegeView.ts`): the war state for
 * the defender colour, the siege state, the winner guild and the best player for the two marks, and the defenders (character ids).
 */
export interface ViewerSiege {
  /** `g_GuildCombatMng.m_nState`: 0 Open, 1 Close (the marks), 2 War, 3 CompleteClose, 4 GmClose. */
  readonly state: number;
  /** `g_GuildCombatMng.m_nGCState`: 100 .. 107; the defender colour from 103 (MAINTENANCE) on. */
  readonly warState: number;
  /** `m_uWinGuildId`, 0 for none. */
  readonly winGuildId: number;
  /** `m_uBestPlayer` (a character id), 0 for none. */
  readonly bestPlayerId: number;
  /** The keys of `m_mapGC_GuildStatus` (`GetGCStatusDefender`): character ids of the defenders. */
  readonly defenders: ReadonlySet<number>;
}

/**
 * One mover the overlay may draw something for: the flags the original's `RenderName`, `RenderHP` and
 * `RenderChrState` read, plus the world position of its head anchor. The world render host builds it from the entity
 * table and the sim render view every frame (`overlaySubjectOf`).
 */
export interface OverlaySubject {
  id: number;
  kind: SubjectKind;
  /** `IsActiveMover()`. */
  isSelf: boolean;
  name: string;
  /** The character id (`m_idPlayer`) of a player: the key of the siege roster and of the best-player mark. */
  idPlayer?: number;
  /** `m_nLevel` of a player (the master icon table: 60..120). */
  level?: number;
  /** `GetLegendChar()`: the master / hero icon above the name. */
  legend?: LegendClass | null;
  /** The selected honor title (`GetTitle()`), `''` for none. */
  title: string;
  /** `m_dwAuthorization` (authority letter). */
  authorization: number;
  /** `IsMode(TRANSPARENT_MODE)`. */
  transparent: boolean;
  /** `QUIZ_RENDER_SKIP_MODE` / `GCWAR_RENDER_SKIP_MODE` of another player. */
  renderSkip: boolean;
  chaotic: boolean;
  pink: boolean;
  /** `g_Party.FindMember(idPlayer) != -1`. */
  partyMember: boolean;
  guild: SubjectGuild | null;
  /** A GM with `IK3_TEXT_DISGUISE`: no guild line, no HP bar. */
  textDisguise: boolean;
  /** `IsPeaceful()`. */
  peaceful: boolean;
  /** `m_bActiveAttack`. */
  activeAttack: boolean;
  /** The NPC's structure name (`lpCharacter->m_nStructure`), `null` for none. */
  structure: string | null;
  /** `GetProp()->szName`: used by the disguise lines. */
  propName: string;
  disguise: NameDisguise | null;
  /** `CHS_STUN`. */
  stun: boolean;
  hp: number;
  maxHp: number;
  /** `IsStateDbuff()`. */
  debuffed: boolean;
  /** The head anchor in LH world space (`headAnchor`; a pet passes its smoothed screen position's world point). */
  head: Vec3LH;
  /** The logical position (LH): where the flight speed digits are anchored (`RenderTurboGauge` projects the model origin). */
  foot?: Vec3LH;
  /** The distance from the camera (`m_fDistCamera`). */
  distCamera: number;
  /** The object is outside the view frustum (`IsCull()`): nothing is drawn for it. */
  culled: boolean;
  /** `m_nQuestEmoticonIndex` of an NPC (1 new, 2 in progress, 3 completable, 4 near level; <= 0 or absent: none; WP-REND-27, `QuestMarks.ts`). */
  questEmoticon?: number;
}

/** The viewing player and the frame-wide switches. */
export interface OverlayViewer {
  /** The viewer's authority letter (`CMover::GetActiveMover()->m_dwAuthorization`). */
  authorization: number;
  /** Zoom level of the camera: the own name and HP bar are hidden when not 0. */
  telescope: 0 | 1 | 2 | 3;
  /** The world is `WI_WORLD_GUILDWAR` (the colours of the guild war). */
  guildWar: boolean;
  /** `g_pPlayer->GetGuild()->GetGuildId()`, `null` without a guild. */
  ownGuildId: number | null;
  /** `g_WorldMng()->m_bViewHP`. */
  viewHp: boolean;
  /** The guild siege facts of the colour override and the two marks; `null` / absent without the siege feature (no override, no marks). */
  siege?: ViewerSiege | null;
}

/** A drawable image or a rectangle of one. */
export interface OverlayImage {
  readonly source: CanvasImageSource;
  readonly sx: number;
  readonly sy: number;
  readonly w: number;
  readonly h: number;
}

/** The textures of the overlay. Both return `null` while the image loads or when it does not exist (nothing is drawn). */
export interface OverlayAssets {
  /** A whole image by its lower-case path (`theme/gaupartyhp.tga`). */
  image(path: string): OverlayImage | null;
  /** A sprite of an atlas by the lower-case `.inc` path and the sprite index. */
  sprite(atlas: string, index: number): OverlayImage | null;
}

/** An assets port with nothing in it (tests, before the textures exist). */
export const NO_OVERLAY_ASSETS: OverlayAssets = { image: () => null, sprite: () => null };

/** A text item of a layout. */
export interface TextItem {
  readonly kind: 'text';
  readonly font: OverlayFont;
  readonly text: string;
  readonly x: number;
  readonly y: number;
  /** `0xAARRGGBB`. */
  readonly color: number;
}

/** A sprite of an atlas. `scale` (default 1) scales both axes. */
export interface SpriteItem {
  readonly kind: 'sprite';
  readonly atlas: string;
  readonly index: number;
  readonly x: number;
  readonly y: number;
  /** 0..255. */
  readonly alpha: number;
  readonly scale?: number;
  /** Stretches the sprite to this destination size instead of scaling it (the 9-patch pieces). */
  readonly dw?: number;
  readonly dh?: number;
  /**
   * Clockwise quarter turns of the drawn sprite about the centre of its destination box (BFIX1 #001); absent or 0 draws as
   * before. For 90 and 270 the footprint is the box turned about its centre (only square sprites use it).
   */
  readonly rotate?: 0 | 90 | 180 | 270;
}

/** A whole image, optionally cropped to its left `cropW` source pixels and scaled. */
export interface ImageItem {
  readonly kind: 'image';
  readonly path: string;
  readonly x: number;
  readonly y: number;
  readonly alpha: number;
  readonly scaleX: number;
  readonly scaleY: number;
  /** Only the left `cropW` source pixels are drawn (the HP fill). */
  readonly cropW?: number;
  /** Stretches the image to this destination size instead of scaling it (the shout balloon, the vendor strip). */
  readonly dw?: number;
  readonly dh?: number;
}

/** A rectangle outline (`C2DRender::RenderRect`) or fill (`RenderFillRect`); `color` is `0xAARRGGBB`. */
export interface RectItem {
  readonly kind: 'rect';
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  readonly color: number;
  /** `true`: the area is filled; `false`: a one pixel outline. */
  readonly filled: boolean;
}

/** A filled convex polygon of `points` (`[x0, y0, x1, y1, ...]`) in `color` (`0xAARRGGBB`): the off-screen focus arrow. */
export interface PolyItem {
  readonly kind: 'poly';
  readonly points: readonly number[];
  readonly color: number;
}

export type DrawItem = TextItem | SpriteItem | ImageItem | RectItem | PolyItem;
