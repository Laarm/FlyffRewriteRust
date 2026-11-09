// The duel message reducers (Plan/45 §25.1, ref/09a §5.8, ref/09b `OnSetDuel`; `src:Neuz/DPClient.cpp:4815-5115`, `15517-15552`;
// WP-CFEAT-36): each one writes the duel fields of the movers it names into the {@link DuelBook} and returns the effects the module
// carries out (system chat lines, the result window). Pure functions over a narrow view of the client, so every row of the legacy
// handlers has a table test.
//
// The legacy handlers all start with `IsValidObj(pTarget)`: a message about a player the client does not know (out of sight) changes
// nothing. Kept.

import {
  TID_GAME_DUELCANCEL,
  TID_GAME_DUELNO,
  TID_GAME_DUELSTART,
  TID_GAME_PDCANCEL,
  TID_GAME_PDLOSE,
  TID_GAME_PDSTART,
  TID_GAME_PDVICTORY,
  TID_GAME_PVPLEAVE,
} from '../../../gen/defs/text';
import { RA_FIGHT } from '../../../gen/defs/define';
import { NULL_ID } from '../store';
import { DUEL_STATE_COUNTDOWN, DUEL_STATE_FIGHT, DUEL_STATE_OUT, type DuelBook } from './book';

/** What the reducers know about the client. */
export interface DuelWorld {
  /** The object id of the own mover (`g_pPlayer->GetId()`), 0 before the spawn. */
  selfObj(): number;
  /** `g_Party.m_uPartyId`: the own party (0 none). */
  selfParty(): number;
  /** `prj.GetUserByID(idPlayer)`: the known player with this character id. */
  playerById(idPlayer: number): { objId: number; name: string } | null;
  /** `prj.GetMover(objId)` + `IsValidObj`: the known mover's name, `null` when the client does not know it. */
  moverName(objId: number): string | null;
}

/** Something the module does after a reducer. */
export type DuelEffect =
  /** `PutString(format(tid, args), colour of tid)`. */
  | { readonly kind: 'text'; readonly tid: number; readonly args: readonly string[] }
  /** `new CWndDuelResult` (the confirm window is deleted first). */
  | { readonly kind: 'result'; readonly win: boolean; readonly name: string };

const text = (tid: number, ...args: string[]): DuelEffect => ({ kind: 'text', tid, args });

// ---- solo duels ------------------------------------------------------------------------------------------------------------------

/** `OnDuelStart`: `idTarget` is a character id (`uidTarget`). */
export function onDuelStart(
  book: DuelBook,
  w: DuelWorld,
  idTarget: number,
  flag: number,
): DuelEffect[] {
  const target = w.playerById(idTarget);
  if (!target) return [];
  const me = w.selfObj();
  switch (flag) {
    case 0:
      // the duel is accepted: both sides know each other; the countdown runs (the state of the target is not set, like the original)
      book.set(me, { duel: 1, other: target.objId, state: DUEL_STATE_COUNTDOWN });
      book.set(target.objId, { duel: 1, other: me });
      return [];
    case 1:
      book.set(me, { state: DUEL_STATE_FIGHT });
      return [text(TID_GAME_DUELSTART, target.name)];
    case 2:
      book.clearSolo(me);
      book.clearSolo(target.objId);
      return [];
    case 3:
    case 4:
      // the original also wants the opponent alive; here the loser of a duel really dies, the winner must still get the window
      return [{ kind: 'result', win: flag === 3, name: target.name }];
    default:
      return [];
  }
}

/** `OnDuelNo` / `OnDuelPartyNo`: the decliner is an object id. */
export function onDuelNo(w: DuelWorld, decliner: number): DuelEffect[] {
  const name = w.moverName(decliner);
  return name === null ? [] : [text(TID_GAME_DUELNO, name)];
}

/** `OnDuelCancel`: the duel of the own player and of the named opponent ends (`DuelCancel{NULL_ID}` clears only the own). */
export function onDuelCancel(book: DuelBook, w: DuelWorld, target: number): DuelEffect[] {
  book.clearSolo(w.selfObj());
  if (target !== NULL_ID && w.moverName(target) !== null) book.clearSolo(target);
  return [text(TID_GAME_DUELCANCEL)];
}

/** `OnSetDuel`: the four fields of one mover as the server sees them. */
export function onSetDuel(
  book: DuelBook,
  w: DuelWorld,
  m: {
    readonly moverObjId: number;
    readonly duel: number;
    readonly duelState: number;
    readonly idDuelOther: number;
    readonly idDuelParty: number;
  },
): DuelEffect[] {
  const name = w.moverName(m.moverObjId);
  if (name === null) return [];
  const out: DuelEffect[] = [];
  const cur = book.get(m.moverObjId);
  if (m.moverObjId === w.selfObj() && cur.duel === 1 && m.duel === 0) {
    // the own solo duel ends: the opponent is cleared too (the server sends `SetDuel` to the players themselves only)
    if (cur.other !== NULL_ID && w.moverName(cur.other) !== null) book.clearSolo(cur.other);
  }
  if (m.duel === 2 && m.duelState === DUEL_STATE_OUT) out.push(text(TID_GAME_PVPLEAVE, name));
  book.set(m.moverObjId, {
    duel: m.duel,
    state: m.duelState,
    other: m.idDuelOther,
    party: m.idDuelParty,
  });
  return out;
}

// ---- party duels -----------------------------------------------------------------------------------------------------------------

/** `OnDuelPartyStart`: the members are object ids; `flag` 0 = countdown, 1 = fighting. */
export function onDuelPartyStart(
  book: DuelBook,
  w: DuelWorld,
  m: {
    readonly idParty: number;
    readonly partyName: string;
    readonly members: readonly number[];
    readonly flag: number;
  },
): DuelEffect[] {
  const myParty = w.selfParty();
  for (const id of m.members) {
    if (w.moverName(id) === null) continue;
    // the opposing party gets MY party id as the party it duels
    book.set(id, { duel: 2, party: myParty });
    if (m.flag === 1) book.set(id, { state: DUEL_STATE_FIGHT });
    else if (m.flag === 0) book.set(id, { state: DUEL_STATE_COUNTDOWN });
  }
  const me = w.selfObj();
  book.set(me, { duel: 2, party: m.idParty });
  if (m.flag === 1) {
    book.set(me, { state: DUEL_STATE_FIGHT });
    return [text(TID_GAME_PDSTART, m.partyName)];
  }
  if (m.flag === 0) book.set(me, { state: DUEL_STATE_COUNTDOWN });
  return [];
}

/** `OnDuelPartyCancel` (`win` = null) and `OnDuelPartyResult`: the players are character ids. */
export function onDuelPartyEnd(
  book: DuelBook,
  w: DuelWorld,
  ids: readonly number[],
  win: boolean | null,
): DuelEffect[] {
  for (const id of ids) {
    const p = w.playerById(id);
    if (p) book.clearParty(p.objId);
  }
  book.clearParty(w.selfObj());
  if (win === null) return [text(TID_GAME_PDCANCEL)];
  return [text(win ? TID_GAME_PDVICTORY : TID_GAME_PDLOSE)];
}

// ---- spawn -----------------------------------------------------------------------------------------------------------------------

/**
 * `OnAddObj` for a player: the fields the server put into the spawn, and the markers of the running duel (a player of the opposing
 * party is a party-duel enemy, the solo opponent is the solo enemy).
 */
export function onSpawnPlayer(
  book: DuelBook,
  w: DuelWorld,
  s: {
    readonly objId: number;
    readonly duel: number;
    readonly idParty: number;
    readonly idDuelParty: number;
  },
): void {
  if (s.duel !== 0 || s.idDuelParty !== 0) {
    book.set(s.objId, { duel: s.duel, party: s.idDuelParty });
  }
  const own = book.get(w.selfObj());
  if (own.party !== 0 && s.idParty === own.party) {
    book.set(s.objId, { duel: 2, party: w.selfParty() });
  } else if (own.other === s.objId && own.duel === 1) {
    book.set(s.objId, { duel: 1, other: w.selfObj() });
  }
}

// ---- the enemy markers -----------------------------------------------------------------------------------------------------------

/**
 * The movers that carry the "enemy" marker (`RenderFlag(ENEMY_PVP)` / `ENEMY_PVP_PARTY`, `XI_GEN_PVP_FLAG01` above the head, 41): the
 * solo opponent of a solo duel and every known member of the opposing party of a party duel.
 */
export function enemyMarkers(
  book: DuelBook,
  selfObj: number,
  selfParty: number,
): { readonly solo: readonly number[]; readonly party: readonly number[] } {
  const own = book.get(selfObj);
  const solo: number[] = [];
  const party: number[] = [];
  if (own.duel === 1 && own.other !== NULL_ID) solo.push(own.other);
  if (own.duel === 2 && own.party !== 0 && selfParty !== 0) {
    for (const id of book.ids()) {
      if (id === selfObj) continue;
      const f = book.get(id);
      if (f.duel === 2 && f.party === selfParty) party.push(id);
    }
  }
  return { solo, party };
}

// ---- the menu --------------------------------------------------------------------------------------------------------------------

/**
 * The player menu offers "Duel - 1 vs 1" when the target lives and both stand in an `RA_FIGHT` region
 * (`g_pPlayer->IsRegionAttr( RA_FIGHT ) && pTarget->IsRegionAttr( RA_FIGHT )`, `WndWorld.cpp:6781-6806`). `targetAttr` is `null` when the
 * client cannot tell where the target stands.
 */
export function duelMenuAllowed(
  ownAttr: number,
  targetAttr: number | null,
  targetDead: boolean,
): boolean {
  if (targetDead || targetAttr === null) return false;
  return (ownAttr & RA_FIGHT) === RA_FIGHT && (targetAttr & RA_FIGHT) === RA_FIGHT;
}
