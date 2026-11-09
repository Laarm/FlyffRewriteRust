// The `pvp` feature module, duel part (Plan/45 §25.1, §2.5; ref/09a §5.8, ref/99 §4.7; WP-CFEAT-36): the duel request window, the duel
// messages and texts, the countdown, the result window and the "Duel" entries of the player menu.
//
//   DuelRequestNtf / DuelPartyRequestNtf   (requester visible and alive) APP_DUEL_CONFIRM; Yes -> `closeTransactional('DuelAccepted')`
//                                          THEN `DuelYes` (solo), `DuelPartyYes` without the clean-up (party), No -> `DuelNoReq` / `DuelPartyNoReq`
//   DuelStart{target, flag}                0 accepted (markers, state 104), 1 fight (`TID_GAME_DUELSTART`), 2 clear, 3 win / 4 lose (result window)
//   DuelNoNtf, DuelCancel                  `TID_GAME_DUELNO(name)`, `TID_GAME_DUELCANCEL`
//   DuelPartyStart / Cancel / Result       `TID_GAME_PDSTART(party)`, `PDCANCEL`, `PDVICTORY` / `PDLOSE`
//   SetDuel, DuelCount                     the four duel fields of a mover (+ `TID_GAME_PVPLEAVE`), the 3-2-1 of the overlay
//
// The duel fields of every mover live in the {@link DuelBook}; the own fields are mirrored into `pvpStore`. Other features reach this
// part with `duelOf(ctx)` (`fieldsOf` for the attack rule and the enemy markers, `request` for a challenge).

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../../app/context';
import { entityInfoOf } from '../../../app/inputHost';
import { inputProvidersOf } from '../../../app/inputProviders';
import { combatRuntime } from '../../../game/combat';
import { OBJMSG } from '../../../game/combat/constants';
import { EntityKind, type EntityRecord } from '../../../game/entities';
import { peekRulesRuntime } from '../../../game/rules';
import { regionAt } from '../../../game/world/region';
import { MTI_HEAD, MTI_HIPKIDDING, MMI_DUEL, MMI_DUEL_PARTY } from '../../../gen/defs/neuz';
import { APP_PARTY_CONFIRM } from '../../../gen/defs/resdata';
import {
  TID_DIAG_0066,
  TID_DIAG_0067,
  TID_DIAG_DUELRESULT_LOSE,
  TID_DIAG_DUELRESULT_WIN,
} from '../../../gen/defs/text';
import type { SpawnPlayer, SpawnSelf } from '../../../gen/proto/flyff/v1/world_pb';
import { type FeatureContext, type FeatureModule, fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { formatTid, sysChat } from '../../common/text';
import { eventOf } from '../../events/api';
import { inventoryOf } from '../../inventory';
import { ANILOOP } from '../../motions/execute';
import { menuOf } from '../../npc/menuModule';
import { definePvpStore, type DuelRequestInfo, NULL_ID, type PvpStore } from '../store';
import {
  EVE_GUILDCOMBAT,
  EVE_GUILDCOMBAT1TO1,
  EVE_GUILDWAR,
  playerAttackViews,
} from './attackable';
import { DuelBook, type DuelFields } from './book';
import { DuelCountdown } from './DuelCountdown';
import {
  type DuelEffect,
  type DuelWorld,
  duelMenuAllowed,
  enemyMarkers,
  onDuelCancel,
  onDuelNo,
  onDuelPartyEnd,
  onDuelPartyStart,
  onDuelStart,
  onSetDuel,
  onSpawnPlayer,
} from './reducers';
import {
  sendDuelNo,
  sendDuelPartyNo,
  sendDuelPartyRequest,
  sendDuelPartyYes,
  sendDuelRequest,
  sendDuelYes,
} from './senders';

/** How many 60 Hz frames a `local_act` of the result motion is retried while the sim is busy (as the motions do). */
const ACT_RETRY_FRAMES = 30;

/** The public face of the duel part of a context. */
export interface DuelFeature {
  readonly book: DuelBook;
  readonly store: PvpStore;
  /** The duel fields of a mover (`m_nDuel` ...); a mover that is not in a duel has none. */
  fieldsOf(objId: number): DuelFields;
  /** The movers that carry the enemy marker (41 draws `XI_GEN_PVP_FLAG01` above them). */
  markers(): { readonly solo: readonly number[]; readonly party: readonly number[] };
  /** `MMI_DUEL` / `MMI_DUEL_PARTY`: challenge the player `targetObjId` (a dead or unknown player is refused). */
  request(targetObjId: number, party?: boolean): boolean;
  /** The menu rule: both stand in an `RA_FIGHT` region and the target lives. */
  duelAllowed(targetObjId: number): boolean;
  /** The duel confirm window is open. */
  confirmOpen(): boolean;
  /** Unmounts the countdown overlay (tests; the page never does). */
  dispose(): void;
}

/** Mounts the countdown overlay container between the world canvas and the UI; returns the unmount. */
export function mountCountdown(ctx: ClientContext, store: PvpStore): () => void {
  const el = document.createElement('div');
  el.dataset['testid'] = 'duel-countdown-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const ui = fullUi(ctx);
    const st = ui?.scaler.get();
    root.render(
      createElement(DuelCountdown, {
        store,
        now: () => ctx.now(),
        canvasDrawn: () => ctx.render?.pvpCountdownDrawn?.() === true,
        viewport: st?.virtual ?? { width: 1024, height: 768 },
        scale: st?.scale ?? 1,
      }),
    );
  };
  render();
  const off = fullUi(ctx)?.scaler.subscribe(render);
  return () => {
    off?.();
    root.unmount();
    el.remove();
  };
}

const features = new WeakMap<ClientContext, DuelFeature>();
export function duelOf(ctx: ClientContext): DuelFeature | undefined {
  return features.get(ctx);
}

export const duelModule: FeatureModule = {
  id: 'pvp',
  milestone: 5,
  register(fc) {
    registerDuel(fc);
  },
};

function registerDuel(fc: FeatureContext): DuelFeature {
  const ctx = fc.app;
  const { selfStore, worldStore } = ctx.stores;
  const store = definePvpStore(ctx.stores.registry);
  const book = new DuelBook();
  const info = entityInfoOf(ctx);
  const sysLine = (tid: number, args: readonly string[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };

  // ---- the client as the reducers see it ----------------------------------------------------------------------------------

  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  const nameOf = (rec: EntityRecord): string => rec.visual?.name ?? '';
  const playerByCharacter = (idPlayer: number): EntityRecord | null => {
    let found: EntityRecord | null = null;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (found === null && rec.idPlayer === idPlayer) found = rec;
    });
    return found;
  };
  const world: DuelWorld = {
    selfObj: () => selfStore.getState().objId,
    selfParty: () => selfStore.getState().partyId,
    playerById: (idPlayer) => {
      const rec = playerByCharacter(idPlayer);
      return valid(rec ?? undefined) && rec ? { objId: rec.objId, name: nameOf(rec) } : null;
    },
    moverName: (objId) => {
      const rec = ctx.entities.get(objId);
      return valid(rec) && (rec.kind === EntityKind.Player || rec.kind === EntityKind.Npc)
        ? nameOf(rec)
        : null;
    },
  };

  // ---- the own fields in the store ----------------------------------------------------------------------------------------

  const mirror = (): void => {
    const own = book.get(world.selfObj());
    const s = store.getState();
    const duel = own.duel;
    // the opponent's name follows the solo opponent; a finished duel forgets it
    const opponent =
      duel === 1 && own.other !== NULL_ID ? (world.moverName(own.other) ?? s.opponent) : '';
    if (
      s.duel === duel &&
      s.duelState === own.state &&
      s.duelOther === own.other &&
      s.duelParty === own.party &&
      s.opponent === opponent
    ) {
      return;
    }
    store.setState({
      duel,
      duelState: own.state,
      duelOther: own.other,
      duelParty: own.party,
      opponent,
      ...(duel === 2 ? {} : { partyName: '' }),
    });
  };
  book.subscribe(mirror);

  // ---- carrying out the effects of a reducer ------------------------------------------------------------------------------

  // `SendActMsg` on the own mover; a busy query slot defers it to the next iteration
  const retries: {
    msg: number;
    p: readonly [number, number, number, number, number];
    then: (result: number) => void;
    left: number;
  }[] = [];
  const act = (
    msg: number,
    p: readonly [number, number, number, number, number],
    then: (result: number) => void,
  ): void => {
    const r = ctx.sim.localAct(msg, p);
    if (r === undefined) retries.push({ msg, p, then, left: ACT_RETRY_FRAMES });
    else then(r);
  };
  ctx.tickSystems.register('pvp.duel.act', (due) => {
    for (let i = retries.length - 1; i >= 0; i -= 1) {
      const r = retries[i];
      if (!r) continue;
      const res = ctx.sim.localAct(r.msg, r.p);
      r.left -= due;
      if (res !== undefined || r.left <= 0) {
        retries.splice(i, 1);
        if (res !== undefined) r.then(res);
      }
    }
  });

  /** `CWndDuelResult::OnChildNotify`: the winner plays `MTI_HIPKIDDING` once, the loser `MTI_HEAD` looped; a started motion is reported. */
  const resultMotion = (win: boolean): void => {
    const p: readonly [number, number, number, number, number] = win
      ? [MTI_HIPKIDDING, ANILOOP.PLAY1, 0, 0, 0]
      : [MTI_HEAD, ANILOOP.CONT, 0, 0, 0];
    act(OBJMSG.MOTION, p, (result) => {
      if (result === 1) {
        // legacy-send: SendPlayerBehavior
        (ctx.moveReporter as { armBehavior?: () => void } | null)?.armBehavior?.();
      }
    });
  };

  const openResult = (win: boolean, name: string): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    // the request window is deleted first (`SAFE_DELETE(m_pWndDuelConfirm)`)
    ui.duel.closeConfirm();
    ui.duel.openResult({
      text: formatTid(fc.texts, win ? TID_DIAG_DUELRESULT_WIN : TID_DIAG_DUELRESULT_LOSE, [name]),
      win,
      done: () => {
        resultMotion(win);
      },
    });
  };

  const run = (effects: readonly DuelEffect[]): void => {
    for (const e of effects) {
      if (e.kind === 'text') sysLine(e.tid, e.args);
      else openResult(e.win, e.name);
    }
  };

  // ---- the request window -------------------------------------------------------------------------------------------------

  let requestToken = 0;
  const forget = (token: number): void => {
    if (token === requestToken) store.setState({ request: null });
  };

  /** `OnDuelRequest` / `OnDuelPartyRequest`: the window opens when the requester is visible and alive. */
  const openRequest = (src: number, party: boolean): void => {
    const ui = fullUi(ctx);
    const rec = playerByCharacter(src);
    if (!ui || !valid(rec ?? undefined) || !rec) return;
    if (info(rec.objId)?.dead === true) return;
    const name = nameOf(rec);
    const req: DuelRequestInfo = { src, objId: rec.objId, name, party };
    requestToken += 1;
    const token = requestToken;
    // `IsValidObj(pSrc)` is asked at the time of the answer: a requester who went away is not answered
    const requesterHere = (): boolean => valid(ctx.entities.get(req.objId));
    const opened = ui.duel.openConfirm({
      text: formatTid(fc.texts, party ? TID_DIAG_0067 : TID_DIAG_0066, [name]),
      yes: () => {
        forget(token);
        if (!requesterHere()) return;
        if (party) {
          sendDuelPartyYes(ctx, src);
          return;
        }
        // the transactional windows go first (dialog, trade, shop, bank, vendor, upgrade, mail ...), then the answer
        inventoryOf(ctx)?.use.cursor.cancel();
        fc.transactional.closeTransactional('DuelAccepted');
        // a window announces its closing when it is destroyed (the shop sends `CloseShopWnd`, the bank `CloseBankWnd`): the removal that
        // `closeTransactional` queued runs now, so that the announcements go out BEFORE `DuelYes` (the original sends them in its handler)
        fullUi(ctx)?.wm.flushDestroyed();
        sendDuelYes(ctx, src);
      },
      no: () => {
        forget(token);
        if (!requesterHere()) return;
        if (party) sendDuelPartyNo(ctx, src);
        else sendDuelNo(ctx, src);
      },
      closed: () => {
        forget(token);
      },
    });
    if (opened) store.setState({ request: req });
  };

  fc.net.on('duelRequestNtf', (m) => {
    openRequest(m.src, false);
  });
  fc.net.on('duelPartyRequestNtf', (m) => {
    openRequest(m.src, true);
  });

  // the request window goes with the own death, a world change, the logout and a kick (45 §25.1, 17 §11.1); not with the accept itself.
  // The result window is no transactional window: it stays through the death of the loser, but not through a world change or a logout.
  fc.transactional.register({
    id: 'pvp.duelConfirm',
    reasons: new Set(['OwnDeath', 'WorldChange', 'Logout', 'Kicked']),
    close: (reason) => {
      fullUi(ctx)?.duel.closeConfirm();
      if (reason !== 'OwnDeath') fullUi(ctx)?.duel.closeResult();
    },
  });

  // ---- the duel messages --------------------------------------------------------------------------------------------------

  fc.net.on('duelStart', (m) => {
    // `SAFE_DELETE(m_pWndPartyConfirm)`: a party invitation in the way is dropped when a duel begins
    fullUi(ctx)?.wm.close(APP_PARTY_CONFIRM);
    run(onDuelStart(book, world, m.idTarget, m.flag));
  });
  fc.net.on('duelNoNtf', (m) => {
    run(onDuelNo(world, m.decliner));
  });
  fc.net.on('duelPartyNoNtf', (m) => {
    run(onDuelNo(world, m.decliner));
  });
  fc.net.on('duelCancel', (m) => {
    run(onDuelCancel(book, world, m.target));
  });
  fc.net.on('duelPartyStart', (m) => {
    store.setState({ partyName: m.partyName });
    run(
      onDuelPartyStart(book, world, {
        idParty: m.idParty,
        partyName: m.partyName,
        members: m.members,
        flag: m.flag,
      }),
    );
  });
  fc.net.on('duelPartyCancel', (m) => {
    run(onDuelPartyEnd(book, world, m.ids, null));
  });
  fc.net.on('duelPartyResult', (m) => {
    run(onDuelPartyEnd(book, world, m.ids, m.win));
  });
  fc.net.on('setDuel', (m) => {
    run(
      onSetDuel(book, world, {
        moverObjId: m.moverObjId,
        duel: m.duel,
        duelState: m.duelState,
        idDuelOther: m.idDuelOther,
        idDuelParty: m.idDuelParty,
      }),
    );
  });
  let countSeq = 0;
  fc.net.on('duelCount', (m) => {
    countSeq += 1;
    store.setState({ count: { n: m.count, at: ctx.now(), seq: countSeq } });
    // the canvas sprite of `CMover::RenderPVPCount` (the overlay of the renderer, WP-REND-25); the DOM text hides once it is drawn
    ctx.render?.pvpCountdown?.(m.count);
  });

  // a player who appears in the middle of a duel: the fields of the spawn and the markers (`OnAddObj`)
  fc.net.on('spawnPlayer', (m, objId) => {
    const p = m.player;
    if (!p) return;
    onSpawnPlayer(book, world, {
      objId,
      duel: p.duel,
      idParty: p.idParty,
      idDuelParty: p.idDuelParty,
    });
  });
  ctx.entities.onRemove((rec) => {
    book.delete(rec.objId);
  });
  // the next world starts without duels (the store resets with its scope)
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
      book.clear();
      requestToken += 1;
    }
  });

  // ---- the challenge ----------------------------------------------------------------------------------------------------

  const regionOf = (id: number): number | null => {
    const w = worldStore.getState();
    const regions = ctx.data?.worldFolder(w.folder)?.regions;
    const pos = info(id)?.pos;
    if (!regions || !pos) return null;
    return regionAt(regions, ctx.data?.worldFolder(w.folder)?.settings.pk_mode ?? 0, pos[0], pos[2])
      .attribute;
  };

  const request = (targetObjId: number, party = false): boolean => {
    const rec = ctx.entities.get(targetObjId);
    if (!valid(rec) || rec.kind !== EntityKind.Player || rec.idPlayer === undefined) return false;
    if (info(targetObjId)?.dead === true) return false;
    const me = selfStore.getState().idPlayer;
    if (party) sendDuelPartyRequest(ctx, me, rec.idPlayer);
    else sendDuelRequest(ctx, me, rec.idPlayer);
    return true;
  };

  const duelAllowed = (targetObjId: number): boolean =>
    duelMenuAllowed(
      worldStore.getState().region.attribute,
      regionOf(targetObjId),
      info(targetObjId)?.dead === true,
    );

  // the player branch of the attack gate: `rules.clientAttackable` with the duel fields of the book (a duel partner once the fight began)
  inputProvidersOf(ctx).playerAttackable = (id) => {
    const rules = peekRulesRuntime(ctx)?.rules();
    const rec = ctx.entities.get(id);
    if (!rules || !valid(rec) || rec.kind !== EntityKind.Player) return 0;
    const s = selfStore.getState();
    const sp = (rec.spawn as SpawnPlayer).player;
    const at = info(id)?.pos;
    const own = ctx.entities.self();
    const events = eventOf(ctx);
    const views = playerAttackViews(book, {
      self: {
        objId: s.objId,
        flying: s.isFlying,
        chaotic: s.pk.propensity > 0,
        partyId: s.partyId,
        guildId: s.guildId,
        level: s.level,
        regionAttribute: worldStore.getState().region.attribute,
        x: s.pos[0],
        z: s.pos[2],
        worldId: worldStore.getState().worldId,
        warId: own?.visual?.idWar ?? 0,
        guildCombatState: (own?.spawn as SpawnSelf | undefined)?.player?.guildCombatState ?? 0,
      },
      target: {
        objId: id,
        chaotic: rec.visual?.pk.chaotic ?? false,
        partyId: sp?.idParty ?? 0,
        guildId: sp?.idGuild ?? 0,
        level: sp?.level ?? 1,
        regionAttribute: regionOf(id),
        x: at?.[0] ?? 0,
        z: at?.[2] ?? 0,
        vendorOpen: rec.visual?.vendorTitle != null,
        warId: rec.visual?.idWar ?? 0,
        guildCombatState: sp?.guildCombatState ?? 0,
      },
      events: {
        guildWar: events?.isEventOn(EVE_GUILDWAR) === true,
        guildCombat: events?.isEventOn(EVE_GUILDCOMBAT) === true,
        // ---- 1:1 guild combat (ui-minigames, WP-CFEAT-45)
        guildCombat1to1: events?.isEventOn(EVE_GUILDCOMBAT1TO1) === true,
      },
      gc1to1Target: inputProvidersOf(ctx).gc1to1?.possible(id) ?? false,
    });
    try {
      return rules.clientAttackable(views.self, views.target, views.ctx);
    } catch {
      return 0;
    }
  };

  const menu = menuOf(ctx);
  menu?.registerAction(MMI_DUEL, ({ targetId }) => {
    request(targetId, false);
  });
  menu?.registerAction(MMI_DUEL_PARTY, ({ targetId }) => {
    request(targetId, true);
  });
  menu?.addPlayerEnv((targetId) => ({ duelAllowed: duelAllowed(targetId) }));

  // the countdown overlay is mounted with the first `DuelCount` (title screens and tests without a UI pay nothing)
  let unmount: (() => void) | null = null;
  if (typeof document !== 'undefined') {
    store.subscribe((s) => {
      if (unmount === null && s.count !== null) unmount = mountCountdown(ctx, store);
    });
  }

  const feature: DuelFeature = {
    book,
    store,
    fieldsOf: (objId) => book.get(objId),
    markers: () => enemyMarkers(book, world.selfObj(), world.selfParty()),
    request,
    duelAllowed,
    confirmOpen: () => fullUi(ctx)?.duel.isConfirmOpen() ?? false,
    dispose: () => {
      unmount?.();
      unmount = null;
    },
  };
  features.set(ctx, feature);

  addProbeMember(ctx, 'duel', () => ({
    ...store.getState(),
    fields: book.ids().map((id) => ({ objId: id, ...book.get(id) })),
    markers: feature.markers(),
    confirmOpen: feature.confirmOpen(),
    resultOpen: fullUi(ctx)?.duel.isResultOpen() ?? false,
  }));
  // what the attack gate sees for a clicked player (the e2e flows of the war and the siege print it when a blow never goes out)
  addProbeMember(ctx, 'attackable', (objId: number) => {
    const events = eventOf(ctx);
    const own = ctx.entities.self();
    const rec = ctx.entities.get(objId);
    return {
      code: inputProvidersOf(ctx).playerAttackable?.(objId) ?? -1,
      worldId: worldStore.getState().worldId,
      events: {
        guildWar: events?.isEventOn(EVE_GUILDWAR) === true,
        guildCombat: events?.isEventOn(EVE_GUILDCOMBAT) === true,
      },
      own: {
        guild: selfStore.getState().guildId,
        war: own?.visual?.idWar ?? 0,
        combat: (own?.spawn as SpawnSelf | undefined)?.player?.guildCombatState ?? 0,
      },
      target: {
        known: rec !== undefined,
        guild: (rec?.spawn as SpawnPlayer | undefined)?.player?.idGuild ?? 0,
        war: rec?.visual?.idWar ?? 0,
        combat: (rec?.spawn as SpawnPlayer | undefined)?.player?.guildCombatState ?? 0,
      },
    };
  });
  // the e2e hook of the menu entry: the shipped data has no `RA_FIGHT` region (PL-147), so the "Duel" entry is not offered anywhere
  addProbeMember(ctx, 'duelRequest', (objId: number, party?: boolean) => request(objId, party));

  // own death: the transactional rows above close the request; the countdown overlay is void too
  combatRuntime(ctx).onOwnDeath(() => {
    store.setState({ count: null });
  });
  return feature;
}
