// The `guild` feature module (Plan/45 §23, §2.5, §8.6, §11.1-11.2, §14.1; ref/09b §3.7; WP-CFEAT-37, -38): the guild messages and their texts, the guild window with its five pages, the
// invitation, the first naming and the rename scroll, the notice, the logo, the disband question, the salaries, the contribution window, the nickname, the member menu, the guild bank
// with its log and the cloak button, the guild-war dialogs, the visuals of the movers (`visual.guild`, `visual.idWar`) and the player-data queries of the members.
//
//   CreateGuild, DestroyGuildNtf, SetGuild, Guild, AllGuilds    the guild table and the own record (`reducers.ts`); `SetGuild` of a mover is its `visual.guild`
//   Add/RemoveGuildMemberNtf, GuildMemberLevelNtf, ...          the roster; the lines of 45 §23
//   GuildBankWndNtf, Put/GetItemGuildBankNtf, ...               the bank window, the client-side adds of the original (`item_mine` into the bag, `penya_mine` into the purse)
//   DeclGuildWarNtf, AcptGuildWarNtf, War, SetWar, ...          the war windows and lines
//
// Other features reach it with `guildOf(ctx)`: the store, the selectors (`ownGuildId`, `rankOf`, `hasPower`, `nameOf`, `selfWar`, `isOnline`) and the seams of the siege windows
// (`setEnv( { siegeBusy, siegeApplied } )`).

import { create } from '@bufbuild/protobuf';
import { AUTH_GAMEMASTER } from '../../app/hosts';
import { EntityKind, type EntityRecord } from '../../game/entities';
import { IK3_GEM } from '../../gen/defs/itemkind';
import { MMI_GUILDBANKING, MMI_INVITE_COMPANY } from '../../gen/defs/neuz';
import {
  APP_GUILD,
  APP_GUILD_BANK,
  APP_GUILD_BANK_LOG,
  APP_TRADE_GOLD,
} from '../../gen/defs/resdata';
import {
  TID_GAME_CANNT_USE_GLOG,
  TID_GAME_EQUIPPUT,
  TID_GAME_GUILD_APPELLATIONDOWN,
  TID_GAME_GUILD_APPELLATIONUP,
  TID_GAME_GUILD_CLASSDOWN,
  TID_GAME_GUILD_CLASSUP,
  TID_GAME_GUILD_LEAVE,
  TID_GAME_GUILD_NICKNAME,
  TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD,
  TID_GAME_GUILDCOMBAT_NOT_LEAVE_GUILD,
  TID_GAME_CHG_MASTER,
  TID_GAME_GUILDNOTCHGNAME,
  TID_GAME_GUILDNOTHINGNAME,
  TID_GAME_GUILDNOTLEVEL,
  TID_GAME_GUILDONLYMASTER,
  TID_GAME_GUILDONLYNUMBER,
  TID_GAME_GUILDSTILLLOGO,
  TID_GAME_GUILDWARERRORDISBAND,
  TID_GAME_GUILDWARERRORLOGO,
  TID_GAME_GUILDWARERRORMEMBER,
  TID_GAME_GUILDWARREQUEST,
  TID_GAME_MOVECOUNT,
  TID_GAME_MOVEPENYA,
  TID_GAME_NOTLOGIN,
  TID_GAME_NOTTRADE,
  TID_GAME_NUMCOUNT,
  TID_GAME_PENYACOUNT,
  TID_GUILD_NOTICE_ERROR,
} from '../../gen/defs/text';
import { type GuildBrief, GuildBriefSchema } from '../../gen/proto/flyff/v1/social_types_pb';
import type { SpawnPlayer, SpawnSelf } from '../../gen/proto/flyff/v1/world_pb';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import {
  type BankLogKind,
  type BankLogRequest,
  type BankLogView,
  EMPTY_BANK_LOG,
  type GuildActions,
  type GuildBankRequest,
  type GuildBankView,
  type GuildMenuEntry,
  type GuildPorts,
  type GuildView,
  type MeritRequest,
  MGI,
  PF,
  type PowerView,
} from '../../ui/windows/guild/ports';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { nameRulesBytes } from '../common/lobbyRules';
import { playerDataOf } from '../common/playerData';
import { formatTemplate, sysChat } from '../common/text';
import { derivedView, type ViewSource } from '../common/view';
import { bankBusy } from '../bank/busy';
import { localCommandsOf } from '../chat/commands';
import { inventoryOf, itemFromWire } from '../inventory';
import { ItemContainer } from '../inventory/container';
import type { InvItem } from '../inventory/items';
import { messengerOf } from '../messenger/api';
import { menuOf } from '../npc/menuModule';
import { type GuildEnvSeams, type GuildFeature, setGuildOf } from './api';
import type { LogEntry, LogEnv } from './bankLog';
import { registerGuildProbe } from './probe';
import { canContribute, gemPoints, type GemStack, meritResultTid } from './contribution';
import { checkGuildName, checkNickname, checkSalary, noticeFits, warTargetOk } from './names';
import { PowerDraft } from './power';
import {
  type GuildEffect,
  type GuildEnv,
  type LineArg,
  onAcptGuildWar,
  onAddGuildMember,
  onAllGuilds,
  onBankClosed,
  onChgMaster,
  onCreateGuild,
  onDeclGuildWar,
  onDestroyGuild,
  onGetItemAll,
  onGetItemMine,
  onGetPenyaAll,
  onGetPenyaMine,
  onGuildAuthority,
  onGuildBankWnd,
  onGuildClass,
  onGuildContribution,
  onGuildError,
  onGuildFull,
  onGuildInvite,
  onGuildLogo,
  onGuildLogView,
  onGuildMemberLevel,
  onGuildNickname,
  onGuildNotice,
  onGuildPenya,
  onGuildPresence,
  onGuildRealPenya,
  onGuildSetName,
  onLogRequested,
  onPutItemGuildBank,
  onQueryTruce,
  onRemoveGuildBankItem,
  onRemoveGuildMember,
  onSetGuildSelf,
  onSetWarSelf,
  onSurrender,
  onWar,
  onWarDead,
  onWarEnd,
  type Reduced,
} from './reducers';
import { guildSenders } from './senders';
import {
  defineGuildStore,
  type GuildState,
  GUD,
  isMasterOf,
  ownBrief,
  rankOf as rankIn,
} from './store';
import {
  BankViewBuilder,
  buildGuildView,
  buildPowerView,
  memberRows,
  type ViewFacts,
} from './view';

export const guildModule: FeatureModule = {
  id: 'guild',
  milestone: 6,
  register(fc) {
    registerGuild(fc);
  },
};

/** `INT_MAX`: the quantity box clamps the penya to it, `AddGold` saturates there. */
const INT_MAX = 0x7fff_ffff;
const NO_SLOT = 0xffff_ffff;

function registerGuild(fc: FeatureContext): GuildFeature {
  const ctx = fc.app;
  const { selfStore, sessionStore } = ctx.stores;
  const store = defineGuildStore(ctx.stores.registry);
  const playerData = playerDataOf(ctx);
  const senders = guildSenders((msg) => ctx.net.sender.send(msg));
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const seams: GuildEnvSeams = { siegeBusy: () => null, siegeApplied: () => false };

  const resolveArgs = (args: readonly LineArg[]): (string | number)[] =>
    args.map((a) => (typeof a === 'object' ? text(a.tid) : a));
  const say = (tid: number, args: readonly LineArg[] = []): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, resolveArgs(args));
  };
  const box = (tid: number): void => {
    fullUi(ctx)?.msgbox.open(text(tid));
  };

  const selfId = (): number => selfStore.getState().idPlayer;
  const isGm = (): boolean =>
    Math.max(sessionStore.getState().authority, selfStore.getState().authority) >= AUTH_GAMEMASTER;

  // ---- the facts the reducers and the views read -------------------------------------------------------------------------------------------

  const env = (): GuildEnv => ({
    selfId: selfId(),
    nameOf: (id) => playerData.get(id)?.name ?? '',
    packMax: (itemId) => ctx.data?.item(itemId)?.pack_max ?? 1,
  });
  const levels = (): readonly (readonly number[])[] =>
    ctx.data?.bundle('tables')?.guild.levels ?? [];
  const maxLevel = (): number => Math.max(levels().length, 1);
  const logEnv = (): LogEnv => ({
    text,
    item: (id) => {
      const p = ctx.data?.item(id);
      return p ? { name: p.name, kind3: p.item_kind3, referStat1: p.refer_stat1 } : null;
    },
    guildName: () => {
      const b = ownBrief(store.getState());
      return b ? b.name : null;
    },
  });
  const presenceOf = (id: number): number | undefined =>
    messengerOf(ctx)?.store.getState().presence.get(id);
  const facts = (): ViewFacts => ({
    state: store.getState(),
    selfId: selfId(),
    member: (id) => {
      const e = playerData.get(id);
      return e ? { name: e.name, level: e.level, job: e.job, online: e.online } : undefined;
    },
    presence: presenceOf,
    jobType: (job) => ctx.data?.job(job)?.type ?? 0,
    levels: levels(),
    text,
  });

  // ---- the power draft and the views --------------------------------------------------------------------------------------------------------

  const draft = new PowerDraft();
  const powerListeners = new Set<() => void>();
  const powerSource = {
    subscribe: (fn: () => void) => {
      powerListeners.add(fn);
      return () => {
        powerListeners.delete(fn);
      };
    },
  };
  const dataSource = {
    subscribe: (fn: () => void) =>
      ctx.data?.onBundle(() => {
        fn();
      }) ?? (() => undefined),
  };
  const lazyStore = (get: () => { subscribe(fn: () => void): () => void } | undefined) => ({
    subscribe: (fn: () => void) => get()?.subscribe(fn) ?? (() => undefined),
  });
  const messengerSource = lazyStore(() => messengerOf(ctx)?.store);
  let version = 0;
  const view: ViewSource<GuildView> = derivedView(
    () => {
      version += 1;
      return buildGuildView(facts(), version);
    },
    [store, playerData.store, messengerSource, selfStore, dataSource],
    (a, b) => a === b,
  );
  const powerView: ViewSource<PowerView> = derivedView(
    () => buildPowerView(draft, store.getState(), selfId()),
    [store, selfStore, powerSource],
    (a, b) => a === b,
  );
  const bankBuilder = new BankViewBuilder(() => ctx.data);
  let bankVersion = 0;
  const bankView: ViewSource<GuildBankView> = derivedView(
    () => {
      bankVersion += 1;
      return bankBuilder.build(
        store.getState(),
        selfId(),
        ctx.features.features.new_font_color,
        ctx.data?.version ?? -1,
        bankVersion,
      );
    },
    [store, selfStore, dataSource],
    (a, b) => a === b,
  );
  let logVersion = 0;
  let lastLog: GuildState['log'] | null = null;
  let lastLogView: BankLogView = EMPTY_BANK_LOG;
  const logView: ViewSource<BankLogView> = derivedView(
    () => {
      const l = store.getState().log;
      if (l === lastLog) return lastLogView;
      lastLog = l;
      logVersion += 1;
      lastLogView = { version: logVersion, lines: l };
      return lastLogView;
    },
    [store],
    (a, b) => a === b,
  );
  const notifyPower = (): void => {
    for (const fn of [...powerListeners]) fn();
  };

  // ---- selectors ----------------------------------------------------------------------------------------------------------------------------------

  const ownId = (): number => store.getState().own?.id ?? 0;
  const hasPower = (bit: number): boolean => {
    const s = store.getState();
    const own = s.own;
    if (!own) return false;
    const rank = rankIn(s, selfId());
    if (rank < 0) return false;
    // the master always may (PL-092); the others by their word
    return rank === GUD.MASTER || ((own.powers[rank] ?? 0) & bit) !== 0;
  };
  const isOnline = (id: number): boolean =>
    (presenceOf(id) ?? 0) > 0 || playerData.get(id)?.online === true;
  const myRecord = () => store.getState().own?.members.get(selfId()) ?? null;

  // ---- the movers in view ---------------------------------------------------------------------------------------------------------------------

  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  /** The brief of a guild id for a mover; a guild the client does not know yet is a placeholder that `AllGuilds` replaces. */
  const briefFor = (idGuild: number): GuildBrief | null => {
    if (idGuild === 0) return null;
    return store.getState().briefs.get(idGuild) ?? create(GuildBriefSchema, { idGuild });
  };
  const players = (fn: (rec: EntityRecord) => void): void => {
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (rec.visual) fn(rec);
    });
  };
  /** `SpawnSelf` / `SpawnPlayer`: the guild and the war of the mover. */
  const onSpawn = (objId: number): void => {
    const rec = ctx.entities.get(objId);
    if (!valid(rec) || !rec.visual) return;
    const p = (rec.spawn as SpawnPlayer | SpawnSelf).player;
    if (!p) return;
    rec.visual.guild = briefFor(p.idGuild);
    rec.visual.idWar = p.idWar;
  };
  /** `AllGuilds` and the notifications: the movers share the new brief objects; a dissolved guild leaves its members. */
  const syncVisuals = (gone?: number): void => {
    const briefs = store.getState().briefs;
    players((rec) => {
      const v = rec.visual;
      const cur = v?.guild;
      if (!v || !cur) return;
      if (gone !== undefined && cur.idGuild === gone) {
        v.guild = null;
        return;
      }
      const b = briefs.get(cur.idGuild);
      if (b && b !== cur) v.guild = b;
    });
  };

  // ---- the quantity boxes ---------------------------------------------------------------------------------------------------------------

  const closeQuantity = (): void => {
    fullUi(ctx)?.wm.get(APP_TRADE_GOLD, 'quantity')?.destroy();
  };
  const askCount = (max: number, then: (n: number) => void): void => {
    fullUi(ctx)?.msgbox.quantity(max, then, undefined, undefined, {
      prompt: text(TID_GAME_MOVECOUNT),
      label: text(TID_GAME_NUMCOUNT),
    });
  };
  const askPenya = (max: number, then: (n: number) => void): void => {
    fullUi(ctx)?.msgbox.quantity(Math.min(max, INT_MAX), then, undefined, undefined, {
      prompt: text(TID_GAME_MOVEPENYA),
      label: text(TID_GAME_PENYACOUNT),
    });
  };

  // ---- the dialogs -------------------------------------------------------------------------------------------------------------------------------

  const names = (): Uint8Array | null => {
    const n = ctx.data?.bundle('names');
    return n ? nameRulesBytes(n) : null;
  };

  const openNaming = (initial: string, send: (name: string) => void): void => {
    fullUi(ctx)?.guild.openName({
      initial,
      submit: (raw) => {
        const name = raw.trim();
        const tid = checkGuildName(name, fc.lobbyRules, names());
        if (tid !== 0) {
          box(tid);
          return false;
        }
        send(name);
        return true;
      },
    });
  };

  const openDismiss = (): void => {
    fullUi(ctx)?.guild.openDismiss({
      yes: () => {
        // the application for the siege forbids the disbanding (re-checked at the answer)
        if (seams.siegeApplied()) box(TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD);
        else senders.destroy();
      },
    });
  };

  const openNickname = (idPlayer: number): void => {
    fullUi(ctx)?.guild.openNickname({
      initial: '',
      submit: (raw) => {
        const alias = raw.trim();
        const tid = alias === '' ? 0 : checkNickname(alias, fc.lobbyRules, names());
        if (alias === '') {
          // an empty alias: `TID_DIAG_0011`
          box(checkNickname('', fc.lobbyRules, names()));
          return false;
        }
        if (tid !== 0) {
          box(tid);
          return false;
        }
        // the alias the member already has: nothing to send, the window stays open as in the original
        if (store.getState().own?.members.get(idPlayer)?.alias === alias) return false;
        senders.nickname(idPlayer, alias);
        return true;
      },
    });
  };

  /** The gem stacks of the bag (`IK3_GEM`) with their item levels. */
  const gemItems = () => {
    const bag = inventoryOf(ctx)?.view.get().bag ?? [];
    return bag
      .filter((it): it is NonNullable<typeof it> => it !== null)
      .filter((it) => ctx.data?.item(it.itemId)?.item_kind3 === IK3_GEM && !it.expired);
  };
  const gemStacks = (): GemStack[] =>
    gemItems().map((it) => ({ itemLv: ctx.data?.item(it.itemId)?.item_lv ?? 0, count: it.count }));

  const contributionGuild = () => {
    const b = ownBrief(store.getState());
    const own = store.getState().own;
    return b && own
      ? { level: b.level, pxp: own.pxp, penya: b.guildGold, maxLevel: maxLevel() }
      : null;
  };

  /** `CWndGuildMerit::OnChildNotify( WIDC_BUTTON1 )`: the pre-checks, then `GuildContributionReq`. */
  const contribute = (penya: number, gems: boolean): void => {
    const g = contributionGuild();
    if (!g) return;
    const rec = myRecord();
    const member = rec ? { givePxp: rec.givePxp, giveGold: rec.giveGold } : null;
    if (penya > 0) {
      const r = canContribute(g, member, 0, penya);
      if (r === 'ok') senders.contribution(penya, false);
      else {
        const tid = meritResultTid(r);
        if (tid !== 0) say(tid);
      }
    }
    if (gems) {
      const r = canContribute(g, member, gemPoints(gemStacks()), 0);
      if (r === 'ok') senders.contribution(0, true);
      else {
        const tid = meritResultTid(r);
        if (tid !== 0) say(tid);
      }
    }
  };

  const meritRequest = (): MeritRequest => ({
    gems: () => gemItems(),
    reserve: (objId, extra) => {
      inventoryOf(ctx)?.setExtra(objId, extra);
    },
    submit: contribute,
  });

  // ---- the guild bank ---------------------------------------------------------------------------------------------------------------------------

  const bankActions = {
    putFromInventory: (sc: Shortcut): boolean => {
      closeQuantity();
      // the penya is never deposited: contributions are the way
      if (sc.data === 0) return false;
      // a worn item comes with `index = 0xffffffff` and no selection (`SelectCount != 1`)
      if (sc.index === NO_SLOT) {
        say(TID_GAME_EQUIPPUT);
        return false;
      }
      const item = inventoryOf(ctx)?.item(sc.id) ?? null;
      if (!item) return false;
      if (item.charged) {
        say(TID_GAME_NOTTRADE);
        return false;
      }
      const objId = item.objId;
      if (item.num > 1) {
        askCount(item.num, (n) => {
          const cur = inventoryOf(ctx)?.item(objId);
          senders.putItem(objId, Math.min(n, cur?.num ?? n));
        });
      } else {
        senders.putItem(objId, 1);
      }
      return true;
    },
    support: (): void => {
      const b = ownBrief(store.getState());
      if (!b) return;
      if (b.name === '') {
        box(TID_GAME_GUILDNOTHINGNAME);
        return;
      }
      fullUi(ctx)?.guild.openMerit(meritRequest());
    },
    cloak: (): void => {
      const b = ownBrief(store.getState());
      if (!b) return;
      if (b.level < 4) {
        box(TID_GAME_GUILDNOTLEVEL);
        return;
      }
      senders.createCloak();
    },
    log: (): void => {
      const r = rankIn(store.getState(), selfId());
      if (r !== GUD.MASTER && r !== GUD.KINGPIN) {
        say(TID_GAME_CANNT_USE_GLOG);
        return;
      }
      const ui = fullUi(ctx);
      if (!ui) return;
      if (ui.wm.isOpen(APP_GUILD_BANK_LOG)) ui.guild.closeBankLog();
      else ui.guild.openBankLog(logRequest());
    },
    closed: (): void => {
      closeQuantity();
      const ui = fullUi(ctx);
      ui?.guild.closeBankLog();
      ui?.wm.get(APP_GUILD_BANK_LOG)?.destroy();
      apply(onBankClosed(store.getState()));
      senders.closeBank();
    },
  };
  const bankRequest = (): GuildBankRequest => ({ view: bankView, actions: bankActions });
  function logRequest(): BankLogRequest {
    return {
      view: logView,
      request: (kind: BankLogKind) => {
        apply(onLogRequested(store.getState(), kind));
        senders.logView(kind);
      },
    };
  }

  /** A bank item dragged onto the inventory (the ITEM power) or the coin (the PENYA power): the quantity box, then `GetItemGuildBankReq`. */
  const dropIn = (sc: Shortcut): boolean => {
    if (sc.kind !== SC.ITEM) return false;
    closeQuantity();
    const s = store.getState();
    if (sc.data !== 0) {
      if (!hasPower(PF.ITEM)) return false;
      const item = s.bank.container?.getAtId(sc.id) ?? null;
      if (!item) return false;
      const objId = item.objId;
      if (item.num > 1) {
        askCount(item.num, (n) => {
          const cur = store.getState().bank.container?.getAtId(objId);
          senders.getItem(objId, Math.min(n, cur?.num ?? n), 1);
        });
      } else {
        senders.getItem(objId, 1, 1);
      }
      return true;
    }
    if (!hasPower(PF.PENYA)) return false;
    const gold = ownBrief(s)?.guildGold ?? 0;
    askPenya(gold, (n) => {
      const amount = Math.min(n, ownBrief(store.getState())?.guildGold ?? 0);
      if (amount > 0) senders.getItem(0, amount, 0);
    });
    return true;
  };

  /** `MMI_GUILDBANKING` (`CWndWorld::OnCommand`): the busy checks in the order of the original, then the request. */
  const requestBank = (): boolean => {
    const ui = fullUi(ctx);
    if (!ui) return false;
    const busy = bankBusy((app) => ui.wm.isOpen(app));
    if (busy === 'silent') return false;
    if (busy !== null) {
      say(busy);
      return false;
    }
    senders.openBank();
    return true;
  };

  // ---- the actions of the guild window ----------------------------------------------------------------------------------------------------

  const selfRank = (): number => rankIn(store.getState(), selfId());
  const amMaster = (): boolean => selfRank() === GUD.MASTER;
  const warNoMember = (): boolean => {
    if (store.getState().selfWar === 0) return false;
    box(TID_GAME_GUILDWARERRORMEMBER);
    return true;
  };

  const actions: GuildActions = {
    setName() {
      const b = ownBrief(store.getState());
      if (!b || !amMaster()) return;
      if (b.name !== '') {
        box(TID_GAME_GUILDNOTCHGNAME);
        return;
      }
      openNaming('', (name) => {
        senders.setName(name);
      });
    },
    setNotice() {
      const own = store.getState().own;
      if (!own || !amMaster()) return;
      fullUi(ctx)?.guild.openNotice({
        initial: own.notice,
        send: (t) => {
          if (!noticeFits(t)) {
            box(TID_GUILD_NOTICE_ERROR);
            return false;
          }
          senders.notice(t);
          return true;
        },
      });
    },
    setLogo() {
      const s = store.getState();
      const b = ownBrief(s);
      if (!b || !amMaster()) return;
      if (s.selfWar !== 0) {
        box(TID_GAME_GUILDWARERRORLOGO);
        return;
      }
      if (b.logo !== 0) {
        box(TID_GAME_GUILDSTILLLOGO);
        return;
      }
      if (b.level < 2) {
        box(TID_GAME_GUILDNOTLEVEL);
        return;
      }
      fullUi(ctx)?.guild.openLogo({
        current: b.logo,
        gm: isGm(),
        pick: (n) => {
          senders.logo(n);
        },
      });
    },
    disband() {
      const ui = fullUi(ctx);
      const s = store.getState();
      if (!ui || !s.own || !amMaster()) return;
      // the bank window open: nothing happens
      if (ui.wm.isOpen(APP_GUILD_BANK)) return;
      if (s.selfWar !== 0) {
        box(TID_GAME_GUILDWARERRORDISBAND);
        return;
      }
      if (seams.siegeApplied()) {
        box(TID_GAME_GUILDCOMBAT_NOT_DISSOLVE_GUILD);
        return;
      }
      openDismiss();
    },
    memberMenu(id) {
      const s = store.getState();
      const own = s.own;
      const target = own?.members.get(id);
      if (!own || !target) return [];
      const entries: GuildMenuEntry[] = [];
      const add = (e: number, tid: number): void => {
        entries.push({ id: e, text: text(tid) });
      };
      const me = selfId();
      if (hasPower(PF.MEMBERLEVEL)) {
        if (target.rank > 0) add(MGI.APPELL_UP, TID_GAME_GUILD_APPELLATIONUP);
        if (target.rank < 4) add(MGI.APPELL_DOWN, TID_GAME_GUILD_APPELLATIONDOWN);
      }
      if (hasPower(PF.LEVEL) && target.rank !== GUD.MASTER) {
        add(MGI.CLASS_UP, TID_GAME_GUILD_CLASSUP);
        add(MGI.CLASS_DOWN, TID_GAME_GUILD_CLASSDOWN);
      }
      if (amMaster()) add(MGI.NICKNAME, TID_GAME_GUILD_NICKNAME);
      // leaving is everybody's right; removing somebody else is the master's
      if (id === me || amMaster()) add(MGI.GUILD_LEAVE, TID_GAME_GUILD_LEAVE);
      if (amMaster() && id !== me) add(MGI.CHG_MASTER, TID_GAME_CHG_MASTER);
      return entries;
    },
    pickMember(id, entry) {
      const s = store.getState();
      const target = s.own?.members.get(id);
      if (!s.own || !target) return;
      // `if( g_pPlayer->m_idWar )`: every entry only says that members cannot be changed during a war
      if (warNoMember()) return;
      switch (entry) {
        case MGI.APPELL_UP:
          if (target.rank - 1 >= 0) senders.memberLevel(id, target.rank - 1);
          break;
        case MGI.APPELL_DOWN:
          if (target.rank + 1 <= 4) senders.memberLevel(id, target.rank + 1);
          break;
        case MGI.CLASS_UP:
        case MGI.CLASS_DOWN:
          // `SendGuildClass` sends nothing for the own id
          if (id !== selfId()) senders.guildClass(id, entry === MGI.CLASS_UP);
          break;
        case MGI.NICKNAME: {
          if (!amMaster()) {
            box(TID_GAME_GUILDONLYMASTER);
            break;
          }
          const level = ownBrief(s)?.level ?? 0;
          if (level < 10) box(TID_GAME_GUILDNOTLEVEL);
          else openNickname(id);
          break;
        }
        case MGI.GUILD_LEAVE:
          if (seams.siegeApplied()) box(TID_GAME_GUILDCOMBAT_NOT_LEAVE_GUILD);
          else senders.removeMember(id);
          break;
        case MGI.CHG_MASTER:
          if (id === selfId()) break;
          if (isOnline(id)) senders.chgMaster(id);
          else box(TID_GAME_NOTLOGIN);
          break;
        default:
          break;
      }
    },
    setPower(rank, bit, on) {
      if (!amMaster()) return;
      if (draft.set(rank, bit, on)) notifyPower();
    },
    sendPower() {
      if (!amMaster()) return;
      const words = draft.toSend();
      if (words) senders.authority(words);
    },
    openPay(rank) {
      if (!amMaster()) return;
      fullUi(ctx)?.guild.openPayConfirm({
        rank,
        submit: (t) => {
          const r = checkSalary(t);
          if (!r.ok) {
            box(TID_GAME_GUILDONLYNUMBER);
            return false;
          }
          senders.penya(rank, r.penya);
          return true;
        },
      });
    },
    declareWar() {
      if (!amMaster()) return;
      fullUi(ctx)?.guild.openWarDecl({
        submit: (name) => {
          if (!warTargetOk(name)) return false;
          senders.declWar(name.trim());
          return true;
        },
      });
    },
    truce() {
      if (!amMaster()) return;
      fullUi(ctx)?.guild.openWarPeace({
        yes: () => {
          senders.queryTruce();
        },
      });
    },
    surrender() {
      if (!store.getState().own) return;
      fullUi(ctx)?.guild.openWarGiveUp({
        yes: () => {
          senders.surrender();
        },
      });
    },
  };

  // ---- carrying out the effects of a reducer --------------------------------------------------------------------------------------------

  const run = (effects: readonly GuildEffect[]): void => {
    const ui = fullUi(ctx);
    for (const e of effects) {
      switch (e.kind) {
        case 'line':
          say(e.tid, e.args);
          break;
        case 'invite': {
          const name = store.getState().briefs.get(e.guildId)?.name ?? '';
          ui?.guild.openInvite({
            guildName: name,
            yes: () => {
              senders.addMember(e.inviter);
            },
          });
          break;
        }
        case 'closeWindows':
          closeGuildWindows();
          break;
        case 'openBank':
          // `CreateApplet( APP_INVENTORY )`, then the bank beside it
          ui?.char.openInventory();
          ui?.guild.openBank(bankRequest());
          break;
        case 'need':
          playerData.onQuery(e.id, e.name, undefined);
          playerData.need(e.id);
          break;
        case 'needAll':
          playerData.needAll(e.ids);
          break;
        case 'selfGuild':
          if (selfStore.getState().guildId !== e.id) selfStore.setState({ guildId: e.id });
          break;
        case 'visuals':
          syncVisuals(e.gone);
          break;
        case 'wallet': {
          const gold = selfStore.getState().gold;
          selfStore.setState({ gold: Math.min(INT_MAX, Math.max(0, gold + e.gold)) });
          break;
        }
        case 'inventoryAdd': {
          const inv = inventoryOf(ctx);
          inv?.addItem(e.item);
          break;
        }
        case 'warRequest': {
          const guild = store.getState().briefs.get(e.idDecl)?.name ?? '';
          ui?.guild.openWarRequest({
            text: formatTemplate(text(TID_GAME_GUILDWARREQUEST), [guild, e.masterName]),
            yes: () => {
              senders.acptWar(e.idDecl);
            },
          });
          break;
        }
        case 'truceConfirm':
          ui?.guild.openWarPeaceConfirm({
            yes: () => {
              senders.acptTruce();
            },
          });
          break;
        case 'powerRebase': {
          const words = store.getState().own?.powers ?? [];
          draft.rebase(words);
          notifyPower();
          break;
        }
        case 'refresh':
          // the views rebuild on any store write; a presence message changes no store of ours
          store.setState({ ...store.getState() });
          break;
      }
    }
  };

  /** Every guild window except the guild window (the player lost the guild). */
  const closeGuildWindows = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    closeQuantity();
    const keep = ui.wm.get(APP_GUILD);
    ui.guild.closeAll();
    // the guild window itself stays (blank): `closeAll` took it too, so it opens again when it was open
    if (keep && !ui.wm.isOpen(APP_GUILD)) ui.wm.open(APP_GUILD);
  };

  const apply = (r: Reduced): void => {
    if (r.state !== store.getState()) store.setState(r.state, true);
    run(r.effects);
  };

  // ---- the messages --------------------------------------------------------------------------------------------------------------------------

  const isSelfObj = (objId: number): boolean => objId === selfStore.getState().objId;
  const containerOf = (
    wire:
      | {
          max: number;
          index: number[];
          objIndex: number[];
          items: Parameters<typeof itemFromWire>[0][];
        }
      | undefined,
  ): ItemContainer<InvItem> => {
    if (!wire || wire.max <= 0) return new ItemContainer<InvItem>(42, 0);
    return ItemContainer.fromWire(
      {
        max: wire.max,
        index: wire.index,
        objIndex: wire.objIndex,
        items: wire.items.map(itemFromWire),
      },
      0,
    );
  };
  const nowUnix = (): number => Math.floor(Date.now() / 1000);

  fc.net.on('spawnSelf', (_m, objId) => {
    onSpawn(objId);
  });
  fc.net.on('spawnPlayer', (_m, objId) => {
    onSpawn(objId);
  });

  fc.net.on('guildInviteNtf', (m) => {
    apply(onGuildInvite(store.getState(), { idGuild: m.idGuild, idInviter: m.idInviter }));
  });
  fc.net.on('createGuild', (m) => {
    apply(
      onCreateGuild(
        store.getState(),
        {
          idPlayer: m.idPlayer,
          idGuild: m.idGuild,
          masterName: m.masterName,
          guildName: m.guildName,
        },
        env(),
      ),
    );
  });
  fc.net.on('destroyGuildNtf', (m) => {
    apply(onDestroyGuild(store.getState(), { masterName: m.masterName, idGuild: m.idGuild }));
  });
  fc.net.on('setGuild', (m, objId) => {
    const rec = ctx.entities.get(objId);
    if (valid(rec) && rec.visual) rec.visual.guild = briefFor(m.idGuild);
    if (isSelfObj(objId)) apply(onSetGuildSelf(store.getState(), m.idGuild));
  });
  fc.net.on('guild', (m) => {
    if (m.guild) apply(onGuildFull(store.getState(), m.guild));
  });
  fc.net.on('allGuilds', (m) => {
    apply(onAllGuilds(store.getState(), { nextId: m.nextId, guilds: m.guilds }));
  });
  fc.net.on('addGuildMemberNtf', (m) => {
    apply(
      onAddGuildMember(store.getState(), {
        idPlayer: m.info?.idPlayer ?? 0,
        idGuild: m.idGuild,
        name: m.name,
      }),
    );
  });
  fc.net.on('removeGuildMemberNtf', (m) => {
    apply(
      onRemoveGuildMember(store.getState(), { idPlayer: m.idPlayer, idGuild: m.idGuild }, env()),
    );
  });
  fc.net.on('guildMemberLevelNtf', (m) => {
    apply(onGuildMemberLevel(store.getState(), { idPlayer: m.idPlayer, rank: m.rank }, env()));
  });
  fc.net.on('guildClassNtf', (m) => {
    apply(onGuildClass(store.getState(), { idPlayer: m.idPlayer, cls: m.class }, env()));
  });
  fc.net.on('guildNicknameNtf', (m) => {
    apply(onGuildNickname(store.getState(), { idPlayer: m.idPlayer, alias: m.alias }));
  });
  fc.net.on('chgMasterNtf', (m) => {
    apply(onChgMaster(store.getState(), { oldMaster: m.oldMaster, newMaster: m.newMaster }, env()));
  });
  fc.net.on('guildAuthorityNtf', (m) => {
    apply(onGuildAuthority(store.getState(), { power: m.power }));
  });
  fc.net.on('guildPenyaNtf', (m) => {
    apply(onGuildPenya(store.getState(), { rank: m.rank, penya: m.penya }));
  });
  fc.net.on('guildSetNameNtf', (m) => {
    apply(onGuildSetName(store.getState(), { idGuild: m.idGuild, name: m.name }));
  });
  fc.net.on('guildLogoNtf', (m) => {
    apply(onGuildLogo(store.getState(), { idGuild: m.idGuild, logo: m.logo }));
  });
  fc.net.on('guildContributionNtf', (m) => {
    apply(
      onGuildContribution(
        store.getState(),
        {
          idGuild: m.idGuild,
          idPlayer: m.idPlayer,
          pxpDelta: m.pxpDelta,
          penyaDelta: m.penyaDelta,
          guildPxp: m.guildPxp,
          guildPenya: m.guildPenya,
          guildLevel: m.guildLevel,
        },
        env(),
      ),
    );
  });
  fc.net.on('guildNoticeNtf', (m) => {
    apply(onGuildNotice(store.getState(), { idGuild: m.idGuild, notice: m.notice }));
  });
  fc.net.on('guildRealPenya', (m) => {
    apply(onGuildRealPenya(store.getState(), { gold: m.gold, rank: m.rank }));
  });
  fc.net.on('guildGameJoin', () => {
    apply(onGuildPresence(store.getState()));
  });
  fc.net.on('guildGameLogin', () => {
    apply(onGuildPresence(store.getState()));
  });
  fc.net.on('guildError', (m) => {
    apply(onGuildError(store.getState(), m.code));
  });

  fc.net.on('guildBankWndNtf', (m) => {
    apply(
      onGuildBankWnd(store.getState(), {
        mode: m.mode,
        gold: m.gold,
        container: containerOf(m.items),
      }),
    );
  });
  fc.net.on('putItemGuildBankNtf', (m) => {
    if (m.item) apply(onPutItemGuildBank(store.getState(), itemFromWire(m.item), env()));
  });
  fc.net.on('getItemGuildBankNtf', (m) => {
    const sub = m.sub;
    switch (sub.case) {
      case 'penyaMine':
        apply(
          onGetPenyaMine(store.getState(), {
            gold: sub.value.gold,
            idPlayer: sub.value.idPlayer,
            cloak: sub.value.cloak,
          }),
        );
        break;
      case 'itemMine':
        if (sub.value.item) apply(onGetItemMine(store.getState(), itemFromWire(sub.value.item)));
        break;
      case 'penyaAll':
        apply(
          onGetPenyaAll(store.getState(), {
            gold: sub.value.gold,
            idPlayer: sub.value.idPlayer,
            cloak: sub.value.cloak,
          }),
        );
        break;
      case 'itemAll':
        if (sub.value.item) apply(onGetItemAll(store.getState(), itemFromWire(sub.value.item)));
        break;
      case undefined:
        break;
    }
  });
  fc.net.on('removeGuildBankItem', (m) => {
    apply(
      onRemoveGuildBankItem(store.getState(), {
        idGuild: m.idGuild,
        itemObjId: m.itemObjId,
        num: m.num,
      }),
    );
  });
  fc.net.on('guildLogViewNtf', (m) => {
    const entries: LogEntry[] = m.entries.map((e) => ({
      player: e.player,
      loggedAt: Number(e.loggedAt),
      itemId: e.itemId,
      abilityOption: e.abilityOption,
      count: e.count,
    }));
    apply(onGuildLogView(store.getState(), { kind: m.kind, entries }, logEnv()));
  });

  fc.net.on('declGuildWarNtf', (m) => {
    apply(
      onDeclGuildWar(store.getState(), { idDeclGuild: m.idDeclGuild, masterName: m.masterName }),
    );
  });
  fc.net.on('acptGuildWarNtf', (m) => {
    apply(
      onAcptGuildWar(
        store.getState(),
        { idWar: m.idWar, idDecl: m.idDecl, idAcpt: m.idAcpt },
        nowUnix(),
      ),
    );
  });
  fc.net.on('surrenderNtf', (m) => {
    apply(
      onSurrender(store.getState(), {
        idWar: m.idWar,
        idPlayer: m.idPlayer,
        name: m.name,
        decl: m.decl,
      }),
    );
  });
  fc.net.on('queryTruceNtf', () => {
    apply(onQueryTruce(store.getState()));
  });
  fc.net.on('warDead', (m) => {
    apply(onWarDead(store.getState(), { idWar: m.idWar, playerName: m.playerName, decl: m.decl }));
  });
  fc.net.on('warEnd', (m) => {
    apply(onWarEnd(store.getState(), { idWar: m.idWar, result: m.result }));
  });
  fc.net.on('war', (m) => {
    if (m.war) apply(onWar(store.getState(), m.war));
  });
  fc.net.on('setWar', (m, objId) => {
    const rec = ctx.entities.get(objId);
    if (valid(rec) && rec.visual) rec.visual.idWar = m.idWar;
    if (isSelfObj(objId)) apply(onSetWarSelf(store.getState(), m.idWar));
  });

  // ---- wiring into the other features --------------------------------------------------------------------------------------------------------

  const menu = menuOf(ctx);
  // the player menu offers the invitation to a member with the INVITATION power (the master always)
  menu?.addPlayerEnv(() => ({ canInviteToGuild: hasPower(PF.INVITATION) }));
  menu?.registerAction(MMI_INVITE_COMPANY, ({ targetId }) => {
    // `CWndWorld::InviteCompany`: a guild member only; the siege windows may forbid it
    if (!store.getState().own) return;
    const busy = seams.siegeBusy();
    if (busy !== null) {
      box(busy);
      return;
    }
    senders.invite(targetId);
  });
  menu?.registerAction(MMI_GUILDBANKING, ({ npcKey }) => {
    if (npcKey !== null) requestBank();
  });

  // the rename scroll (router R17): `APP_GUILDNAME` in rename mode
  inventoryOf(ctx)?.registerDispatchWindows({
    guildName: (item) => {
      const b = ownBrief(store.getState());
      if (!b) return;
      openNaming(b.name, (name) => {
        senders.querySetName(item.objId, name);
      });
    },
  });
  // the withdrawals: a bank item or the coin dragged onto the bag
  let unregisterDrop: (() => void) | null = null;
  const hookInventory = (): void => {
    const inv = inventoryOf(ctx);
    if (!inv || unregisterDrop) return;
    unregisterDrop = inv.registerDropIn(APP_GUILD_BANK, dropIn);
  };
  ctx.tickSystems.register('guild.inventory', hookInventory);
  hookInventory();

  // `/destroyguild` (row 68): the disband question of the master
  localCommandsOf(ctx).set(68, () => {
    if (amMaster()) openDismiss();
  });

  // the bank closes with every transactional close (a duel, the own death, a world change, logout)
  fc.transactional.register({
    id: 'guildBank',
    reasons: 'all',
    close: () => {
      fullUi(ctx)?.guild.closeBank();
    },
  });

  // ---- the windows -------------------------------------------------------------------------------------------------------------------------------

  const ports: GuildPorts = { view, power: powerView, actions, now: nowUnix };
  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    ui.guild.setPorts({ guild: ports });
  };
  ctx.tickSystems.register('guild.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s === 'InGame' && screen.s !== 'InGame' && screen.s !== 'Reconnecting') {
      fullUi(ctx)?.guild.closeAll();
      draft.rebase([]);
      notifyPower();
    }
  });

  // the guild page of the messenger reads the roster of this store
  const roster = {
    ids: (): readonly number[] => [...(store.getState().own?.members.keys() ?? [])],
    subscribe: (fn: () => void) => store.subscribe(fn),
  };
  messengerOf(ctx)?.setGuildRoster(roster);

  const feature: GuildFeature = {
    store,
    senders,
    view,
    actions,
    ownGuildId: ownId,
    ownGuildName: () => ownBrief(store.getState())?.name ?? '',
    isMaster: (id) => isMasterOf(store.getState(), id),
    rankOf: (id) => rankIn(store.getState(), id),
    members: () => memberRows(facts()),
    memberIds: () => roster.ids(),
    nameOf: (id) => store.getState().briefs.get(id)?.name ?? null,
    briefOf: (id) => store.getState().briefs.get(id) ?? null,
    hasPower,
    selfWar: () => store.getState().selfWar,
    isOnline,
    setEnv: (e) => {
      if (e.siegeBusy) seams.siegeBusy = e.siegeBusy;
      if (e.siegeApplied) seams.siegeApplied = e.siegeApplied;
    },
    openGuild: (tab) => {
      const ui = fullUi(ctx);
      if (!ui) return;
      ui.wm.open(APP_GUILD);
      if (tab !== undefined) ui.guild.guild()?.setCurTab(tab);
    },
  };
  setGuildOf(ctx, feature);

  // ---- the debug probe ----------------------------------------------------------------------------------------------------------------------------

  registerGuildProbe(ctx, feature, { ports, bankView, logView, draft, isOnline, rank: selfRank });
  return feature;
}
