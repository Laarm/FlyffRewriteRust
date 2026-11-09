// The pure reducers of the guild-house slice (Plan/45 §28.11; `GuildHouse.cpp:752-970`, `DPClient.cpp:19135-19185`; WP-CFEAT-46): one message in, the new state and a list of effects out (chat lines, window refreshes, the end of a
// running placement). Every message applies whether or not a window is open; the lists are rebuilt from the slice by the storage window.

import { APP_GH_FURNITURE_STORAGE } from '../../../gen/defs/resdata';
import { IK3_TELEPORTER } from '../../../gen/defs/itemkind';
import {
  TID_GAME_GUILDHOUSE_FUR_DISMANTLE_SUCCESS,
  TID_GAME_GUILDHOUSE_FUR_INSTALL_SUCCESS,
  TID_GAME_GUILDHOUSE_FUR_REINSTALL_SUCCESS,
} from '../../../gen/defs/text';
import type { PropFacts } from '../housing/reducers';
import {
  type GhHouse,
  type GhRow,
  GH_OP,
  type GuildHouseSlice,
  initialGuildHouse,
  NULL_OBJ,
} from './slice';

export type { PropFacts };

export interface GuildHouseEnv {
  readonly prop: (itemId: number) => PropFacts | undefined;
  /** `IsAuthHigher( AUTH_GAMEMASTER )`: a GM sees the rest points as a chat line. */
  readonly isGm: boolean;
}

export type GhEffect =
  | { readonly kind: 'chat'; readonly tid: number; readonly args: readonly (string | number)[] }
  | { readonly kind: 'chatText'; readonly text: string; readonly color: number }
  /** The storage window re-reads the rows (scroll kept, the selected row found again). */
  | { readonly kind: 'refresh'; readonly app: number }
  /** A guild placement in progress ends (the house is gone, the guild changed). */
  | { readonly kind: 'endDeploy' };

export interface Reduced {
  readonly state: GuildHouseSlice;
  readonly effects: readonly GhEffect[];
}

export type GhMsg =
  | { readonly kind: 'allInfo'; readonly bankEnabled: boolean; readonly house: GhHouse | null }
  | { readonly kind: 'packet'; readonly op: number; readonly index: number; readonly info: GhRow }
  | { readonly kind: 'remove' }
  | { readonly kind: 'restPoint'; readonly restPoint: number }
  /** The character left the world or the own guild changed: the slice starts empty. */
  | { readonly kind: 'reset' }
  /** `m_dwSelectedObjID` (`null` = `NULL_ID`). */
  | { readonly kind: 'select'; readonly objId: number | null }
  /** `m_iMode`. */
  | { readonly kind: 'mode'; readonly mode: 0 | 2 | 4 };

const NONE: readonly GhEffect[] = [];
const REFRESH: GhEffect = { kind: 'refresh', app: APP_GH_FURNITURE_STORAGE };
/** `0xffffff99`: the rest-point line of a game master. */
export const REST_POINT_COLOR = 0xffffff99;

/** The percent of the rest gauge: `points * 100 / 1 000 000` (whole). */
export function restPercentOf(points: number): number {
  return Math.trunc((points * 100) / 1_000_000);
}

/** A row replaced at `index`: only when it exists and holds the same item (`m_vecFntInfo[nIndex].dwItemId != gfi.dwItemId` refuses). */
function replaceRow(house: GhHouse, index: number, info: GhRow): GhHouse {
  const cur = house.rows[index];
  if (cur?.itemId !== info.itemId) return house;
  const rows = house.rows.slice();
  rows[index] = info;
  return { ...house, rows };
}

export function reduceGuildHouse(state: GuildHouseSlice, msg: GhMsg, env: GuildHouseEnv): Reduced {
  switch (msg.kind) {
    case 'allInfo':
      // FIX: an absent house clears the slice (the original kept a previous guild's house)
      return {
        state: { ...state, furnitureChannel: msg.bankEnabled, house: msg.house },
        effects: [REFRESH],
      };
    case 'packet': {
      const house = state.house;
      if (house === null) return { state, effects: NONE };
      const { op, index, info } = msg;
      const effects: GhEffect[] = [];
      let next: GhHouse = house;
      switch (op) {
        case GH_OP.ListUp:
          next = { ...house, rows: [...house.rows, info] };
          break;
        case GH_OP.ListDrop:
          if (index >= 0 && index < house.rows.length) {
            next = { ...house, rows: house.rows.filter((_, i) => i !== index) };
          }
          break;
        case GH_OP.Setup:
        case GH_OP.Remove:
        case GH_OP.Reset: {
          // the success line first (the original printed it before the row guard), then the row like a REFRESH
          const prop = env.prop(info.itemId);
          if (prop) {
            effects.push({
              kind: 'chat',
              tid:
                op === GH_OP.Setup
                  ? TID_GAME_GUILDHOUSE_FUR_INSTALL_SUCCESS
                  : op === GH_OP.Remove
                    ? TID_GAME_GUILDHOUSE_FUR_DISMANTLE_SUCCESS
                    : TID_GAME_GUILDHOUSE_FUR_REINSTALL_SUCCESS,
              args: [prop.name],
            });
          }
          // `ResetFurnitureCtrl` refuses a teleporter row (KEEP)
          if (op === GH_OP.Reset && prop?.kind3 === IK3_TELEPORTER) break;
          next = replaceRow(house, index, info);
          break;
        }
        case GH_OP.Refresh:
          next = replaceRow(house, index, info);
          break;
        case GH_OP.Upkeep:
          next = { ...house, upkeepAt: info.expireAt };
          break;
        case GH_OP.Expired:
          next = {
            ...house,
            upkeepAt: 0,
            rows: house.rows.map((r) =>
              r.setup ? { ...r, setup: false, ctrlObjId: NULL_OBJ } : r,
            ),
          };
          break;
        default:
          return { state, effects: NONE };
      }
      effects.push(REFRESH);
      return { state: next === house ? state : { ...state, house: next }, effects };
    }
    case 'remove':
      return {
        state: { ...state, house: null, selectedObjId: null, mode: 0 },
        effects: [{ kind: 'endDeploy' }, REFRESH],
      };
    case 'restPoint': {
      const effects: GhEffect[] = env.isGm
        ? [
            {
              kind: 'chatText',
              text: `Rest Point : ${String(msg.restPoint)}`,
              color: REST_POINT_COLOR,
            },
          ]
        : [];
      return {
        state: { ...state, restPoint: msg.restPoint, restPercent: restPercentOf(msg.restPoint) },
        effects,
      };
    }
    case 'reset':
      return { state: initialGuildHouse(), effects: [{ kind: 'endDeploy' }, REFRESH] };
    case 'select':
      return {
        state: state.selectedObjId === msg.objId ? state : { ...state, selectedObjId: msg.objId },
        effects: NONE,
      };
    case 'mode':
      return {
        state: state.mode === msg.mode ? state : { ...state, mode: msg.mode },
        effects: NONE,
      };
  }
}
