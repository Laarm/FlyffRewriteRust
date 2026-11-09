// The `gc1to1` feature (Plan/45 §28.5; WP-CFEAT-45; 21 §8; ref/09b §3.16 "1:1 guild combat"): the guild-versus-guild combat on the four small arenas (worlds 241 .. 244), event 22, default OFF (PD-14).
// EVERY handler and menu action is a no-op while `isEventOn( 22 )` is false (the server sends none of the messages then, and `features/npc/menu.ts` hides ids 60-70).
//
//   five server messages   `eventStore.gc1to1`, one pure reducer each (`reducers.ts`): the tender window (675) and the application status (676) (`tender.ts`), the line-up editor (672), the state with the HUD
//                          timer label and the entrance box, the round result
//   windows                ui-siege's shells (`ui.siege.openOffer / openInfoBox / openCancelConfirm / openJoinConfirm / openBoard`, mode 1), ui-lord's 676 (`ui.gc1to1.openOfferState`) and ui-secret-tax's 672
//                          (`ui.secretRoom.openGc11Selection`): requests with callbacks, this module owns the wiring
//   menu                   ids 60-70 and the arena pair 80 / 81 (`menu.ts`)
//   senders                the nine `SendGC1to1*` and the two `SendHdr` of the arena (`senders.ts`)
//   probe                  `features.gc1to1()` (here) and `features.gc1to1Selection()` (`probe.ts`, the line-up editor)
//   HUD                    the boards of the line-up and of the round wins for a participant of the arena (`hudModel.ts`, `Gc1to1Hud.tsx`); the timer line is the siege HUD's
//   seams                  `gc1to1Of( ctx )`: `state()`, `participant()`, `possible( objId )`; `inputProvidersOf( ctx ).gc1to1` for the camera, the Alt-click menus and the attack facts; the guild
//                          window refuses an invitation while the combat is not closed (`events/siege/siegeModule.ts` `siegeBusy`)
//
// The slice is reset when the character leaves the world (logout, kick, back to the character list), not on a world change (the combat itself moves players between worlds).

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../../app/context';
import { inputProvidersOf } from '../../../app/inputProviders';
import { EntityKind } from '../../../game/entities';
import { APP_GUILDCOMBAT_MSG } from '../../../gen/defs/resdata';
import { JOB_VAGRANT } from '../../../gen/defs/job';
import { TID_GAME_GUILDWAR_CANCEL } from '../../../gen/defs/text';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { playerDataOf } from '../../common/playerData';
import { formatTid } from '../../common/text';
import { guildOf } from '../../guild/api';
import { menuOf } from '../../npc/menuModule';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import { withHelpPanels } from '../helpPanels';
import { secondsLeft } from '../siege/hudModel';
import { siegeOf } from '../siege/siegeModule';
import { memberRowOf, onlineMembersByLevel, RANK_KINGPIN, RANK_MASTER } from '../siege/view';
import type { EventStore } from '../store';
import { Gc1to1Hud, Gc1to1HudState } from './Gc1to1Hud';
import { gc1to1HudModel, type Gc1to1HudEnv, type Gc1to1HudModel } from './hudModel';
import { GC1TO1_MENU_IDS, menuTable } from './menu';
import { registerGc1to1Probe } from './probe';
import {
  type Gc1to1Effect,
  type Gc1to1Env,
  reduceLineUp,
  reduceNowState,
  reduceWarResult,
} from './reducers';
import { gc1to1Senders, type Gc1to1Senders } from './senders';
import { GC1TO1_WORLDS, type Gc1to1Slice, initialGc1to1 } from './slice';
import { offerRequest, stateRequest } from './tender';

/** The event flag of the combat (`EVE_GUILDCOMBAT1TO1`). */
export const EVE_GUILDCOMBAT1TO1 = 22;

/** The oneof cases of the five server messages (a new proto case fails `every_gc1to1_message_has_a_handler`). */
export const GC1TO1_CASES = [
  'gc1to1TenderOpenWndNtf',
  'gc1to1TenderViewNtf',
  'gc1to1MemberLineUpOpenWndNtf',
  'gc1to1NowState',
  'gc1to1WarResult',
] as const;

/** The refresh period of the HUD (a name arrives, a mover leaves the view). */
export const GC1TO1_HUD_REFRESH_MS = 250;

/** The public face of the 1:1 guild combat feature of a context. */
export interface Gc1to1Feature {
  readonly store: EventStore;
  readonly senders: Gc1to1Senders;
  /** The slice now. */
  slice(): Gc1to1Slice;
  /** `g_GuildCombat1to1Mng.m_nState`: any value of the last `Gc1to1NowState`. */
  state(): number;
  /** The event is on (`g_eLocal.GetState( EVE_GUILDCOMBAT1TO1 )`). */
  enabled(): boolean;
  /** `IsPossibleMover( g_pPlayer )`: the own character fights in the arena. */
  participant(): boolean;
  /** `IsPossibleMover( mover )`: the mover is in an arena world and takes part. */
  possible(objId: number): boolean;
  /** The HUD (tests read its state). */
  readonly hud: Gc1to1HudState;
  hudModel(): Gc1to1HudModel | null;
  refreshHud(): void;
  /** Unmounts the HUD overlay (tests; the page never does). */
  dispose(): void;
}

const features = new WeakMap<ClientContext, Gc1to1Feature>();

/** The 1:1 combat feature of a context; `undefined` before the module registered. */
export function gc1to1Of(ctx: ClientContext): Gc1to1Feature | undefined {
  return features.get(ctx);
}

/** Mounts the HUD container between the world canvas and the UI; returns the unmount. */
export function mountGc1to1Hud(ctx: ClientContext, state: Gc1to1HudState): () => void {
  const el = document.createElement('div');
  el.dataset['testid'] = 'gc1to1-hud-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const st = fullUi(ctx)?.scaler.get();
    root.render(
      createElement(Gc1to1Hud, {
        state,
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

/** Registers the 1:1 combat (after the events feature and the siege: `minigamesM7.ts` registers them under the one handler module `events.minigames`). */
export function registerGc1to1(fc: FeatureContext): Gc1to1Feature {
  const ctx = fc.app;
  const { selfStore, worldStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const playerData = playerDataOf(ctx);
  const senders = gc1to1Senders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const jobName = (job: number): string => ctx.data?.job(job)?.name ?? '';
  const slice = (): Gc1to1Slice => store.getState().gc1to1;
  const enabled = (): boolean => events.isEventOn(EVE_GUILDCOMBAT1TO1);
  const selfChar = (): number => selfStore.getState().idPlayer;

  // ---- the movers -----------------------------------------------------------------------------------------------------------

  const inArenaWorld = (): boolean =>
    (GC1TO1_WORLDS as readonly number[]).includes(worldStore.getState().worldId);
  const userStateOf = (objId: number): number => siegeOf(ctx)?.userStateOf(objId) ?? 0;
  /** `IsPossibleMover`: an arena world and `m_nGuildCombatState != 0` (the combat shares `GC_USERSTATE` with the siege). */
  const possible = (objId: number): boolean => inArenaWorld() && userStateOf(objId) !== 0;
  const participant = (): boolean => possible(selfStore.getState().objId);
  const moverOfCharacter = (charId: number): number | null => {
    if (charId === selfChar()) return selfStore.getState().objId;
    let out: number | null = null;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (!rec.removed && rec.idPlayer === charId) out = rec.objId;
    });
    return out;
  };

  // ---- the windows ---------------------------------------------------------------------------------------------------------------

  const env = (): Gc1to1Env => ({ selfIsMaster: guildOf(ctx)?.isMaster(selfChar()) === true });
  const tenderEnv = (info: string) => ({
    info,
    guildName: (id: number): string | null => guildOf(ctx)?.nameOf(id) ?? null,
    send: senders,
  });
  const openLineUp = (ids: readonly number[]): void => {
    const ui = fullUi(ctx);
    const g = guildOf(ctx);
    if (!ui || !g) return;
    const limits = slice().limits;
    ui.secretRoom.openGc11Selection({
      minLevel: limits.minLevel,
      min: limits.min,
      max: limits.max,
      selected: ids,
      members: () => onlineMembersByLevel(g.members(), jobName),
      memberOf: (id) => memberRowOf(g.members(), id, jobName),
      isMaster: (id) => g.isMaster(id),
      isKingpin: (id) => {
        const r = g.rankOf(id);
        return r === RANK_MASTER || r === RANK_KINGPIN;
      },
      // the editor works only while the combat takes line-ups (`g_GuildCombat1to1Mng.m_nState == OPEN`)
      canEdit: () => slice().state === 1,
      submit: (picked) => {
        senders.memberLineUp(picked);
      },
    });
  };
  const run = (effects: readonly Gc1to1Effect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'hudTimer':
          events.setHudTimer(formatTid(fc.texts, e.tid, e.args), e.ms, 'gc1to1', e.waitSec);
          break;
        case 'clearHudTimer':
          events.clearHudTimer();
          break;
        case 'infoBox':
          fullUi(ctx)?.siege.openInfoBox({
            mode: 1,
            text: text(e.tid),
            yes: () => {
              senders.teleportToNpc();
            },
          });
          break;
        case 'needAll':
          playerData.needAll(e.ids);
          break;
        case 'openLineUp':
          openLineUp(e.ids);
          break;
      }
    }
  };
  const setSlice = (next: Gc1to1Slice): void => {
    if (next !== slice()) store.setState({ gc1to1: next });
  };

  // ---- the messages ----------------------------------------------------------------------------------------------------------------

  fc.net.on('gc1to1TenderOpenWndNtf', (m) => {
    if (!enabled()) return;
    withHelpPanels(ctx, (panels) => {
      fullUi(ctx)?.siege.openOffer(
        offerRequest(
          { myPenya: m.myPenya, joinPenya: m.joinPenya },
          tenderEnv(panels['guildcombatredchipguild'] ?? ''),
        ),
      );
    });
  });
  fc.net.on('gc1to1TenderViewNtf', (m) => {
    if (!enabled()) return;
    fullUi(ctx)?.gc1to1.openOfferState(
      stateRequest(
        { penya: m.penya, ranking: m.ranking, remainS: m.remainS, guilds: [...m.guilds] },
        tenderEnv(''),
      ),
    );
  });
  fc.net.on('gc1to1MemberLineUpOpenWndNtf', (m) => {
    if (!enabled()) return;
    const r = reduceLineUp(slice(), {
      minLevel: m.minLevel,
      minPlayers: m.minPlayers,
      maxPlayers: m.maxPlayers,
      ids: [...m.ids],
    });
    setSlice(r.slice);
    run(r.effects);
  });
  fc.net.on('gc1to1NowState', (m) => {
    if (!enabled()) return;
    const r = reduceNowState(
      slice(),
      {
        state: m.state,
        timeMs: m.timeMs,
        members: [...m.members],
        enemyGuild: m.enemyGuild,
        progCount: m.progCount,
      },
      env(),
    );
    setSlice(r.slice);
    run(r.effects);
  });
  fc.net.on('gc1to1WarResult', (m) => {
    if (!enabled()) return;
    setSlice(
      reduceWarResult(slice(), {
        state: m.state,
        idPlayer: m.idPlayer,
        winCount0: m.winCount0,
        winCount1: m.winCount1,
      }),
    );
  });

  // ---- the NPC menu ------------------------------------------------------------------------------------------------------------------

  const board = (panel: string): void => {
    withHelpPanels(ctx, (panels) => {
      const t = panels[panel];
      // a missing panel opens nothing (`scanner.Load` failed)
      if (t === undefined) return;
      fullUi(ctx)?.siege.openBoard({ mode: 1, text: t });
    });
  };
  const table = menuTable({
    tenderOpenWnd: () => {
      senders.tenderOpenWnd();
    },
    openCancelConfirm: () => {
      fullUi(ctx)?.siege.openCancelConfirm({
        mode: 1,
        textColor: fc.texts.textColor(TID_GAME_GUILDWAR_CANCEL),
        yes: () => {
          senders.tenderCancel();
        },
      });
    },
    tenderView: () => {
      senders.tenderView();
    },
    memberLineUpOpenWnd: () => {
      senders.memberLineUpOpenWnd();
    },
    openJoinConfirm: () => {
      fullUi(ctx)?.siege.openJoinConfirm({
        mode: 1,
        vagrant: () => selfStore.getState().job === JOB_VAGRANT,
        join: () => {
          // an open information box of the entrance goes first (`WndField.cpp:17571-17600`)
          fullUi(ctx)?.wm.close(APP_GUILDCOMBAT_MSG);
          senders.teleportToStage();
        },
      });
    },
    openBoard: board,
    tenderFailed: () => {
      senders.tenderFailed();
    },
    arenaEnter: () => {
      senders.arenaEnter();
    },
    arenaExit: () => {
      senders.arenaExit();
    },
  });
  const gated = new Set<number>(GC1TO1_MENU_IDS);
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      // ids 60-70 do nothing while the event is off; the arena pair is not gated by the client
      if (gated.has(id) && !enabled()) return;
      fn();
    });
  }

  // ---- the seams into the other features ----------------------------------------------------------------------------------------------

  inputProvidersOf(ctx).gc1to1 = { participant, possible };

  // ---- the lifetime: the character ---------------------------------------------------------------------------------------------------

  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    fullUi(ctx)?.gc1to1.closeAll();
    events.clearHudTimer('gc1to1');
    store.setState({ gc1to1: initialGc1to1() });
  });

  // ---- the HUD ------------------------------------------------------------------------------------------------------------------------

  const hud = new Gc1to1HudState();
  const hudEnv = (): Gc1to1HudEnv => {
    const g = guildOf(ctx);
    const memberIds = new Set(g?.memberIds() ?? []);
    return {
      width: fullUi(ctx)?.scaler.get().virtual.width ?? 1024,
      selfGuildName: g?.ownGuildName() ?? '',
      isMember: (id) => memberIds.has(id),
      nameOf: (id) => playerData.nameOf(id),
      possible: (id) => {
        const obj = moverOfCharacter(id);
        return obj !== null && possible(obj);
      },
      guildName: (id) => g?.nameOf(id) ?? null,
      text,
    };
  };
  const currentModel = (): Gc1to1HudModel | null =>
    participant() ? gc1to1HudModel(slice(), hudEnv()) : null;
  const refreshHud = (): void => {
    const s = store.getState().hudTimer;
    // a timer that reached 0 is gone (the siege HUD does the same for its own writers; this one owns the 'gc1to1' line)
    if (s?.src === 'gc1to1' && secondsLeft(s, ctx.now()) <= 0) events.clearHudTimer('gc1to1');
    hud.set(currentModel());
  };
  store.subscribe(refreshHud);
  playerData.store.subscribe(refreshHud);
  let last = Number.NEGATIVE_INFINITY;
  ctx.tickSystems.register('gc1to1.hud', () => {
    const now = ctx.now();
    if (now - last < GC1TO1_HUD_REFRESH_MS) return;
    last = now;
    refreshHud();
  });
  let unmount: (() => void) | null = null;
  if (typeof document !== 'undefined') {
    hud.subscribe(() => {
      if (unmount === null && hud.model !== null) unmount = mountGc1to1Hud(ctx, hud);
    });
  }

  // ---- the public face ------------------------------------------------------------------------------------------------------------------

  const feature: Gc1to1Feature = {
    store,
    senders,
    slice,
    state: () => slice().state,
    enabled,
    participant,
    possible,
    hud,
    hudModel: currentModel,
    refreshHud,
    dispose: () => {
      unmount?.();
      unmount = null;
    },
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'gc1to1', () => {
    const s = slice();
    return {
      enabled: enabled(),
      state: s.state,
      lineup: [...s.lineup],
      opponentGuild: s.opponentGuild,
      round: s.round,
      waiting: s.waiting,
      results: [...s.results],
      wins: [...s.wins],
      limits: s.limits,
      participant: participant(),
      hudTimer: store.getState().hudTimer,
      windows: fullUi(ctx)?.siege.openApps() ?? [],
    };
  });
  registerGc1to1Probe(ctx);
  return feature;
}
