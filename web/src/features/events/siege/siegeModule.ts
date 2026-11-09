// The `siege` feature (Plan/45 §28.4; WP-CFEAT-41; 21 §7; ref/09a §5.10): the weekly guild siege on world 202 ("guild combat" of the code; NOT the declared guild war and NOT the 1:1 combat of M7).
//
//   GuildCombat (23 subs)       `eventStore.siege`, one pure reducer per sub (`reducers.ts`); every list is applied whether or not a window is open (PL-124)
//   windows                     the bid window, the application list, the line-up editor, the gate choice, the boards, the info boxes, the cancel / join confirmations, the result window, the
//                               person ranking: requests with callbacks over `ui.siege` (`ui/windows/siege`)
//   menu                        the siege entries of the NPC menu, ids 26-41 (`menu.ts`)
//   HUD                         `SiegeHud.tsx`: the timer, the boards of world 202, the roster of the own guild, the "be ready" lines
//   seams                       `siegeOf( ctx )` for the name colours (41) and Tab cycling (43), `revivalOf( ctx ).setGuildCombatState`, `guildOf( ctx ).setEnv`
//
// The music of the war (`BGM_BA_CRISIS`) belongs to the audio binding (T36, WP-AUDIO-12): this module emits no music and calls no cue.

import { createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ClientContext } from '../../../app/context';
import { EntityKind, type EntityRecord } from '../../../game/entities';
import { JOB_VAGRANT } from '../../../gen/defs/job';
import {
  TID_GAME_GUILDCOMBAT1TO1_NOTINVITEGUILD,
  TID_GAME_GUILDCOMBAT_NOT_INVITATION_GUILD,
  TID_GAME_GUILDWAR_CANCEL,
  TID_GAME_GUILDWAR_JOIN,
  TID_GAME_TOOLTIP_LOG1,
} from '../../../gen/defs/text';
import { menuOf } from '../../npc/menuModule';
import type { SpawnPlayer, SpawnSelf } from '../../../gen/proto/flyff/v1/world_pb';
import { CHATSTY } from '../../../ui/chat/styles';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { playerDataOf } from '../../common/playerData';
import { formatTid } from '../../common/text';
import { derivedView } from '../../common/view';
import { guildOf } from '../../guild/api';
import { hudOf } from '../../hud/hudModule';
import { deepEqual } from '../../party/view';
import { revivalOf } from '../../pvp/revival/revivalModule';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import {
  initialSiege,
  SIEGE_STATE,
  type EventState,
  type EventStore,
  WORLD_GUILDWAR,
} from '../store';
import { hudModel, type HudEnv, secondsLeft, type SiegeHudModel } from './hudModel';
import { logFileName, logText, type LogEnv } from './log';
import { menuTable } from './menu';
import { registerSiegeProbe } from './probe';
import { type PlayerPointEntry, rankView, resultGuildRows, resultPersonRows } from './ranking';
import {
  type Reduced,
  reduceGuildCombat,
  type SiegeEffect,
  type SiegeEnv,
  type SiegeOpen,
} from './reducers';
import { siegeSenders, type SiegeSenders } from './senders';
import { saveTextFile } from './download';
import { SiegeHud, SiegeHudState } from './SiegeHud';
import { memberRowOf, onlineMembersByLevel, RANK_KINGPIN, RANK_MASTER } from './view';

/** The refresh period of the HUD (the timer seconds and the HP bars move; a name arrives). */
export const HUD_REFRESH_MS = 250;

/** The public face of the siege feature of a context. */
export interface SiegeFeature {
  readonly store: EventStore;
  readonly senders: SiegeSenders;
  /** `siege.state`: 0 Open, 1 Close, 2 War, 3 CompleteClose, 4 GmClose. */
  state(): number;
  /** `siege.warState`: 100 .. 107. */
  warState(): number;
  /** `m_nGuildCombatState == 1`: the mover fights. */
  isFighter(objId: number): boolean;
  /** `GC_USERSTATE` of a mover (the spawn's value while it did not tell), 0 for none. */
  userStateOf(objId: number): number;
  /** The guild that won the last siege, 0 for none. */
  winGuildId(): number;
  /** The best player (MVP) of the last siege, 0 for none. */
  bestPlayerId(): number;
  /** The own guild has an application. */
  requested(): boolean;
  /** The character stands on the siege map. */
  inSiegeWorld(): boolean;
  /** The HUD (the state of its overlay; tests read it). */
  readonly hud: SiegeHudState;
  /** The model of the HUD now. */
  hudModel(): SiegeHudModel;
  /** Recomputes the HUD now. */
  refreshHud(): void;
  /** Where the kill log goes: the browser download by default (tests replace it). */
  download: (name: string, text: string) => boolean;
  /** Unmounts the HUD overlay (tests; the page never does). */
  dispose(): void;
}

const features = new WeakMap<ClientContext, SiegeFeature>();

/** The siege feature of a context; `undefined` before the module registered. */
export function siegeOf(ctx: ClientContext): SiegeFeature | undefined {
  return features.get(ctx);
}

/** Mounts the HUD container between the world canvas and the UI; returns the unmount. */
export function mountSiegeHud(ctx: ClientContext, state: SiegeHudState): () => void {
  const el = document.createElement('div');
  el.dataset['testid'] = 'siege-hud-root';
  el.style.cssText = 'position:fixed;left:0;top:0;width:0;height:0;pointer-events:none;z-index:0';
  const world = document.getElementById('world');
  if (world?.parentNode) world.parentNode.insertBefore(el, world.nextSibling);
  else document.body.append(el);
  const root: Root = createRoot(el);
  const render = (): void => {
    const st = fullUi(ctx)?.scaler.get();
    root.render(
      createElement(SiegeHud, {
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

/** Registers the siege (after the events feature: `siegeM6.ts` registers both under the one feature id `events`). */
export function registerSiege(fc: FeatureContext): SiegeFeature {
  const ctx = fc.app;
  const { selfStore, worldStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const playerData = playerDataOf(ctx);
  const senders = siegeSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const selfChar = (): number => selfStore.getState().idPlayer;
  const jobName = (job: number): string => ctx.data?.job(job)?.name ?? '';

  // ---- the facts the reducers and the windows read ----------------------------------------------------------------------------

  const selfRank = (): number => guildOf(ctx)?.rankOf(selfChar()) ?? -1;
  const siegeEnv = (): SiegeEnv => {
    const g = guildOf(ctx);
    const rank = selfRank();
    return {
      now: ctx.now(),
      worldId: worldStore.getState().worldId,
      selfCharId: selfChar(),
      selfObjId: selfStore.getState().objId,
      isMaster: rank === RANK_MASTER,
      isMasterOrKingpin: rank === RANK_MASTER || rank === RANK_KINGPIN,
      selfGuildName: g?.ownGuildName() ?? '',
      purse: selfStore.getState().gold,
      guildName: (id) => g?.nameOf(id) ?? null,
    };
  };
  const logEnv = (): LogEnv => {
    const g = guildOf(ctx);
    return {
      selfId: selfChar(),
      selfGuild: g?.ownGuildId() ?? 0,
      guildName: (id) => g?.nameOf(id) ?? null,
      masterOf: (id) => g?.briefOf(id)?.idMaster ?? 0,
      nameOf: (id) => playerData.nameOf(id),
      text,
    };
  };

  // ---- the movers ------------------------------------------------------------------------------------------------------------------

  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  const spawnState = (objId: number): number => {
    const rec = ctx.entities.get(objId);
    if (!valid(rec)) return 0;
    return (rec.spawn as SpawnPlayer | SpawnSelf).player?.guildCombatState ?? 0;
  };
  const userStateOf = (objId: number): number =>
    store.getState().siege.userState.get(objId) ?? spawnState(objId);
  const setEntityState = (objId: number, state: number): void => {
    const rec = ctx.entities.get(objId);
    if (!valid(rec)) return;
    const player = (rec.spawn as SpawnPlayer | SpawnSelf).player;
    if (player) player.guildCombatState = state;
  };

  // ---- the windows -----------------------------------------------------------------------------------------------------------------

  /** The panels of the lazy help bundle, loading it first when needed. */
  const withPanels = (fn: (panels: Readonly<Record<string, string>>) => void): void => {
    const have = ctx.data?.bundle('help');
    if (have) {
      fn(have.panels);
      return;
    }
    ctx.bundles.ensure('help').then(
      () => {
        fn(ctx.data?.bundle('help')?.panels ?? {});
      },
      () => {
        fn({});
      },
    );
  };

  /** The entries of the person ranking now open (the view recomputes them when a name arrives). */
  let rankEntries: readonly PlayerPointEntry[] = [];
  const rankSource = derivedView(
    () =>
      rankView(rankEntries, {
        selfId: selfChar(),
        nameOf: (id) => playerData.nameOf(id),
        jobName,
      }),
    [playerData.store],
    deepEqual,
  );

  /** The rows of the result window from the stored lists. */
  const resultRows = (entries: Extract<SiegeOpen, { window: 'result' }>['entries']) => {
    const s = store.getState().siege;
    return {
      guildRows: resultGuildRows(s.guildRank),
      personRows: resultPersonRows(s.playerRank, s.bestPlayer, (id) => playerData.nameOf(id)),
      log: logText(entries, logEnv()),
    };
  };

  let download: SiegeFeature['download'] = saveTextFile;
  let stopResultWatch: (() => void) | null = null;

  const open = (o: SiegeOpen): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    switch (o.window) {
      case 'offer':
        withPanels((panels) => {
          ui.siege.openOffer({
            mode: 0,
            min: o.min,
            myBid: o.myBid,
            prize: o.prize,
            info: panels['guildcombatredchipguild'] ?? '',
            bid: (penya) => {
              senders.app(penya);
            },
          });
        });
        break;
      case 'offerState':
        ui.siege.openOfferState({
          secondsToNext: o.secondsToNext,
          guilds: o.names,
          rank: o.rank,
          myBid: o.myBid,
          prize: o.prize,
          apply: () => {
            senders.window();
          },
          refresh: () => {
            senders.requestStatus();
          },
        });
        break;
      case 'selection': {
        const g = guildOf(ctx);
        if (!g) break;
        ui.siege.openSelection({
          maxWar: o.maxWar,
          maxJoin: o.maxJoin,
          members: () => onlineMembersByLevel(g.members(), jobName),
          memberOf: (id) => memberRowOf(g.members(), id, jobName),
          selected: o.ids,
          defender: o.defender,
          warState: () => store.getState().siege.warState,
          isMaster: (id) => g.isMaster(id),
          isMasterOrKingpin: (id) => {
            const r = g.rankOf(id);
            return r === RANK_MASTER || r === RANK_KINGPIN;
          },
          submit: (defender, ids) => {
            senders.selectPlayer(defender, ids);
          },
        });
        break;
      }
      case 'joinSelection':
        ui.siege.openJoinSelection({
          map: o.map,
          timeoutMs: o.timeoutMs,
          select: (map) => {
            senders.selectMap(map);
          },
        });
        break;
      case 'infoBox':
        ui.siege.openInfoBox({
          mode: 0,
          text: o.text,
          yes: () => {
            senders.tele();
          },
        });
        break;
      case 'infoBox2':
        ui.siege.openInfoBox2({ text: o.text });
        break;
      case 'result': {
        stopResultWatch?.();
        const entries = o.entries;
        const stop = (): void => {
          off();
          if (stopResultWatch === stop) stopResultWatch = null;
        };
        // a name that arrives after the window opened refreshes its rows
        let shown = JSON.stringify(resultRows(entries));
        const off = playerData.store.subscribe(() => {
          const rows = resultRows(entries);
          const json = JSON.stringify(rows);
          if (json === shown) return;
          shown = json;
          fullUi(ctx)?.siege.result()?.update(rows);
        });
        stopResultWatch = stop;
        ui.siege.openResult({
          ...resultRows(entries),
          save: (plain) => {
            const name = logFileName(new Date());
            return download(name, plain) ? name : '';
          },
          closed: stop,
        });
        break;
      }
      case 'rankPerson':
        rankEntries = o.entries;
        ui.siege.openRankPerson({
          view: rankSource,
          tabs: [text(TID_GAME_TOOLTIP_LOG1), jobName(1), jobName(4), jobName(2), jobName(3)],
        });
        break;
    }
  };

  // ---- carrying out the effects of a reducer ----------------------------------------------------------------------------------

  const putChat = (line: string): void => {
    // `g_WndMng.PutString( str )`: white, in the general channel
    fc.chat?.putString(line, null, 0xffffffff, CHATSTY.GENERAL);
  };
  const run = (effects: readonly SiegeEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'box':
          fullUi(ctx)?.msgbox.open(formatTid(fc.texts, e.tid, e.args));
          break;
        case 'chat':
          putChat(formatTid(fc.texts, e.tid, e.args));
          break;
        case 'chatText':
          putChat(e.text);
          break;
        case 'caption':
          hudOf(ctx).caption(text(e.tid), false, ctx.now());
          break;
        case 'needAll':
          playerData.needAll(e.ids);
          break;
        case 'entityState':
          setEntityState(e.objId, e.state);
          break;
        case 'hudTimer':
          events.setHudTimer(text(e.tid), e.ms, 'siege');
          break;
        case 'clearHudTimer':
          events.clearHudTimer();
          break;
        case 'open':
          open(e);
          break;
      }
    }
  };
  const apply = (r: Reduced): void => {
    if (r.state !== store.getState()) store.setState(r.state);
    run(r.effects);
  };

  fc.net.on('guildCombat', (m) => {
    apply(reduceGuildCombat(store.getState(), m.sub, siegeEnv()));
  });

  // a mover that leaves the view takes its state with it (`DelObj`)
  ctx.entities.onRemove((rec) => {
    const s = store.getState().siege;
    if (!s.userState.has(rec.objId)) return;
    const userState = new Map(s.userState);
    userState.delete(rec.objId);
    store.setState({ siege: { ...s, userState } });
  });

  // ---- the NPC menu -------------------------------------------------------------------------------------------------------------------

  const board = (panel: string): void => {
    withPanels((panels) => {
      const t = panels[panel];
      // a missing panel opens nothing
      if (t === undefined) return;
      fullUi(ctx)?.siege.openBoard({ mode: 0, text: t });
    });
  };
  const table = menuTable({
    window: () => {
      senders.window();
    },
    requestStatus: () => {
      senders.requestStatus();
    },
    openCancelConfirm: () => {
      fullUi(ctx)?.siege.openCancelConfirm({
        mode: 0,
        textColor: fc.texts.textColor(TID_GAME_GUILDWAR_CANCEL),
        yes: () => {
          senders.cancel();
        },
      });
    },
    openJoinConfirm: () => {
      fullUi(ctx)?.siege.openJoinConfirm({
        mode: 0,
        textColor: fc.texts.textColor(TID_GAME_GUILDWAR_JOIN),
        vagrant: () => selfStore.getState().job === JOB_VAGRANT,
        join: () => {
          senders.join();
        },
      });
    },
    playerPoint: () => {
      senders.playerPoint();
    },
    selectPlayerWindow: () => {
      senders.selectPlayerWindow();
    },
    getPenyaGuild: () => {
      // only the guild master asks (else silent)
      if (guildOf(ctx)?.isMaster(selfChar()) === true) senders.getPenyaGuild();
    },
    getPenyaPlayer: () => {
      senders.getPenyaPlayer();
    },
    openBoard: board,
  });
  for (const [id, fn] of table) {
    menuOf(ctx)?.registerAction(id, () => {
      fn();
    });
  }

  // ---- the seams into the other features -----------------------------------------------------------------------------------

  // a fighter gets no revival window and sends nothing: the server revives (PL-109)
  revivalOf(ctx)?.setGuildCombatState(() => userStateOf(selfStore.getState().objId));
  // the invitation, the leaving and the disbanding refuse as the original client did (the server enforces the same rules)
  guildOf(ctx)?.setEnv({
    siegeBusy: () => {
      const s = store.getState().siege;
      if (s.requested && s.state !== SIEGE_STATE.Close) {
        return TID_GAME_GUILDCOMBAT_NOT_INVITATION_GUILD;
      }
      // ---- 1:1 guild combat (task ui-minigames, WP-CFEAT-45): `g_GuildCombat1to1Mng.m_nState != GC1TO1_CLOSE` (`WndWorld.cpp:11495-11510`); any state the last message stored counts
      return store.getState().gc1to1.state !== 0 ? TID_GAME_GUILDCOMBAT1TO1_NOTINVITEGUILD : null;
    },
    siegeApplied: () => store.getState().siege.requested,
  });

  // ---- the lifetime: the world and the character ----------------------------------------------------------------------------

  // a Replace into another world takes the timer of the siege with it (ref/09a §6)
  worldStore.subscribe((s, prev) => {
    if (s.worldId !== prev.worldId && s.worldId !== WORLD_GUILDWAR) events.clearHudTimer('siege');
  });
  // the siege of a character that left the world (logout, kick, back to the character list) goes with it; a world change keeps it, the join batch sends it again
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    stopResultWatch?.();
    rankEntries = [];
    fullUi(ctx)?.siege.closeAll();
    store.setState({ siege: initialSiege(), hudTimer: null });
  });

  // ---- the HUD ---------------------------------------------------------------------------------------------------------------------

  const hud = new SiegeHudState();
  const hpOf = (id: number): number => {
    if (id === selfChar()) {
      const s = selfStore.getState();
      return s.maxHp > 0 ? s.hp / s.maxHp : 0;
    }
    let out = 0;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (rec.removed || rec.idPlayer !== id) return;
      const max = rec.maxHp ?? 0;
      out = max > 0 ? rec.hp / max : 0;
    });
    return out;
  };
  const hudEnv = (): HudEnv => {
    const st = fullUi(ctx)?.scaler.get();
    const fmtSeconds = (tid: number, n: number): string => formatTid(fc.texts, tid, [n]);
    return {
      width: st?.virtual.width ?? 1024,
      height: st?.virtual.height ?? 768,
      now: ctx.now(),
      worldId: worldStore.getState().worldId,
      selfCharId: selfChar(),
      selfGuildName: guildOf(ctx)?.ownGuildName() ?? '',
      selfName: selfStore.getState().name,
      nameOf: (id) => playerData.nameOf(id),
      hpOf,
      text,
      fmt: fmtSeconds,
    };
  };
  const currentModel = (): SiegeHudModel => {
    const s = store.getState();
    return hudModel(s.siege, s.hudTimer, hudEnv());
  };
  const refreshHud = (): void => {
    const s = store.getState();
    // a timer that reached 0 is gone
    if (s.hudTimer && secondsLeft(s.hudTimer, ctx.now()) <= 0) {
      store.setState({ hudTimer: null });
      return;
    }
    const m = currentModel();
    const empty =
      !m.timer && !m.guildBoard && !m.personBoard && !m.roster && m.joinReady.length === 0;
    // nothing to draw until the first siege data exists; the overlay is mounted with it
    hud.set(empty ? null : m);
  };
  store.subscribe(refreshHud);
  playerData.store.subscribe(refreshHud);
  let last = Number.NEGATIVE_INFINITY;
  ctx.tickSystems.register('siege.hud', () => {
    const now = ctx.now();
    if (now - last < HUD_REFRESH_MS) return;
    last = now;
    refreshHud();
  });
  let unmount: (() => void) | null = null;
  if (typeof document !== 'undefined') {
    hud.subscribe(() => {
      if (unmount === null && hud.model !== null) unmount = mountSiegeHud(ctx, hud);
    });
  }

  // ---- the public face ------------------------------------------------------------------------------------------------------------

  const feature: SiegeFeature = {
    store,
    senders,
    state: () => store.getState().siege.state,
    warState: () => store.getState().siege.warState,
    isFighter: (objId) => userStateOf(objId) === 1,
    userStateOf,
    winGuildId: () => store.getState().siege.winGuild?.guildId ?? 0,
    bestPlayerId: () => store.getState().siege.bestPlayer ?? 0,
    requested: () => store.getState().siege.requested,
    inSiegeWorld: () => worldStore.getState().worldId === WORLD_GUILDWAR,
    hud,
    hudModel: currentModel,
    refreshHud,
    get download() {
      return download;
    },
    set download(fn) {
      download = fn;
    },
    dispose: () => {
      unmount?.();
      unmount = null;
    },
  };
  features.set(ctx, feature);

  registerSiegeProbe(ctx);
  addProbeMember(ctx, 'siege', () => {
    const s: EventState = store.getState();
    const g = s.siege;
    return {
      state: g.state,
      warState: g.warState,
      requested: g.requested,
      hudTimer: s.hudTimer,
      roster: g.roster,
      ownLives: g.ownLives,
      joinIndex: g.joinIndex,
      guildRank: g.guildRank,
      playerRank: g.playerRank,
      winGuild: g.winGuild ?? null,
      bestPlayer: g.bestPlayer ?? null,
      userState: [...g.userState],
      inWorld: feature.inSiegeWorld(),
      windows: fullUi(ctx)?.siege.openApps() ?? [],
    };
  });
  return feature;
}
