// `eventStore` (Plan/45 §28.0; WP-CFEAT-07 flags, WP-CFEAT-41 siege): the event slices of the client. Registry store `events`, scope `session` (the flags and the HUD timer survive a world
// change; the siege slice is reset by the siege module when the character leaves the world). The M7 event tasks add their slices (`gc1to1`, `secretRoom`, `lord`, `tax`, `rainbow`, `quiz`,
// `housing`, `guildHouse`) to `EventState` and `initialEventState()`; nothing below depends on them.
//
//   flags      the 1024 event bytes of the server (`FlyffEvent`), one of them changed by `SetLocalEvent`; `isEventOn( id ) = flags[ id ] != 0` (`g_eLocal.GetState`)
//   hudTimer   the ONE timer line of the world HUD (`CWndWorld::m_szGuildCombatStr` + `m_dwGuildCombatTime`); siege, the 1:1 combat and the secret room write it, the last writer wins
//   siege      the guild siege (`g_GuildCombatMng` and the lists of `CWndWorld`)

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';
// ---- lord (ui-lord, M7)
import { initialLord, type LordSlice } from './lord/slice';
// ---- housing / guild house (ui-house-quiz)
import { initialGuildHouse, type GuildHouseSlice } from './guildHouse/slice';
import { type HousingSlice, initialHousing } from './housing/slice';
// ---- rainbow race and quiz (ui-rainbow)
import { initialQuiz, type QuizSlice } from './quiz/state';
import { initialRainbow, type RainbowSlice } from './rainbow/state';
// ---- M7 GC 1:1 and attendance (task ui-minigames)
import { type Gc1to1Slice, initialGc1to1 } from './gc1to1/slice';

/** `MAX_EVENT`: the size of the flag array. */
export const EVENT_FLAGS = 1024;

/** The writer of the HUD timer line. */
export type HudTimerSource = 'siege' | 'gc1to1' | 'secretRoom';

/** `m_szGuildCombatStr` + `m_dwGuildCombatTime` (an end time in the clock of the client, `ctx.now()`). */
export interface HudTimer {
  readonly label: string;
  /** The end, in ms of `ctx.now()`. */
  readonly endMs: number;
  readonly src: HudTimerSource;
  /** The 1:1 combat shows "label + `TID_GAME_GUILDCOMBAT_1TO1_WAITSEC`( seconds )" (WP-CFEAT-45). */
  readonly waitSec?: boolean;
}

/** `CGuildCombat` siege states (`GcGcState.state`). */
export const SIEGE_STATE = { Open: 0, Close: 1, War: 2, CompleteClose: 3, GmClose: 4 } as const;

/** The war states (`GcGcState.war_state`, `CGuildCombat::NOTENTER_STATE` ...). */
export const WAR_STATE = {
  NotEnter: 100,
  NotEnterCount: 101,
  Enter: 102,
  Maintenance: 103,
  WarWar: 104,
  WarClose: 105,
  WarCloseWait: 106,
  WarTeleport: 107,
} as const;

/** The world of the siege (`WI_WORLD_GUILDWAR`). */
export const WORLD_GUILDWAR = 202;

/** `GC_WINGUILD`. */
export interface WinGuild {
  readonly index: number;
  readonly guildId: number;
  readonly streak: number;
}

/** One admitted guild of `GC_WARPLAYERLIST`: its defender and its selected players (`status` 1 = fighting). */
export interface RosterGuild {
  readonly defender: number;
  readonly players: readonly { readonly id: number; readonly fighting: boolean }[];
}

/** An entry of `GC_GUILDPRECEDENCE` (an entry with `send = false` keeps `recv = false`). */
export interface GuildRankEntry {
  readonly name: string;
  readonly points: number;
  readonly recv: boolean;
}

export interface SiegeSlice {
  /** 0 Open, 1 Close, 2 War, 3 CompleteClose, 4 GmClose (initial 1: `CLOSE_STATE`). */
  readonly state: number;
  /** 100 .. 107 (initial 100). */
  readonly warState: number;
  /** `g_GuildCombatMng.m_bRequest`: the own guild has an application. */
  readonly requested: boolean;
  readonly winGuild?: WinGuild;
  /** `g_GuildCombatMng.m_uBestPlayer`. */
  readonly bestPlayer?: number;
  /** `m_mapGC_GuildStatus`: the admitted guilds with their selected players, whose members the HUD roster needs. */
  readonly roster: readonly RosterGuild[];
  /** `m_vecGuildCombat_GuildStatus`: the players of the own guild with their lives, in the order sent. */
  readonly ownLives: readonly { readonly id: number; readonly lives: number }[];
  /** `nJoinCount` of `GC_GUILDSTATUS` (`max_war_player`); the row with that index (not 0) is the next to enter. */
  readonly joinIndex: number;
  /** `m_mmapGuildCombat_GuildPrecedence`, in arrival order. */
  readonly guildRank: readonly GuildRankEntry[];
  /** `m_mmapGuildCombat_PlayerPrecedence`: one list per guild, in arrival order. */
  readonly playerRank: readonly (readonly { readonly id: number; readonly points: number }[])[];
  /** `CMover::m_nGuildCombatState` of the movers that told (`GC_USERSTATE`); 1 = fighting. */
  readonly userState: ReadonlyMap<number, number>;
}

// ---- secret room and tax (ui-secret-tax, M7)

/** One bidding guild of `SecretRoomInfo.GuildList` (`SECRETROOM_GUILD`): its war state, kills and the line-up (the receiver's own guild only). */
export interface SecretRoomGuild {
  readonly guildId: number;
  /** 0 FAILED, 1 NORMAL, 2 MIDBOSS, 3 BOSS, 4 WIN. */
  readonly war: number;
  readonly kills: number;
  readonly lineup: readonly number[];
}

/** The secret room as the client keeps it (`CSecretRoomMng`, Plan/45 §28.0, §28.6). */
export interface SecretRoomSlice {
  /** `SecretRoomMngState.state`: the manager states 1 .. 6 and the continent states 101 .. 104 share the field (initial 1). */
  readonly state: number;
  /** The continent of the last `GuildList` (15 East, 240 West; 0 none): the legacy `m_pSRCont`. */
  readonly continent: number;
  /** The bidding guilds of that continent in rank order (`KillCount.index` and `WarState.index` are indexes into this list). */
  readonly guilds: readonly SecretRoomGuild[];
  /** The kills needed per war state (`monster_totals`). */
  readonly totals: ReadonlyMap<number, number>;
}

/** One continent of `TaxAllInfo`: the owner guild (`null` = none) and the rate in percent per tax kind (0 sales, 1 purchase, 2 admission). */
export interface TaxContinentRow {
  readonly cont: number;
  readonly owner: number | null;
  readonly rates: ReadonlyMap<number, number>;
}

/** The tax tables of every continent (`CTax`, Plan/45 §28.3). */
export interface TaxSlice {
  readonly continents: readonly TaxContinentRow[];
}

// ---- M7 GC 1:1 and attendance (task ui-minigames)

/** `CWebBox::m_nEventFlag` + the coupon notice (`CallTheRoll`, `EventCoupon`; WP-CFEAT-45): one bit per attendance day and the last coupon count. */
export interface AttendanceSlice {
  readonly bits: bigint;
  readonly coupons: number;
}

export function initialAttendance(): AttendanceSlice {
  return { bits: 0n, coupons: 0 };
}

export function initialSecretRoom(): SecretRoomSlice {
  return { state: 1, continent: 0, guilds: [], totals: new Map() };
}

export function initialTax(): TaxSlice {
  return { continents: [] };
}

export interface EventState {
  readonly flags: Uint8Array;
  readonly hudTimer: HudTimer | null;
  readonly siege: SiegeSlice;
  // ---- lord (ui-lord, M7)
  readonly lord: LordSlice;
  // ---- housing / guild house (ui-house-quiz)
  readonly housing: HousingSlice;
  readonly guildHouse: GuildHouseSlice;
  // ---- secret room and tax (ui-secret-tax, M7)
  readonly secretRoom: SecretRoomSlice;
  readonly tax: TaxSlice;
  // ---- rainbow race and quiz (ui-rainbow)
  readonly rainbow: RainbowSlice;
  readonly quiz: QuizSlice;
  // ---- M7 GC 1:1 and attendance (task ui-minigames)
  readonly gc1to1: Gc1to1Slice;
  readonly attendance: AttendanceSlice;
}

export function initialSiege(): SiegeSlice {
  return {
    state: SIEGE_STATE.Close,
    warState: WAR_STATE.NotEnter,
    requested: false,
    roster: [],
    ownLives: [],
    joinIndex: 0,
    guildRank: [],
    playerRank: [],
    userState: new Map(),
  };
}

export function initialEventState(): EventState {
  return {
    flags: new Uint8Array(EVENT_FLAGS),
    hudTimer: null,
    siege: initialSiege(),
    // ---- lord (ui-lord, M7)
    lord: initialLord(),
    // ---- housing / guild house (ui-house-quiz)
    housing: initialHousing(),
    guildHouse: initialGuildHouse(),
    // ---- secret room and tax (ui-secret-tax, M7)
    secretRoom: initialSecretRoom(),
    tax: initialTax(),
    // ---- rainbow race and quiz (ui-rainbow)
    rainbow: initialRainbow(),
    quiz: initialQuiz(),
    // ---- M7 GC 1:1 and attendance (task ui-minigames)
    gc1to1: initialGc1to1(),
    attendance: initialAttendance(),
  };
}

export type EventStore = StoreApi<EventState>;

/** The store of a registry (created on first use). */
export function defineEventStore(registry: StoreRegistry): EventStore {
  const existing = registry.get('events');
  if (existing) return existing as unknown as EventStore;
  return registry.defineStore<EventState>({
    name: 'events',
    scope: 'session',
    owner: '45',
    initial: initialEventState,
  });
}
