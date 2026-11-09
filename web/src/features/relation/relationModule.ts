// The `relation` feature module (Plan/45 §24.2, §24.3, §19.4, §2.8; 21 §11-§14; WP-CFEAT-40): the couple and its window, the campus (invitation, severance, the messenger's campus tab and the
// player menu), the honor titles (the character window's second tab, the title above the names) and the cheer points (the motion, the menu entry and the tooltip).
//
//   CoupleNtf, ProposeResult, CoupleResult, DecoupleResult, AddCoupleExperience    the couple record, the proposal box (`CWndCoupleMessage`), the lines, the level (`reducers.ts`)
//   CampusInviteNtf, CampusUpdate, CampusRemove, CampusUpdatePoint                  `APP_CONFIRM_ENTER` (accept / refuse), the campus record, the points, the tab of the messenger
//   HonorListAck, HonorChangeAck, SpawnSelf, SpawnPlayer                            the earned titles (a notice for a new one), `visual.title` of every mover that changed its title
//   SetCheerParam                                                                  the cheer points and the wait for the next one
//   motions MOT_BASE_CHEER / MOT_BASE_COUPLE, menu MMI_CHEER / MMI_INVITE_CAMPUS / MMI_REMOVE_CAMPUS
//
// Other features need nothing from it: the windows read their ports (`ui.couple`, `ui.char` honor, `ui.messenger` campus).

import { EntityKind, type EntityRecord } from '../../game/entities';
import {
  MMI_CHEER,
  MMI_INVITE_CAMPUS,
  MMI_REMOVE_CAMPUS,
  MOT_BASE_CHEER,
  MOT_BASE_COUPLE,
} from '../../gen/defs/neuz';
import {
  TID_CHEER_NO2,
  TID_GAME_CAMPUS_INVITATION_CONFIRM,
  TID_GAME_CAMPUS_SEVERANCE_CONFIRM,
  TID_GAME_COUPLECANCEL,
  TID_GAME_NOTCOUPLETARGET,
  TID_GAME_PROPOSETO,
  TID_GAME_RECEIVEPROPOSAL,
} from '../../gen/defs/text';
import type { SpawnPlayer } from '../../gen/proto/flyff/v1/world_pb';
import type { CampusActions, CampusTabPorts, CampusView } from '../../ui/windows/messenger/ports';
import type {
  CoupleInfoView,
  CouplePorts,
  CoupleSkillView,
  HonorPorts,
  HonorView,
} from '../../ui/windows/couple/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { formatTid, sysChat } from '../common/text';
import { playerDataOf } from '../common/playerData';
import { derivedView } from '../common/view';
import { motionsOf } from '../motions/motionsModule';
import { messengerOf } from '../messenger/api';
import { menuOf } from '../npc/menuModule';
import { deepEqual } from '../party/view';
import { cheerTooltip } from './cheer';
import { HonorTable } from './honorTitles';
import {
  isCampusMember,
  onAddCoupleExperience,
  onCampusInvite,
  onCampusRemove,
  onCampusUpdate,
  onCampusUpdatePoint,
  onCoupleNtf,
  onCoupleResult,
  onDecoupleResult,
  onHonorChangeAck,
  onHonorListAck,
  onProposeResult,
  onSetCheerParam,
  onSpawnSelf,
  type Reduced,
  type RelationEffect,
  type RelationTables,
} from './reducers';
import { registerRelationProbe } from './probe';
import { relationSenders } from './senders';
import { defineRelationStore, type RelationStore } from './store';
import { campusViewOf, coupleInfoView, coupleSkillView, honorView, type ViewFacts } from './view';

/** The public face of the relation feature of a context. */
export interface RelationFeature {
  readonly store: RelationStore;
  readonly senders: ReturnType<typeof relationSenders>;
  /** The couple message window for a proposal to the player with that object id (the motion `MOT_BASE_COUPLE`); `false` when it is not a player in view. */
  proposeTo(objId: number): boolean;
  /** `SendCheering` for a player in view (the motion `MOT_BASE_CHEER` and the menu entry); `false` when it is not a player in view. */
  cheer(objId: number): boolean;
  /** The campus invitation of the menu entry for the player with that object id. */
  invite(objId: number): boolean;
  /** The severance question for the player with that object id (the menu entry) or character id (the campus tab). */
  sever(characterId: number, name: string): void;
}

const features = new WeakMap<object, RelationFeature>();

/** The relation feature of a context; `undefined` before the module registered. */
export function relationOf(ctx: object): RelationFeature | undefined {
  return features.get(ctx);
}

export const relationModule: FeatureModule = {
  id: 'relation',
  milestone: 6,
  register(fc) {
    registerRelation(fc);
  },
};

function registerRelation(fc: FeatureContext): RelationFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const store = defineRelationStore(ctx.stores.registry);
  const playerData = playerDataOf(ctx);
  const senders = relationSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  /** What the probe counts: the lists the server sent (the honor page refreshes on each). */
  const stats = { honorAcks: 0 };
  const selfId = (): number => selfStore.getState().idPlayer;

  // ---- the tables -------------------------------------------------------------------------------------------------------------

  let honorRows: unknown = null;
  let honorTable = new HonorTable([]);
  /** The `honor` bundle as a table (rebuilt when the bundle changes). */
  const honor = (): HonorTable => {
    const rows = ctx.data?.bundle('honor')?.titles;
    if (rows !== honorRows && rows) {
      honorRows = rows;
      honorTable = new HonorTable(rows);
    }
    return honorTable;
  };
  const tables = (): RelationTables => ({
    levelExp: ctx.data?.bundle('tables')?.couple.level_exp ?? [],
    honor: honor(),
  });

  // ---- the movers in view -------------------------------------------------------------------------------------------------------

  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  const characterOf = (rec: EntityRecord): number | undefined =>
    rec.idPlayer ?? (rec.spawn as SpawnPlayer).player?.idPlayer;
  /** A player in view with that object id. */
  const playerRec = (objId: number | null): EntityRecord | null => {
    if (objId === null) return null;
    const rec = ctx.entities.get(objId);
    return valid(rec) && rec.kind === EntityKind.Player ? rec : null;
  };

  /** `pMover->SetTitle( GetTitle( id ) )` (`objId` 0 = the own character). */
  const setTitle = (objId: number, id: number): void => {
    const rec = objId === 0 ? ctx.entities.self() : ctx.entities.get(objId);
    if (valid(rec) && rec.visual) rec.visual.title = honor().titleOf(id);
  };

  // ---- the windows -------------------------------------------------------------------------------------------------------------

  /** The couple message window: the text, what OK does and what Cancel does. */
  const coupleBox = (message: string, ok: () => void, cancel?: () => void): void => {
    fullUi(ctx)?.couple.openMessage({ text: message, ok, ...(cancel ? { cancel } : {}) });
  };

  /** `CM_SENDPROPOSE`: "Would you like to propose to X?", OK sends the proposal. */
  const proposeBox = (name: string): void => {
    coupleBox(formatTid(fc.texts, TID_GAME_PROPOSETO, [name]), () => {
      senders.propose(name);
    });
  };

  /** `CWndCampusSeveranceConfirm( id, name )`: a new one replaces the open one. */
  const severance = (characterId: number, name: string): void => {
    fullUi(ctx)?.itemUse.openConfirmEnter({
      text: formatTid(fc.texts, TID_GAME_CAMPUS_SEVERANCE_CONFIRM, [name]),
      yes: () => {
        senders.removeCampusMember(characterId);
      },
    });
  };

  // ---- carrying out the effects of a reducer ----------------------------------------------------------------------------

  const run = (effects: readonly RelationEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'line':
          sysChat(fc.texts, fullUi(ctx)?.chat ?? null, e.tid, e.args);
          break;
        case 'need':
          playerData.need(e.id);
          if (e.name !== undefined && e.name !== '' && playerData.nameOf(e.id) === '') {
            playerData.onRename(e.id, e.name);
          }
          break;
        case 'needAll':
          playerData.needAll(e.ids);
          break;
        case 'proposal':
          // `CM_RECEIVEPROPOSE`: OK accepts (`SendCouple`), Cancel refuses (`SendRefuse`)
          coupleBox(
            formatTid(fc.texts, TID_GAME_RECEIVEPROPOSAL, [e.name]),
            () => {
              senders.couple();
            },
            () => {
              senders.refuse();
            },
          );
          break;
        case 'campusInvite': {
          const requester = e.requester;
          fullUi(ctx)?.itemUse.openConfirmEnter({
            text: formatTid(fc.texts, TID_GAME_CAMPUS_INVITATION_CONFIRM, [e.name]),
            yes: () => {
              senders.acceptCampusMember(requester);
            },
            no: () => {
              senders.refuseCampusMember(requester);
            },
          });
          break;
        }
        case 'refreshCampus':
          fullUi(ctx)?.messenger.refreshCampus();
          break;
        case 'title':
          setTitle(e.objId, e.id);
          break;
      }
    }
  };
  const apply = (r: Reduced): void => {
    if (r.state !== store.getState()) store.setState(r.state);
    run(r.effects);
  };

  // ---- the messages -------------------------------------------------------------------------------------------------------------

  fc.net.on('spawnSelf', (m) => {
    apply(
      onSpawnSelf(
        {
          honorProgress: m.self?.honorProgress ?? [],
          honor: m.player?.honor ?? -1,
          campusPoint: m.self?.campusPoint ?? 0,
          cheerPoint: m.self?.cheerPoint ?? 0,
          cheerRestMs: m.self?.cheerRestMs ?? 0,
        },
        tables(),
        ctx.now(),
      ),
    );
  });
  // a player who appears carries the selected title
  fc.net.on('spawnPlayer', (m, objId) => {
    setTitle(objId, m.player?.honor ?? -1);
  });

  fc.net.on('coupleNtf', (m) => {
    const c = m.couple;
    if (!c) return;
    apply(
      onCoupleNtf(
        store.getState(),
        { exp: c.experience, first: c.idFirst, second: c.idSecond },
        selfId(),
        tables(),
      ),
    );
  });
  fc.net.on('proposeResult', (m) => {
    apply(onProposeResult(store.getState(), { proposer: m.proposer, name: m.name }));
  });
  fc.net.on('coupleResult', (m) => {
    apply(onCoupleResult(store.getState(), { partner: m.partner, name: m.name }, selfId()));
  });
  fc.net.on('decoupleResult', () => {
    apply(onDecoupleResult(store.getState()));
  });
  fc.net.on('addCoupleExperience', (m) => {
    apply(onAddCoupleExperience(store.getState(), m.exp, tables()));
  });

  fc.net.on('campusInviteNtf', (m) => {
    apply(onCampusInvite(store.getState(), { requester: m.requester, name: m.name }));
  });
  fc.net.on('campusUpdate', (m) => {
    const c = m.campus;
    if (!c) return;
    apply(
      onCampusUpdate(store.getState(), {
        id: c.idCampus,
        master: c.idMaster,
        members: c.members.map((x) => ({ id: x.idPlayer, level: x.memberLevel })),
      }),
    );
  });
  fc.net.on('campusRemove', (m) => {
    apply(onCampusRemove(store.getState(), m.idCampus));
  });
  fc.net.on('campusUpdatePoint', (m) => {
    apply(onCampusUpdatePoint(store.getState(), m.point));
  });

  fc.net.on('honorListAck', (m) => {
    stats.honorAcks += 1;
    apply(onHonorListAck(store.getState(), [...m.counters], tables()));
  });
  fc.net.on('honorChangeAck', (m, objId) => {
    const own = ctx.entities.self()?.objId;
    apply(
      onHonorChangeAck(store.getState(), { id: m.id, objId }, own !== undefined && own === objId),
    );
  });
  fc.net.on('setCheerParam', (m) => {
    apply(
      onSetCheerParam(
        store.getState(),
        { points: m.points, restMs: m.restMs, add: m.add },
        ctx.now(),
      ),
    );
  });

  // ---- the player actions ---------------------------------------------------------------------------------------------------

  const proposeTo = (objId: number): boolean => {
    const rec = playerRec(objId);
    if (!rec) return false;
    proposeBox(rec.visual?.name ?? '');
    return true;
  };

  const cheer = (objId: number): boolean => {
    const rec = playerRec(objId);
    if (!rec) return false;
    senders.cheering(rec.objId);
    return true;
  };

  const invite = (objId: number): boolean => {
    const rec = playerRec(objId);
    const id = rec ? characterOf(rec) : undefined;
    if (!rec || id === undefined) return false;
    senders.inviteCampusMember(id);
    // `SendQueryPlayerData( pFocusMover->m_idPlayer )`
    playerData.need(id);
    return true;
  };

  const feature: RelationFeature = {
    store,
    senders,
    proposeTo,
    cheer,
    invite,
    sever: severance,
  };
  features.set(ctx, feature);

  // the player menu: `CCampusHelper` decides which campus entry a player gets; the actions look at the target
  menuOf(ctx)?.addPlayerEnv((targetId) => {
    const campus = store.getState().campus;
    if (!campus) return { campus: 'none' };
    const rec = playerRec(targetId);
    const id = rec ? characterOf(rec) : undefined;
    return { campus: id !== undefined && isCampusMember(campus, id) ? 'member' : 'other' };
  });
  menuOf(ctx)?.registerAction(MMI_CHEER, ({ targetId }) => {
    // `if( g_pPlayer->m_pActMover->IsFly() == FALSE )`; the motion has no such check (KEEP)
    if (selfStore.getState().isFlying) return;
    cheer(targetId);
  });
  menuOf(ctx)?.registerAction(MMI_INVITE_CAMPUS, ({ targetId }) => {
    invite(targetId);
  });
  menuOf(ctx)?.registerAction(MMI_REMOVE_CAMPUS, ({ targetId }) => {
    const rec = playerRec(targetId);
    const id = rec ? characterOf(rec) : undefined;
    if (rec && id !== undefined) severance(id, rec.visual?.name ?? '');
  });

  // the motions: the focus decides (`g_pPlayer->GetWorld()->GetObjFocus()`), a monster does not count
  const motions = motionsOf(ctx);
  motions?.register(MOT_BASE_CHEER, (env) => {
    // no movement report: the original cleared `fBehavior` for a sent cheer
    if (!cheer(env.focusId() ?? -1)) env.say(TID_CHEER_NO2);
  });
  motions?.register(MOT_BASE_COUPLE, (env) => {
    if (!proposeTo(env.focusId() ?? -1)) env.say(TID_GAME_NOTCOUPLETARGET);
  });
  // the tooltip of the cheer motion shows the points and the wait for the next one, read live on every hover
  motions?.registerTooltip(MOT_BASE_CHEER, (description, variant) =>
    cheerTooltip(
      description,
      store.getState().cheer,
      ctx.now(),
      text,
      variant === 'taskbar' ? '\n\n' : '\n',
    ),
  );

  // ---- the read models -------------------------------------------------------------------------------------------------------

  const viewFacts = (): ViewFacts => ({
    self: selfId(),
    cached: (id) => playerData.get(id),
    job: (j) => {
      const row = ctx.data?.job(j);
      return { type: row?.type ?? 0, name: row?.name ?? '' };
    },
  });
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? ((): void => undefined),
  };

  const coupleInfo = derivedView<CoupleInfoView>(
    () => coupleInfoView(store.getState().couple, tables().levelExp, viewFacts()),
    [store, playerData.store, dataSource],
  );
  const coupleSkill = derivedView<CoupleSkillView>(
    () => {
      const t = ctx.data?.bundle('tables')?.couple;
      return coupleSkillView(store.getState().couple, {
        kinds: t?.skill_kinds ?? [],
        rows: t?.skill_levels ?? [],
        item: (id) => {
          const p = ctx.data?.item(id);
          return p ? { name: p.name, icon: p.icon !== '' ? `item/${p.icon}` : null } : undefined;
        },
        info: ctx.data?.bundle('help')?.panels['coupleskillinfo'] ?? '',
      });
    },
    [store, dataSource],
    deepEqual,
  );
  const honorPage = derivedView<HonorView>(
    () => honorView(store.getState()),
    [store],
    (a, b) => a.titles === b.titles,
  );
  const campusView = derivedView<CampusView>(
    () => campusViewOf(store.getState().campus, viewFacts()),
    [store, playerData.store, selfStore],
    deepEqual,
  );

  // ---- the ports ------------------------------------------------------------------------------------------------------------------

  const breakUp = (): void => {
    const v = coupleInfo.get();
    if (!v.canBreak) return;
    // `TID_GAME_COUPLECANCEL( partner )`: OK sends the break-up, Cancel only closes
    coupleBox(formatTid(fc.texts, TID_GAME_COUPLECANCEL, [v.name]), () => {
      senders.decouple();
    });
  };
  const couplePorts: CouplePorts = {
    info: coupleInfo,
    skill: coupleSkill,
    actions: {
      breakUp,
      skillPageOpened: () => {
        // the text of the skill page is a panel of the lazy help bundle
        if (!ctx.bundles.has('help')) ctx.bundles.ensure('help').catch(() => undefined);
      },
    },
  };
  const honorPorts: HonorPorts = {
    view: honorPage,
    actions: {
      opened: () => {
        senders.reqHonorList();
      },
      change: (id) => {
        senders.reqHonorTitleChange(id);
      },
    },
  };
  const campusActions: CampusActions = {
    severance: (id) => {
      severance(id, playerData.nameOf(id));
    },
  };
  const campusPorts: CampusTabPorts = { view: campusView, actions: campusActions };

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.couple.setPorts({ couple: couplePorts });
    ui.char.setPorts({ honor: honorPorts });
    ui.messenger.setPorts({ campus: campusPorts });
  };
  ctx.tickSystems.register('relation.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // the header of the messenger draws the campus points of the character; it reads them when its view is computed, so a change asks for a fresh computation
  const readPoints = (): number => store.getState().campusPoints;
  let shownPoints = Number.NaN;
  const syncPoints = (): void => {
    const m = messengerOf(ctx);
    if (!m) return;
    if (shownPoints === readPoints()) return;
    shownPoints = readPoints();
    m.setCampusPoints(readPoints);
  };
  store.subscribe(syncPoints);
  syncPoints();
  ctx.tickSystems.register('relation.points', syncPoints);

  registerRelationProbe(ctx, {
    feature,
    store,
    couple: coupleInfo,
    skill: coupleSkill,
    stats,
    campus: campusView,
    honorView: honorPage,
  });
  return feature;
}
