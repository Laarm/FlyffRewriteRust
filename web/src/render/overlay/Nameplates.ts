// Nameplates (Plan/41 §19.3; World3D.cpp:1259-1320, MoverRender.cpp:1272-1870): which movers get a name line, its text,
// colour and position. Pure functions over `OverlaySubject`: the colour decision table is unit-tested one row at a
// time, the Canvas2D drawing is `OverlayRenderer`'s.

import { even } from './anchor';
import { OVERLAY_FONTS, type TextMeter } from './GlyphAtlas';
import {
  byteLength,
  COLOR_SIEGE_DEFENDER,
  isSiegeDefender,
  layoutGuildMarks,
  type MarkExtents,
} from './GuildMarks';
import {
  AUTH_ADMINISTRATOR,
  AUTH_GAMEMASTER,
  type DrawItem,
  type OverlayAssets,
  type OverlaySubject,
  type OverlayViewer,
  type SubjectGuild,
} from './types';

// ---- colours (`0xAARRGGBB`) ---------------------------------------------------------------------------------

/** `COLOR_NPC`: peaceful NPCs, structure lines and the GM disguise lines. */
export const COLOR_NPC = 0xffa0a0ff;
/** `COLOR_MONSTER`. */
export const COLOR_MONSTER = 0xffffffa0;
/** `0xffff0000`: a hostile monster that attacks first. */
export const COLOR_ACTIVE_ATTACK = 0xffff0000;
/** `COLOR_PARTY`. */
export const COLOR_PARTY = 0xff70f070;
/** `COLOR_PLAYER`. */
export const COLOR_PLAYER = 0xffffffff;
/** `COLOR_GUILDCOMBAT`: the own guild on the guild-war map. */
export const COLOR_GUILDCOMBAT = 0xff00ff00;
/** `COLOR_GUILDMASTER`. */
export const COLOR_GUILDMASTER = 0xffffff99;

/** The colours the server configures (`tables.pk`, `gameplay.*`). */
export interface NameColors {
  /** `prj.m_PKSetting.dwGeneralColor`. */
  general: number;
  /** `dwReadyColor` (pink / PK-ready). */
  ready: number;
  /** `dwChaoColor` (chaotic). */
  chao: number;
  /** `gameplay.gm_name_color` (default `0xFFFF4500`). */
  gm: number;
  /** `gameplay.admin_name_color`. */
  admin: number;
}

export const DEFAULT_NAME_COLORS: NameColors = {
  general: 0xffffffff,
  ready: 0xffffa8cb,
  chao: 0xffff0000,
  gm: 0xffff4500,
  admin: 0xffff0000,
};

/** `"#FF4500"`, `"0xFFFF4500"` or `"#FFFF4500"` -> `0xAARRGGBB` (opaque when no alpha is given); `fallback` otherwise. */
export function parseArgb(text: string, fallback: number): number {
  const m = /^(?:#|0x)?([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.exec(text.trim());
  const hex = m?.[1];
  if (hex === undefined) return fallback;
  const v = Number.parseInt(hex, 16);
  return hex.length === 6 ? (0xff000000 | v) >>> 0 : v >>> 0;
}

/** The text suffixes of staff names (`gameplay.gm_title`, `gameplay.admin_title`). */
export interface NameTitles {
  gm: string;
  admin: string;
}

export const DEFAULT_NAME_TITLES: NameTitles = { gm: '[Gm]', admin: '[Admin]' };

/** The name-related switches of `RenderOptions` and the feature config. */
export interface NameOptions {
  /** `nameSelf` (`g_Option.m_nPlayerName`). */
  nameSelf: boolean;
  /** `nameOthers` (`m_nOtherPlayerName`). */
  nameOthers: boolean;
  /** `nameMonsters` (`m_nMonName`): monsters and NPCs. */
  nameMonsters: boolean;
  /** `hidePets` (`m_OffPets`). */
  hidePets: boolean;
  /** `features.guild_color` (F-270). */
  guildColor: boolean;
}

export const DEFAULT_NAME_OPTIONS: NameOptions = {
  nameSelf: true,
  nameOthers: true,
  nameMonsters: true,
  hidePets: false,
  guildColor: false,
};

/** The distance limit of a name: 40 for monsters, 100 for players and peaceful NPCs. */
export function nameDistanceLimit(s: OverlaySubject): number {
  return s.kind === 'player' || (s.kind === 'npc' && s.peaceful) ? 100 : 40;
}

/**
 * `true` when the mover gets a name line: visible, not transparent, not render-skipped, the option of its category
 * on, pets not hidden, the own name not at a telescope level, within the distance limit.
 */
export function nameplateShown(s: OverlaySubject, viewer: OverlayViewer, o: NameOptions): boolean {
  if (s.culled || s.transparent || s.renderSkip) return false;
  if (s.isSelf && viewer.telescope !== 0) return false;
  if (s.distCamera >= nameDistanceLimit(s)) return false;
  if (s.isSelf) return o.nameSelf;
  if (s.kind === 'player') return o.nameOthers;
  if (s.kind === 'pet' && o.hidePets) return false;
  return o.nameMonsters;
}

/**
 * The colour of a player's name, first match: chaotic, pink, administrator, game master, general. Then (only when the
 * colour is still the general one) a party member is `COLOR_PARTY`; on the guild-war map two guilds decide instead.
 * KEEP-QUIRK: the staff-viewer colours the world pass prepares are overwritten for players.
 */
export function playerNameColor(
  s: OverlaySubject,
  viewer: OverlayViewer,
  colors: NameColors,
): number {
  let c: number;
  if (s.chaotic) c = colors.chao;
  else if (s.pink) c = colors.ready;
  else if (s.authorization >= AUTH_ADMINISTRATOR) c = colors.admin;
  else if (s.authorization >= AUTH_GAMEMASTER) c = colors.gm;
  else c = colors.general;
  if (c === colors.general && s.partyMember) c = COLOR_PARTY;
  if (viewer.guildWar && viewer.ownGuildId !== null && s.guild) {
    c = s.guild.id === viewer.ownGuildId ? COLOR_GUILDCOMBAT : COLOR_PLAYER;
  }
  return c >>> 0;
}

/** `"[title] name"` and the staff suffix (`" [Admin]"` from ADMINISTRATOR on, `" [Gm]"` from GAMEMASTER on). */
export function playerLabel(s: OverlaySubject, titles: NameTitles): string {
  let text = s.title === '' ? s.name : `[${s.title}] ${s.name}`;
  if (s.authorization >= AUTH_ADMINISTRATOR) text += ` ${titles.admin}`;
  else if (s.authorization >= AUTH_GAMEMASTER) text += ` ${titles.gm}`;
  return text;
}

/**
 * The colour of a guild name, in this order: white; with `features.guild_color` by level (>= 50 `0xFFED2939`, >= 40 `0xFFA020F0`, >= 30
 * `0xFF98FF98`, >= 20 `0xFFEE82EE`, >= 10 `0xFF00FFFF`, >= 5 `0xFF999999`); the guild's master gold; otherwise the colour of the guild-war
 * map (`mapColor`: the own guild green, every other white); and a siege defender (`defender`, `0xFFFEA8FF`) overrides all of it, in any world.
 */
export function guildNameColor(
  g: SubjectGuild,
  o: NameOptions,
  mapColor: number | null,
  defender = false,
): number {
  let c = COLOR_PLAYER;
  if (o.guildColor) {
    if (g.level >= 50) c = 0xffed2939;
    else if (g.level >= 40) c = 0xffa020f0;
    else if (g.level >= 30) c = 0xff98ff98;
    else if (g.level >= 20) c = 0xffee82ee;
    else if (g.level >= 10) c = 0xff00ffff;
    else if (g.level >= 5) c = 0xff999999;
  }
  if (g.master) c = COLOR_GUILDMASTER;
  else if (mapColor !== null) c = mapColor;
  if (defender) c = COLOR_SIEGE_DEFENDER;
  return c >>> 0;
}

/** Everything the layout needs besides the subject. */
export interface NameplateContext {
  viewer: OverlayViewer;
  options: NameOptions;
  colors: NameColors;
  titles: NameTitles;
  meter: TextMeter;
  /** The textures of the guild marks (the master / hero icon, the logo, the siege marks); absent: none are laid out. */
  assets?: OverlayAssets;
}

/** The anchor of the frame in whole pixels. */
export interface AnchorPx {
  x: number;
  y: number;
}

/** The dialog emoticon index of the stun icon (`DLGEMOT_STUN`). */
export const DLGEMOT_STUN = 6;
export const DIALOG_EMOTICON_ATLAS = 'theme/texdialogemoticon.inc';
export const CHAT_EMOTICON_ATLAS = 'theme/texchatemoticon.inc';

/**
 * The stun icon above a player (`RenderChrState`): the dialog emoticon `DLGEMOT_STUN` at `(even(anchor.x - 16), anchor.y
 * - 32)`, alpha 220. Independent of the name options and the name distance.
 */
export function stunIcon(s: OverlaySubject, anchor: AnchorPx): DrawItem | null {
  if (s.kind !== 'player' || !s.stun || s.culled) return null;
  return {
    kind: 'sprite',
    atlas: DIALOG_EMOTICON_ATLAS,
    index: DLGEMOT_STUN,
    x: even(Math.trunc(anchor.x - 16)),
    y: Math.trunc(anchor.y - 32),
    alpha: 220,
  };
}

/**
 * The name lines of one mover (nothing when {@link nameplateShown} says no). `anchor` is the head projection in CSS
 * pixels. Order: structure / disguise line, the name, the guild line.
 */
export function layoutNameplate(
  s: OverlaySubject,
  anchor: AnchorPx,
  c: NameplateContext,
): DrawItem[] {
  if (!nameplateShown(s, c.viewer, c.options)) return [];
  const font = OVERLAY_FONTS.world;
  const out: DrawItem[] = [];
  const ax = even(Math.trunc(anchor.x));
  const ay = Math.trunc(anchor.y);
  const centred = (text: string, dx: number, dy: number, color: number): TextItem2 => ({
    kind: 'text',
    font,
    text,
    x: ax - Math.trunc(c.meter.width(font, text) / 2) - dx,
    y: ay - dy,
    color,
  });

  if (s.kind === 'player') {
    let text = playerLabel(s, c.titles);
    let color = playerNameColor(s, c.viewer, c.colors);
    // a game master wearing a crown or a dwarpet shows the disguise instead (no `-3`)
    let dx = 3;
    if (s.authorization >= AUTH_GAMEMASTER && s.disguise === 'crown') {
      text = s.propName;
      color = COLOR_NPC;
      dx = 0;
    } else if (s.authorization >= AUTH_GAMEMASTER && s.disguise === 'dwarpet') {
      out.push(centred(`[${s.propName}]`, 0, 35, COLOR_NPC));
      color = COLOR_NPC;
      dx = 0;
    }
    const plate = centred(text, dx, 20, color);
    out.push(plate);
    // a game master with a text disguise shows no icons, no guild line, no logo (`bSkip`)
    if (!s.textDisguise) {
      const label = playerLabel(s, c.titles);
      const guildName = s.guild?.name ?? '';
      const extents: MarkExtents = {
        labelW: c.meter.width(font, label),
        guildW: s.guild ? c.meter.width(font, guildName) : 0,
        labelBytes: byteLength(label),
        guildBytes: byteLength(guildName),
      };
      const marks = c.assets
        ? layoutGuildMarks(s, ax, ay, extents, c.viewer.siege, c.assets)
        : { icon: null, logo: [] };
      // the master / hero icon is drawn before the guild text
      if (marks.icon) out.push(marks.icon);
      if (s.guild) {
        const mapColor =
          c.viewer.guildWar && c.viewer.ownGuildId !== null
            ? s.guild.id === c.viewer.ownGuildId
              ? COLOR_GUILDCOMBAT
              : COLOR_PLAYER
            : null;
        const color2 = guildNameColor(
          s.guild,
          c.options,
          mapColor,
          isSiegeDefender(s, c.viewer.siege),
        );
        // an unnamed guild draws no text; its logo and marks still follow the rules
        if (guildName !== '') {
          out.push({
            kind: 'text',
            font,
            text: guildName,
            x: ax - Math.trunc(extents.guildW / 2) - 3,
            y: ay - 32,
            color: color2,
          });
        }
        out.push(...marks.logo);
      }
    }
    return out;
  }

  // monsters, NPCs and pets: only with the monster-name option (checked by `nameplateShown`)
  let dx = 3;
  if (s.structure !== null) {
    out.push(centred(`[${s.structure}]`, 0, 35, COLOR_NPC));
    dx = 0;
  }
  const color = s.peaceful ? COLOR_NPC : s.activeAttack ? COLOR_ACTIVE_ATTACK : COLOR_MONSTER;
  out.push(centred(s.name, dx, 20, color));
  return out;
}

type TextItem2 = Extract<DrawItem, { kind: 'text' }>;
