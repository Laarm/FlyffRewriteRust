// The `party` feature module (Plan/45 §22, §7.6, §7.8, §2.8; ref/09a §5.7; WP-CFEAT-34, -35): the party messages and their texts, the invitation
// and leave windows, the party window with its share modes, the simple display, the name windows of an advanced party, the party skills (the
// taskbar shortcut port, the buff icons of the modes, the auto-sit of Stretching), the finder, the minimap marks of the party, the party-summon
// windows and the player-data cache that all of them read.
//
//   PartyMember                       the record replaces the mirror; the lines of old -> new size; `selfStore.partyId`; the member flags of the movers
//   PartyRequest                      `APP_PARTY_CONFIRM` (Yes `AddPartyMember`, No `MemberRequestCancel{0}`); a second one is answered with `BlockReq{ 7 }`
//   PartyRequestCancel, ErrorParty    the lines and boxes of 45 §22.1
//   PartyExp, SetPartyMode, ...       the state messages (`reducers.ts`), the buff icons of the running modes
//   PartyFinderRefresh                the finder list (only while `APP_PARTY_FIND` is open)
//   SetNaviPointNtf, PartyMapInfo     the party marks and the positions of members out of sight, for the navigator (WP-CFEAT-28)
//   Summon{party confirm}             `APP_MESSAGEBOX_SUMMONPARTY`; the buff icon's double click opens `APP_SUMMON_PARTY_USE`
//   QueryPlayerDataNtf, UpdatePlayerData, PresenceLogout, QuerySetPlayerNameNtf   the player-data cache (`features/common/playerData.ts`)
//
// Other features reach it with `partyOf(ctx)`: the store, the view sources, `invite`, `skillShortcut`, `naviClick`, `marks`.

import type { ClientContext } from '../../app/context';
import { entityInfoOf, selfOf } from '../../app/inputHost';
import { EntityKind, type EntityRecord } from '../../game/entities';
import { OBJMSG, STA } from '../../game/combat/constants';
import { MMI_INVITE_PARTY } from '../../gen/defs/neuz';
import { APP_PARTY_FIND, APP_PARTY_QUICK } from '../../gen/defs/resdata';
import {
  II_CHR_SYS_SCR_PSKILLFULL1,
  II_SYS_SYS_SCR_PARTYEXPUP01_01,
  II_SYS_SYS_SCR_PARTYSKILLUP01,
  II_SYS_SYS_SCR_PARTYSKILLUP01_01,
  II_SYS_SYS_SCR_PARTYSKILLUP02,
  II_SYS_SYS_SCR_PARTYSUMMON,
} from '../../gen/defs/item';
import {
  TID_DIAG_0004,
  TID_DIAG_0007,
  TID_DIAG_0008,
  TID_DIAG_0072,
  TID_DIAG_0084,
  TID_DIAG_0085,
  TID_DIAG_RULE_0,
  TID_DIAG_SUMMONPARTY_CONFIRM,
  TID_DIAG_SUMMONPARTY_CONFIRM_USE,
  TID_DIAG_SUMMONPARTY_USE,
  TID_ERROR_SUMMONPARTY_NOITEM,
  TID_GAME_FULLPARTY3,
  TID_GAME_MASTER_AWAY,
  TID_GAME_PARTY_ALREADYMASTER,
  TID_GAME_PARTY_ISNOTMASTER,
  TID_GAME_PARTY_NAME,
  TID_GAME_PPVP_ADDPARTY,
  TID_GAME_PPVP_LEAVE,
  TID_GAME_SUMMONPARTY,
  TID_PK_NOCHANGE_PARTYLEADER,
} from '../../gen/defs/text';
import type { SpawnPlayer } from '../../gen/proto/flyff/v1/world_pb';
import type { InputInstance } from '../../input/input';
import { REQ_USESKILL } from '../../input/worldTick';
import type { Rect } from '../../ui/layout/geometry';
import { parseMarkup } from '../../ui/text/markup';
import {
  INFO_COL,
  INFO_ROW_PITCH,
  infoRowY,
  skillCell,
} from '../../ui/windows/party/partyGeometry';
import type { TroupeTooltipComposer } from '../../ui/tooltip/types';
import type {
  FinderPorts,
  FinderView,
  PartyActions,
  PartyPorts,
  PartyView,
  ViewSource,
} from '../../ui/windows/party/ports';
import { BuffSet, type BuffRules } from '../buffs/buffSet';
import { buffsOf } from '../buffs/buffsModule';
import { BUFF_ITEM2, BUFF_SKILL } from '../buffs/types';
import { actionSlotOf } from '../actionslot/actionSlotModule';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { nameRulesBytes } from '../common/lobbyRules';
import { registerPlayerData, type PlayerDataApi } from '../common/playerData';
import { addProbeMember } from '../common/probe';
import { formatTid, sysChat } from '../common/text';
import { inventoryOf } from '../inventory';
import { sendMotion } from '../motions/senders';
import { messengerOf } from '../messenger/api';
import { menuOf } from '../npc/menuModule';
import { duelOf } from '../pvp/duel/duelModule';
import { checkPartyName } from './names';
import {
  activeMarks,
  finderEntry,
  onAllowJoin,
  onChangeLeader,
  onChangeName,
  onChangeTroup,
  onErrorParty,
  onExpMode,
  onItemMode,
  onMapInfo,
  onMemberParam,
  onNaviPoint,
  onOwnName,
  onPartyExp,
  onPartyMember,
  onPartyRequestCancel,
  onSetPartyMode,
  type PartyEffect,
  type PartyEnv,
  type Reduced,
} from './reducers';
import { partySenders, type PartySenders } from './senders';
import { type PartySkillFacts, partySkillOutcome, type PartySkillOutcome } from './skills';
import {
  definePartyStore,
  findMember,
  isLeader,
  type NaviMark,
  type PartyState,
  type PartyStore,
  skillOfMode,
  ST,
} from './store';
import {
  buildFinderView,
  buildPartyView,
  liveView,
  type CachedFacts,
  type MoverFacts,
  type ViewFacts,
} from './view';

/** The public face of the party feature of a context. */
export interface PartyFeature {
  readonly store: PartyStore;
  readonly playerData: PlayerDataApi;
  readonly senders: PartySenders;
  /** The read model of the party, quick and skill windows. */
  readonly view: ViewSource<PartyView>;
  /** The read model of the finder. */
  readonly finderView: ViewSource<FinderView>;
  /** The party-skill buff icons (`m_buffs`). */
  readonly buffs: BuffSet;
  /** The own character leads the party. */
  isLeader(): boolean;
  /** `InviteParty`: invite the player with this object id (the player menu); `false` when the client refuses (duel, not the leader, full). */
  invite(targetObjId: number): boolean;
  /** `InviteParty( idPlayer )` by character id (the friend menu of the messenger); the same refusals. */
  inviteById(idPlayer: number): boolean;
  /** The taskbar shortcut port for a party skill; returns what it did. */
  skillShortcut(id: number): PartySkillOutcome;
  /** The minimap click of 45 §7.6: `SetNaviPointReq` for the focused player, else for the party (`target` 0). Returns what was sent. */
  naviClick(pos: readonly [number, number, number]): { sent: boolean; target: number };
  /** The party marks that are still drawn (set by `SetNaviPointNtf`, about 3.3 s old at most). */
  marks(): readonly NaviMark[];
  /** The position of a member for the navigator and the world map: the live one in view, else the last `PartyMapInfo`. */
  memberPosition(idPlayer: number): readonly [number, number, number] | null;
  /** The own party actions (the ports of the windows). */
  readonly actions: PartyActions;
  /** Opens the finder window (the applet U). */
  openFinder(): void;
}

const features = new WeakMap<ClientContext, PartyFeature>();

/** The party feature of a context; `undefined` before the module registered. */
export function partyOf(ctx: ClientContext): PartyFeature | undefined {
  return features.get(ctx);
}

const NO_RULES: BuffRules = { skillIsEvil: () => undefined, itemIsPermanent: () => false };
const NULL_ID = 0xffff_ffff;

export const partyModule: FeatureModule = {
  id: 'party',
  milestone: 5,
  register(fc) {
    registerParty(fc);
  },
};

/** `PutToolTip_Troupe` (Plan/42 §9.8): the name, the MP / FP cost and the description of a party skill. */
export function createTroupeComposer(data: () => ClientContextData | null): TroupeTooltipComposer {
  return (id) => {
    const p = data()?.partySkill(id);
    if (!p) return null;
    let s = `#b#cff2fbe6d${p.name}#nb#nc`;
    if (p.req_mp >>> 0 !== NULL_ID) s += `\nMP : ${String(p.req_mp)}`;
    if (p.req_fp >>> 0 !== NULL_ID) s += `\nFP : ${String(p.req_fp)}`;
    if (p.description !== '=' && p.description !== '') s += `\n${p.description}`;
    return { runs: parseMarkup(s) };
  };
}

type ClientContextData = NonNullable<ClientContext['data']>;

function registerParty(fc: FeatureContext): PartyFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const store = definePartyStore(ctx.stores.registry);
  const playerData = registerPlayerData(fc);
  const senders = partySenders((msg) => ctx.net.sender.send(msg));
  const buffs = new BuffSet();
  const info = entityInfoOf(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const sysLine = (tid: number, args: readonly string[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };
  const nowUnix = (): number => Math.floor(Date.now() / 1000);

  // ---- the client as the reducers see it ----------------------------------------------------------------------------------

  const selfId = (): number => selfStore.getState().idPlayer;
  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  const playerByCharacter = (idPlayer: number): EntityRecord | null => {
    let found: EntityRecord | null = null;
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (found === null && !rec.isSelf && rec.idPlayer === idPlayer) found = rec;
    });
    return found;
  };
  const characterOf = (rec: EntityRecord): number | undefined =>
    rec.idPlayer ?? (rec.spawn as SpawnPlayer).player?.idPlayer;
  const moverFacts = (id: number): MoverFacts | null => {
    if (id === selfId()) {
      const s = selfStore.getState();
      return { hp: s.hp, maxHp: s.maxHp, name: s.name, level: s.level, job: s.job };
    }
    const rec = playerByCharacter(id);
    if (!valid(rec ?? undefined) || !rec) return null;
    const p = (rec.spawn as SpawnPlayer).player;
    return {
      hp: rec.hp,
      maxHp: rec.maxHp ?? 0,
      name: rec.visual?.name ?? p?.name ?? '',
      level: p?.level ?? 1,
      job: p?.job ?? 0,
    };
  };
  const nameOf = (id: number): string => {
    if (id === selfId()) return selfStore.getState().name;
    const rec = playerByCharacter(id);
    if (valid(rec ?? undefined) && rec?.visual?.name) return rec.visual.name;
    return playerData.get(id)?.name ?? '';
  };
  const env = (): PartyEnv => ({
    selfId: selfId(),
    now: ctx.now(),
    text,
    skillName: (id) => ctx.data?.partySkill(id)?.name ?? '',
    nameOf,
    visible: (id) => id === selfId() || moverFacts(id) !== null,
  });

  // ---- the read models ----------------------------------------------------------------------------------------------------

  const cachedFacts = (id: number): CachedFacts | undefined => {
    const e = playerData.get(id);
    return e ? { name: e.name, level: e.level, job: e.job, answered: e.ver !== 0 } : undefined;
  };
  const facts = (): ViewFacts => ({
    party: store.getState(),
    selfId: selfId(),
    now: ctx.now(),
    text,
    mover: moverFacts,
    cached: cachedFacts,
    jobName: (job) => ctx.data?.job(job)?.name ?? '',
    expParty: ctx.data?.bundle('tables')?.exp.party ?? [],
    partySkill: (id) => ctx.data?.partySkill(id),
    dueling: (duelOf(ctx)?.store.getState().duel ?? 0) === 2,
    findEnabled: ctx.features.features.party_find,
  });
  const playerDataStore = playerData.store;
  // a local tick store: the remaining time of a mode and the HP of the members change without a store write
  const view: ViewSource<PartyView> = liveView(
    () => buildPartyView(facts()),
    [store, playerDataStore],
  );
  const flagIcon = (id: number, dir: 'item' | 'icon'): string | null => {
    const p = ctx.data?.item(id);
    return p && p.icon !== '' ? `${dir}/${p.icon}` : null;
  };
  const partyIcon = (skill: number): string | null => {
    const p = ctx.data?.partySkill(skill);
    return p && p.icon !== '' ? `icon/${p.icon}` : null;
  };
  const finderView: ViewSource<FinderView> = liveView(
    () =>
      buildFinderView(store.getState().finder, [
        flagIcon(II_CHR_SYS_SCR_PSKILLFULL1, 'item'),
        flagIcon(II_SYS_SYS_SCR_PARTYEXPUP01_01, 'item'),
        flagIcon(II_SYS_SYS_SCR_PARTYSKILLUP01, 'item'),
        partyIcon(ST.LINKATTACK),
        partyIcon(ST.FORTUNECIRCLE),
        partyIcon(ST.GIFTBOX),
      ]),
    [store],
  );

  // ---- carrying out the effects of a reducer ------------------------------------------------------------------------------

  /** The `partyMember` flag of the movers: members get the party colour and the over-head HP bar of 41. */
  const markMembers = (): void => {
    const ids = new Set(store.getState().members.map((m) => m.id));
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (!rec.visual) return;
      const id = characterOf(rec);
      rec.visual.partyMember = !rec.isSelf && id !== undefined && ids.has(id);
    });
  };

  /** The icons of the running modes 0..3 from the record of the party (the remaining time travels with it). */
  const syncModeBuffs = (): void => {
    const s = store.getState();
    const now = ctx.now();
    buffs.clear();
    for (let mode = 0; mode < 4; mode += 1) {
      const end = s.modeEnds[mode] ?? 0;
      const skill = skillOfMode(mode);
      if (s.modes[mode] === 1 && end > now && skill > 0) {
        buffs.add(BUFF_SKILL, skill, 1, end - now, now, NO_RULES);
      }
    }
  };

  /**
   * STRETCHING started and the own character is not already sitting, jumping, attacking, hurt or busy with an action or a skill request:
   * `ClearDest(); SendMotion( OBJMSG_SITDOWN )` (`OnSetPartyMode`; the server owns the sitting regeneration).
   */
  const selfInfo = selfOf(ctx);
  const autoSit = (): void => {
    const me = selfInfo();
    if (!me || me.dead || me.sitting) return;
    const reqs = (ctx.input as Partial<InputInstance>).core?.reqFlags ?? 0;
    const busy =
      (me.state & (STA.JUMP_ALL | STA.ATK_ALL | STA.DMG_ALL | STA.ACTION_ALL)) !== 0 ||
      (reqs & REQ_USESKILL) !== 0;
    if (busy) return;
    (ctx.input as Partial<InputInstance>).core?.commands.clearDest();
    sendMotion(ctx, OBJMSG.SITDOWN);
  };

  const openNameWindow = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    const s = store.getState();
    ui.party.openChangeName({
      initial: s.ownName !== '' ? s.ownName : text(TID_GAME_PARTY_NAME),
      submit: (name) => submitName(name),
    });
  };

  /** `CWndPartyChangeName::OnChildNotify( WIDC_OK )`: the checks, then `ChangeTroup` (a normal party) or `ChanPartyName`. */
  const submitName = (raw: string): boolean => {
    const ui = fullUi(ctx);
    const name = raw.trim();
    const names = ctx.data?.bundle('names');
    const tid =
      name === ''
        ? TID_DIAG_RULE_0
        : checkPartyName(name, fc.lobbyRules, names ? nameRulesBytes(names) : null);
    if (tid !== 0) {
      ui?.msgbox.open(text(tid));
      return false;
    }
    // the default name is a prompt, not a name: the window stays
    if (name === text(TID_GAME_PARTY_NAME)) return false;
    if (store.getState().kind === 0) senders.changeTroup(name);
    else senders.chanPartyName(name);
    return true;
  };

  const run = (effects: readonly PartyEffect[]): void => {
    const ui = fullUi(ctx);
    for (const e of effects) {
      switch (e.kind) {
        case 'line':
          sysLine(e.tid, e.args);
          break;
        case 'box':
          ui?.msgbox.open(text(e.tid));
          break;
        case 'openChangeName':
          // the window first, the box over it (`OpenMessageBox` replaces nothing of the window)
          openNameWindow();
          if (e.box !== null) ui?.msgbox.open(text(e.box));
          break;
        case 'clearBuffs':
          buffs.clear();
          break;
        case 'buffOn':
          buffs.add(BUFF_SKILL, e.skill, 1, e.ms, ctx.now(), NO_RULES);
          break;
        case 'buffOff':
          buffs.remove(BUFF_SKILL, e.skill);
          break;
        case 'selfParty':
          if (selfStore.getState().partyId !== e.id) selfStore.setState({ partyId: e.id });
          break;
        case 'duelParty': {
          const self = selfStore.getState().objId;
          duelOf(ctx)?.book.set(self, { duel: 2, party: e.id });
          break;
        }
        case 'sit':
          autoSit();
          break;
      }
    }
  };

  const apply = (r: Reduced): void => {
    store.setState(r.state);
    run(r.effects);
  };

  // ---- the messages --------------------------------------------------------------------------------------------------------

  fc.net.on('partyMember', (m) => {
    const before = store.getState().members.length;
    const r = onPartyMember(
      store.getState(),
      { changed: m.changed, leaderName: m.leaderName, memberName: m.memberName, party: m.party },
      env(),
    );
    apply(r);
    const s = store.getState();
    // the names and levels of the members out of view come from the cache: ask for the ones it does not know
    if (s.members.length > 0) playerData.needAll(s.members.map((x) => x.id));
    markMembers();
    if (r.effects.some((e) => e.kind === 'selfParty' && e.id !== 0)) syncModeBuffs();
    // the simple display needs two members (`SetActiveMember` of a dissolved party would show an empty frame)
    if (s.members.length < 2 && before >= 2) fullUi(ctx)?.wm.close(APP_PARTY_QUICK);
  });

  fc.net.on('partyRequest', (m) => {
    // the privacy option "party" is the server's since M6 (`accepts_party_invite`, PL-095): an invitation that reaches the client is for a window
    const ui = fullUi(ctx);
    if (!ui) return;
    // a window is open already: the leader is told (`SendBlock( 7 )`) and the newcomer gets no window
    if (ui.party.isConfirmOpen()) {
      messengerOf(ctx)?.sendBlock(7, m.leaderName);
      return;
    }
    ui.party.openConfirm({
      text: formatTid(fc.texts, TID_DIAG_0072, [m.leaderName]),
      yes: () => {
        senders.addPartyMember(m.leaderId);
      },
      no: () => {
        senders.memberRequestCancel(m.leaderId, 0);
      },
    });
  });

  fc.net.on('partyRequestCancel', (m) => {
    run(
      onPartyRequestCancel(
        store.getState(),
        { leaderId: m.leaderId, memberId: m.memberId, mode: m.mode },
        env(),
      ),
    );
  });

  fc.net.on('errorParty', (m) => {
    run(onErrorParty(store.getState(), m.code));
  });

  fc.net.on('partyExp', (m) => {
    apply(onPartyExp(store.getState(), { exp: m.exp, level: m.level, point: m.point }));
  });

  fc.net.on('addPartyChangeLeader', (m) => {
    apply(onChangeLeader(store.getState(), m.newLeader));
  });

  fc.net.on('setPartyMode', (m) => {
    apply(
      onSetPartyMode(
        store.getState(),
        { mode: m.mode, timeMs: m.timeMs, on: m.on, point: m.point },
        env(),
      ),
    );
  });

  fc.net.on('partyChangeItemModeNtf', (m) => {
    store.setState(onItemMode(store.getState(), m.mode));
  });
  fc.net.on('partyChangeExpModeNtf', (m) => {
    store.setState(onExpMode(store.getState(), m.mode));
  });
  fc.net.on('partyChangeName', (m) => {
    apply(onChangeName(store.getState(), m.name, env()));
  });
  fc.net.on('partyChangeTroup', (m) => {
    apply(onChangeTroup(store.getState(), m.name));
    // the party skills are usable now: a normal party had none running
    buffs.clear();
  });
  fc.net.on('setPartyMemberParam', (m) => {
    const before = store.getState().members.length;
    apply(
      onMemberParam(store.getState(), { idPlayer: m.idPlayer, param: m.param, value: m.value }),
    );
    if (store.getState().members.length < 2 && before >= 2) {
      fullUi(ctx)?.wm.close(APP_PARTY_QUICK);
    }
  });
  fc.net.on('partyAllowJoinNtf', (m) => {
    store.setState(onAllowJoin(store.getState(), m.allow));
  });
  fc.net.on('addPartyName', (m) => {
    store.setState(onOwnName(store.getState(), m.name));
  });
  fc.net.on('partyMapInfo', (m) => {
    const p = m.pos;
    if (!p) return;
    store.setState(
      onMapInfo(store.getState(), { index: m.memberIndex, pos: [p.x, p.y, p.z] }, env()),
    );
  });
  fc.net.on('setNaviPointNtf', (m, objId) => {
    const p = m.pos;
    if (!p) return;
    store.setState(
      onNaviPoint(store.getState(), {
        objId,
        name: m.senderName,
        pos: [p.x, p.y, p.z],
        at: ctx.now(),
      }),
    );
  });
  fc.net.on('partyFinderRefresh', (m) => {
    // applied only while the finder is open (`OnRefreshPartyList` finds the applet or returns)
    if (!fullUi(ctx)?.party.find()) return;
    store.setState({
      finder: m.entries.map(finderEntry),
      finderSeq: store.getState().finderSeq + 1,
    });
  });
  // a player who appears: the member flag (the colour and the HP bar) from the start
  fc.net.on('spawnPlayer', () => {
    markMembers();
  });

  // ---- the minimap marks ---------------------------------------------------------------------------------------------------

  const targeting = (): InputInstance['core']['targeting'] | null =>
    (ctx.input as Partial<InputInstance>).core?.targeting ?? null;

  const naviClick = (pos: readonly [number, number, number]): { sent: boolean; target: number } => {
    const focus = targeting()?.focus ?? null;
    const rec = focus === null ? undefined : ctx.entities.get(focus);
    // 45 §7.6: the focus is a player -> that player (and the sender); else a party -> the whole party
    if (valid(rec) && rec.kind === EntityKind.Player) {
      senders.naviPoint(pos, rec.objId);
      return { sent: true, target: rec.objId };
    }
    if (store.getState().members.length > 0) {
      senders.naviPoint(pos, 0);
      return { sent: true, target: 0 };
    }
    return { sent: false, target: 0 };
  };

  const memberPosition = (id: number): readonly [number, number, number] | null => {
    if (id === selfId()) return selfStore.getState().pos;
    const live = info(playerByCharacter(id)?.objId ?? 0)?.pos;
    if (live) return live;
    return store.getState().mapPos[id] ?? null;
  };

  // ---- the party skills ----------------------------------------------------------------------------------------------------

  const skillFacts = (id: number): PartySkillFacts => {
    const s = store.getState();
    const prop = ctx.data?.partySkill(id);
    const b = buffsOf(ctx);
    return {
      size: s.members.length,
      kind: s.kind,
      leader: isLeader(s, selfId()),
      level: s.level,
      point: s.point,
      prop: prop ? { reqLevel: prop.req_dis_lv, cost: prop.exp, name: prop.name } : undefined,
      running: buffs.has(BUFF_SKILL, id),
      skillUp: [
        II_SYS_SYS_SCR_PARTYSKILLUP01,
        II_SYS_SYS_SCR_PARTYSKILLUP02,
        II_SYS_SYS_SCR_PARTYSKILLUP01_01,
      ].some((item) => b?.has(BUFF_ITEM2, item) === true),
    };
  };

  const skillShortcut = (id: number): PartySkillOutcome => {
    const out = partySkillOutcome(id, skillFacts(id));
    switch (out.kind) {
      case 'box':
        fullUi(ctx)?.msgbox.open(text(out.tid));
        break;
      case 'line':
        sysLine(out.tid, out.args);
        break;
      case 'send':
        senders.skillUse(out.skill);
        break;
      case 'none':
        break;
    }
    return out;
  };

  // ---- the actions of the windows ------------------------------------------------------------------------------------------

  const leader = (): boolean => isLeader(store.getState(), selfId());
  const dueling = (): boolean => (duelOf(ctx)?.store.getState().duel ?? 0) === 2;

  const actions: PartyActions = {
    focusMember(id) {
      const t = targeting();
      if (!t) return;
      if (id === null || id === selfId()) {
        t.setFocus(null);
        return;
      }
      const rec = playerByCharacter(id);
      if (valid(rec ?? undefined) && rec) t.setFocus(rec.objId);
    },
    leave(selected) {
      // `m_nDuel == 2`: no leaving during a party duel
      if (dueling()) {
        sysLine(TID_GAME_PPVP_LEAVE);
        return;
      }
      const s = store.getState();
      if (s.members.length === 0) return;
      const me = selfId();
      const leaveId = selected !== null && isLeader(s, me) ? selected : me;
      fullUi(ctx)?.party.openLeaveConfirm({
        text: formatTid(fc.texts, leaveId === me ? TID_DIAG_0084 : TID_DIAG_0085),
        yes: () => {
          senders.removePartyMember(leaveId);
        },
      });
    },
    transferLeader(selected) {
      const s = store.getState();
      if (s.members.length === 0) return;
      const i = selected === null ? -1 : findMember(s, selected);
      const target = s.members[i];
      if (!leader() || !target || selected === null) {
        sysLine(TID_GAME_PARTY_ISNOTMASTER);
        return;
      }
      if (target.offline) {
        sysLine(TID_GAME_MASTER_AWAY);
        return;
      }
      if (isLeader(s, selected)) {
        sysLine(TID_GAME_PARTY_ALREADYMASTER);
        return;
      }
      if (dueling()) {
        sysLine(TID_PK_NOCHANGE_PARTYLEADER);
        return;
      }
      senders.changeLeader(selected);
    },
    openChange() {
      // the "Change" button: a normal party asks, an advanced one has nothing to change
      if (store.getState().kind !== 0) return;
      fullUi(ctx)?.party.openChangeTroup({
        ok: () => {
          openNameWindow();
        },
      });
    },
    setItemShare(mode) {
      if (!leader()) {
        sysLine(TID_DIAG_0008);
        return;
      }
      if (mode !== store.getState().shareItem) senders.changeItemMode(mode);
    },
    setExpShare(mode) {
      if (!leader()) {
        sysLine(TID_DIAG_0008);
        return;
      }
      const s = store.getState();
      // only an advanced party shares by contribution (`m_nKindTroup != 0`); `TID_DIAG_0004` is the dead third mode
      if (s.kind !== 0 && mode !== s.shareExp) {
        if (mode === 2) sysLine(TID_DIAG_0004);
        else senders.changeExpMode(mode);
      }
    },
    setAllowJoin(allow) {
      senders.allowJoin(allow);
    },
    toggleQuick() {
      fullUi(ctx)?.party.toggleQuick();
    },
    toggleParty() {
      fullUi(ctx)?.party.toggleParty();
    },
    useSkill(id) {
      skillShortcut(id);
    },
    skillTooltip(id, rect) {
      const ui = fullUi(ctx);
      if (!ui) return;
      const owner = ui.party.party()?.skillPage;
      if (!rect || !owner) {
        ui.tooltip.cancel();
        return;
      }
      ui.tooltip.putTroupe(owner, id, rect);
    },
  };

  const finderActions: FinderPorts['actions'] = {
    refresh() {
      senders.joinPartyReq();
    },
    join(leaderId) {
      senders.addPartyMember(leaderId);
    },
  };

  // ---- the invitation from the player menu --------------------------------------------------------------------------------

  const inviteById = (target: number): boolean => {
    const s = store.getState();
    // `CWndWorld::InviteParty`
    if (dueling()) {
      sysLine(TID_GAME_PPVP_ADDPARTY);
      return false;
    }
    if (s.members.length === 0 || isLeader(s, selfId())) {
      if (8 >= s.members.length + 1) {
        senders.memberRequest(target, false);
        return true;
      }
      sysLine(TID_GAME_FULLPARTY3);
      return false;
    }
    fullUi(ctx)?.msgbox.open(text(TID_DIAG_0007));
    return false;
  };

  const invite = (targetObjId: number): boolean => {
    const rec = ctx.entities.get(targetObjId);
    const target = valid(rec) && rec.kind === EntityKind.Player ? characterOf(rec) : undefined;
    return target === undefined ? false : inviteById(target);
  };

  const menu = menuOf(ctx);
  menu?.registerAction(MMI_INVITE_PARTY, ({ targetId }) => {
    invite(targetId);
  });
  menu?.addPlayerEnv(() => ({ partyLeader: leader() }));

  // ---- the party summon ------------------------------------------------------------------------------------------------------

  /** The request of a member's notice: the leader's object id and the scroll word to answer with (`g_Neuz.m_dwSummonParty*`). */
  let summon: { leaderObj: number; data: number; world: string } | null = null;

  fc.net.on('summon', (m) => {
    if (m.sub.case !== 'partyConfirm') return;
    const ui = fullUi(ctx);
    const c = m.sub.value;
    summon = { leaderObj: c.leaderObj, data: c.data, world: c.worldName };
    if (!ui) return;
    const scroll = ctx.data?.item(II_SYS_SYS_SCR_PARTYSUMMON);
    ui.party.openSummonMsg({
      text: formatTid(fc.texts, TID_DIAG_SUMMONPARTY_CONFIRM, [view.get().nameText]),
      how: formatTid(fc.texts, TID_DIAG_SUMMONPARTY_CONFIRM_USE, [scroll?.name ?? '']),
      icon: scroll && scroll.icon !== '' ? `item/${scroll.icon}` : null,
    });
  });

  // a double click on the buff icon of the scroll: the member's question (`CWndWorld::OnLButtonDblClk`)
  const onBuffSummon = (): void => {
    const ui = fullUi(ctx);
    if (!summon || !ui || summon.leaderObj === 0 || summon.leaderObj === selfStore.getState().objId)
      return;
    const pending = summon;
    ui.party.openSummonUse({
      text: formatTid(fc.texts, TID_DIAG_SUMMONPARTY_USE, [pending.world]),
      ok: () => {
        senders.summonPartyConfirm(pending.leaderObj, pending.data);
      },
    });
  };
  const b = buffsOf(ctx);
  if (b) {
    b.partyBuffs = buffs;
    b.onPartySummon = onBuffSummon;
  }

  // the leader used the scroll: `APP_SUMMON_PARTY` (the inventory dispatch asks for the window, rule R27)
  inventoryOf(ctx)?.registerDispatchWindows({
    summonParty: (item) => {
      const ui = fullUi(ctx);
      if (!ui) return;
      const world = ctx.data?.world(selfStore.getState().worldId)?.title ?? '';
      ui.party.openSummon({
        text: formatTid(fc.texts, TID_GAME_SUMMONPARTY, [world]),
        ok: () => {
          const scroll = ctx.data?.item(II_SYS_SYS_SCR_PARTYSUMMON);
          // `GetItemId( wId )`: the scroll must still be in the bag
          if (inventoryOf(ctx)?.item(item.objId)) {
            // `MAKELONG( wId, 0 )`
            senders.summonParty(item.objId & 0xffff);
          } else {
            sysLine(TID_ERROR_SUMMONPARTY_NOITEM, [scroll?.name ?? '']);
          }
        },
      });
    },
  });

  // ---- the party skills on the taskbar ----------------------------------------------------------------------------------------

  const slot = actionSlotOf(ctx);
  if (slot) {
    slot.partySkillUse = (id) => {
      skillShortcut(id);
    };
  }

  // ---- the windows -------------------------------------------------------------------------------------------------------------

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    const ports: { party: PartyPorts; find: FinderPorts } = {
      party: { view, actions },
      find: { view: finderView, actions: finderActions },
    };
    ui.party.setPorts(ports);
    ui.tooltip.registerComposers({ troupe: createTroupeComposer(() => ctx.data) });
  };
  ctx.tickSystems.register('party.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // the icons of the modes run out by the clock; a world that was left takes the party with it
  ctx.tickSystems.register('party.buffs', () => {
    buffs.process(ctx.now(), nowUnix(), NO_RULES);
  });
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
      buffs.clear();
      summon = null;
    }
  });

  const feature: PartyFeature = {
    store,
    playerData,
    senders,
    view,
    finderView,
    buffs,
    isLeader: leader,
    invite,
    inviteById,
    skillShortcut,
    naviClick,
    marks: () => activeMarks(store.getState().marks, ctx.now()),
    memberPosition,
    actions,
    openFinder: () => {
      fullUi(ctx)?.wm.open(APP_PARTY_FIND);
    },
  };
  features.set(ctx, feature);

  // ---- the debug probe -----------------------------------------------------------------------------------------------------------

  addProbeMember(ctx, 'party', () => {
    const s: PartyState = store.getState();
    return {
      id: s.id,
      kind: s.kind,
      name: s.name,
      level: s.level,
      exp: s.exp,
      point: s.point,
      shareExp: s.shareExp,
      shareItem: s.shareItem,
      modes: s.modes,
      allowJoin: s.allowJoin,
      ownName: s.ownName,
      leader: leader(),
      members: s.members.map((m) => ({
        id: m.id,
        offline: m.offline,
        name: nameOf(m.id),
        level: playerData.get(m.id)?.level ?? 0,
      })),
      marks: feature.marks(),
      mapPos: s.mapPos,
      finder: s.finder,
      buffs: buffs.list().map((x) => ({ id: x.id, total: x.total })),
      view: view.get(),
      quickOpen: fullUi(ctx)?.party.quick() != null,
      partyOpen: fullUi(ctx)?.party.party() != null,
      confirmOpen: fullUi(ctx)?.party.isConfirmOpen() ?? false,
      findOpen: fullUi(ctx)?.party.find() != null,
    };
  });
  // the CSS pixel centres of the rows and cells the specs click (the specs read the probe, not pixels)
  addProbeMember(ctx, 'partyPoints', () => {
    const ui = fullUi(ctx);
    if (!ui) return null;
    const scale = ui.scaler.get().scale;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    const css = (origin: Rect, r: Rect): { x: number; y: number } => ({
      x: Math.round((root?.left ?? 0) + (origin.l + (r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + (origin.t + (r.t + r.b) / 2) * scale),
    });
    const w = ui.party.party();
    const info = w?.infoPage;
    const skills = w?.skillPage;
    const quick = ui.party.quick();
    const find = ui.party.find();
    const members = view.get().members;
    const menu = ui.npc.moverMenu;
    return {
      // the entries of the player menu that is open (the invitation is one of them)
      menu: menu.shown
        ? menu.items.map((item) => ({
            id: item.id,
            text: item.getTitle(),
            ...css({ l: 0, t: 0, r: 0, b: 0 }, item.getScreenRect()),
          }))
        : [],
      // the two tab captions of the party window (the strip is the last 18 px of the tab control)
      tabs: w?.tab
        ? [0, 1].map((i) => {
            const r = w.tab?.getScreenRect() ?? { l: 0, t: 0, r: 0, b: 0 };
            const width = r.r - r.l;
            return css(
              { l: 0, t: 0, r: 0, b: 0 },
              {
                l: r.l + (width * i) / 2,
                t: r.b - 18,
                r: r.l + (width * (i + 1)) / 2,
                b: r.b,
              },
            );
          })
        : [],
      rows: info
        ? members.map((m, i) => ({
            index: i,
            id: m.id,
            ...css(info.getScreenClientRect(), {
              l: INFO_COL.square,
              t: infoRowY(i),
              r: INFO_COL.gauge,
              b: infoRowY(i) + INFO_ROW_PITCH,
            }),
          }))
        : [],
      quick: quick
        ? members.map((m, i) => ({
            index: i,
            id: m.id,
            ...css(
              quick.getScreenClientRect(),
              quick.getWndCtrl(`WIDC_STATIC_MEM${String(i + 1)}`)?.rect ?? {
                l: 0,
                t: 0,
                r: 0,
                b: 0,
              },
            ),
          }))
        : [],
      skills: skills
        ? view.get().skills.map((sk) => {
            const c = skills.getClientRect(false);
            return {
              id: sk.id,
              ...css(skills.getScreenClientRect(), skillCell(sk.id - 1, c.r - c.l, c.b - c.t)),
            };
          })
        : [],
      finder: find?.list
        ? find.list.rows.map((row, i) => ({
            partyId: row.partyId,
            ...css(find.list?.getScreenClientRect() ?? { l: 0, t: 0, r: 0, b: 0 }, {
              l: 0,
              t: 2 + i * 20,
              r: 400,
              b: 2 + i * 20 + 20,
            }),
          }))
        : [],
    };
  });
  addProbeMember(ctx, 'playerData', (id?: number) => {
    const entries = playerData.store.getState().entries;
    if (id !== undefined) return entries.get(id) ?? null;
    return [...entries.entries()].map(([k, v]) => ({ id: k, ...v }));
  });
  addProbeMember(ctx, 'partyInvite', (objId: number) => invite(objId));
  addProbeMember(ctx, 'partySkill', (id: number) => skillShortcut(id));
  addProbeMember(ctx, 'partyNavi', (x: number, y: number, z: number) => naviClick([x, y, z]));
  return feature;
}
