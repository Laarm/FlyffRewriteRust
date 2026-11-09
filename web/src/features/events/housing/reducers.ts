// The pure reducers of the housing slice (Plan/45 §28.10, §28.11; ref/09b §3.16; `DPClient.cpp:18533-18712`; WP-CFEAT-46): one message in, the new state and a list of effects out (data: chat lines, window refreshes,
// player-data queries and the visitor notification) that the module carries out. Every message applies whether or not a window is open.

import { APP_HOUSING, APP_MINIROOM_LIST } from '../../../gen/defs/resdata';
import { IK2_PAPERING, IK3_CARPET, IK3_WALLPAPER } from '../../../gen/defs/itemkind';
import {
  TID_GAME_HOUSING_ADDITION,
  TID_GAME_HOUSING_DEMOUNT,
  TID_GAME_HOUSING_INSTALL,
  TID_GAME_HOUSING_INVIATION,
  TID_GAME_HOUSING_INVIATION_CANCEL,
  TID_GAME_HOUSING_TIMEOUT,
} from '../../../gen/defs/text';
import { type HousingRow, type HousingSlice, NULL_ITEM, WORLD_MINIROOM } from './slice';

/** What a reducer needs to know about an item: its name and kinds (`prj.GetItemProp`), `undefined` for an unknown id. */
export interface PropFacts {
  readonly name: string;
  readonly kind2: number;
  readonly kind3: number;
}

export interface HousingEnv {
  readonly prop: (itemId: number) => PropFacts | undefined;
  /** The name of a character in the player-data cache, `null` while unknown. */
  readonly nameOf: (idPlayer: number) => string | null;
}

export type HousingEffect =
  | { readonly kind: 'chat'; readonly tid: number; readonly args: readonly (string | number)[] }
  | { readonly kind: 'refresh'; readonly app: number }
  | { readonly kind: 'notifyVisitors' }
  | { readonly kind: 'needAll'; readonly ids: readonly number[] };

export interface Reduced {
  readonly state: HousingSlice;
  readonly effects: readonly HousingEffect[];
}

/** The messages of the slice (the S2C messages plus the two client-side events). */
export type HousingMsg =
  | {
      readonly kind: 'allInfo';
      readonly items: readonly HousingRow[];
      readonly visitors: readonly number[];
    }
  | { readonly kind: 'furnitureList'; readonly info: HousingRow; readonly add: boolean }
  | { readonly kind: 'setup'; readonly info: HousingRow }
  | { readonly kind: 'papering'; readonly itemId: number; readonly setup: boolean }
  | { readonly kind: 'visitAllow'; readonly target: number; readonly allow: boolean }
  | { readonly kind: 'visitable'; readonly owners: readonly number[] }
  /** The visit request left (`SendHousingVisitRoom`): `owner === self` marks the room as the player's own. */
  | { readonly kind: 'visitSent'; readonly owner: number; readonly self: number }
  /** The room list window opened (`m_vecVisitable.clear()`). */
  | { readonly kind: 'clearVisitable' }
  /** `Replace` into `worldId`. */
  | { readonly kind: 'replace'; readonly worldId: number };

const NO_EFFECTS: readonly HousingEffect[] = [];

/** `HousingAllInfo`, `HousingFurnitureList`, ... in one function; the table test pins every row. */
export function reduceHousing(state: HousingSlice, msg: HousingMsg, env: HousingEnv): Reduced {
  switch (msg.kind) {
    case 'allInfo':
      return {
        state: { ...state, items: [...msg.items], visitors: new Set(msg.visitors) },
        effects: [{ kind: 'refresh', app: APP_HOUSING }],
      };
    case 'furnitureList': {
      // add appends (an existing row of that item id is replaced: defensive, the server never lists an id twice), else the row of the item id is dropped
      const without = state.items.filter((r) => r.itemId !== msg.info.itemId);
      const items = msg.add ? [...without, msg.info] : without;
      const effects: HousingEffect[] = [{ kind: 'refresh', app: APP_HOUSING }];
      const prop = env.prop(msg.info.itemId);
      if (prop) {
        effects.push({
          kind: 'chat',
          tid: msg.add ? TID_GAME_HOUSING_ADDITION : TID_GAME_HOUSING_TIMEOUT,
          args: [prop.name],
        });
      }
      return { state: { ...state, items }, effects };
    }
    case 'setup': {
      const items = state.items.map((r) => (r.itemId === msg.info.itemId ? msg.info : r));
      const effects: HousingEffect[] = [{ kind: 'refresh', app: APP_HOUSING }];
      const prop = env.prop(msg.info.itemId);
      if (prop) {
        effects.push({
          kind: 'chat',
          tid: msg.info.setup ? TID_GAME_HOUSING_INSTALL : TID_GAME_HOUSING_DEMOUNT,
          args: [prop.name],
        });
      }
      return { state: { ...state, items }, effects };
    }
    case 'papering': {
      if (msg.itemId === NULL_ITEM && !msg.setup) {
        return { state: { ...state, papering: { wall: null, floor: null } }, effects: NO_EFFECTS };
      }
      const prop = env.prop(msg.itemId);
      if (prop?.kind2 !== IK2_PAPERING) return { state, effects: NO_EFFECTS };
      const value = msg.setup ? msg.itemId : null;
      if (prop.kind3 === IK3_WALLPAPER) {
        return {
          state: { ...state, papering: { ...state.papering, wall: value } },
          effects: NO_EFFECTS,
        };
      }
      if (prop.kind3 === IK3_CARPET) {
        return {
          state: { ...state, papering: { ...state.papering, floor: value } },
          effects: NO_EFFECTS,
        };
      }
      return { state, effects: NO_EFFECTS };
    }
    case 'visitAllow': {
      const visitors = new Set(state.visitors);
      if (msg.allow) visitors.add(msg.target);
      else visitors.delete(msg.target);
      return {
        state: { ...state, visitors },
        effects: [
          {
            kind: 'chat',
            tid: msg.allow ? TID_GAME_HOUSING_INVIATION : TID_GAME_HOUSING_INVIATION_CANCEL,
            args: [env.nameOf(msg.target) ?? ''],
          },
          { kind: 'notifyVisitors' },
        ],
      };
    }
    case 'visitable':
      return {
        state: { ...state, visitable: [...msg.owners] },
        effects: [
          { kind: 'needAll', ids: msg.owners },
          { kind: 'refresh', app: APP_MINIROOM_LIST },
        ],
      };
    case 'visitSent': {
      const myRoom = msg.owner === msg.self;
      return { state: state.myRoom === myRoom ? state : { ...state, myRoom }, effects: NO_EFFECTS };
    }
    case 'clearVisitable':
      return {
        state: state.visitable.length === 0 ? state : { ...state, visitable: [] },
        effects: NO_EFFECTS,
      };
    case 'replace':
      if (msg.worldId === WORLD_MINIROOM || !state.myRoom) return { state, effects: NO_EFFECTS };
      return { state: { ...state, myRoom: false }, effects: NO_EFFECTS };
  }
}
