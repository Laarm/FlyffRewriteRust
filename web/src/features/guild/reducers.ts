// The reducers of the guild messages (Plan/45 §23, ref/09b §3.7, `DPClient.cpp:12589-13560`, `DPClientGuildWar.cpp`, `OnPutItemGuildBank` ... `OnGuildBankLogList`; WP-CFEAT-37, -38): pure
// functions `(state, message fields, env) -> { state, effects }`. The module carries the effects out (lines, boxes, windows, the player-data cache, the wallet, the inventory,
// the visuals of the movers); one table test per message. A message the original ignores (an unknown guild, a war it does not know) changes nothing and has no effects.
//
// FIX list (the task): `GuildMemberLevelNtf` prints `TID_GAME_GUILDMEM_APPUP` / `APPDOWN` (PL-124: the original built the text and dropped it; the promotion is a LOWER member
// level number), `GuildClassNtf` prints `CLASUP` / `CLASDOWN`, the contribution success line comes with the own `GuildContributionNtf`, the bank log is kept without the window,
// the war messages tolerate a guild the client does not know (orphan wars).

import type { GuildBrief, GuildFull, GuildWarInfo } from '../../gen/proto/flyff/v1/social_types_pb';
import {
  TID_GAME_CHANGE_GUILDMASTER,
  TID_GAME_COMACCEPT,
  TID_GAME_COMACCEPTWELCOME,
  TID_GAME_COMCREATE,
  TID_GAME_COMDISMESS,
  TID_GAME_COMLEAVE,
  TID_GAME_COMLEAVEBYE,
  TID_GAME_COMOVERLAPNAME,
  TID_GAME_GUILD_PAYTAKE,
  TID_GAME_GUILDCHGPAY,
  TID_GAME_GUILDCHGWARRANT,
  TID_GAME_GUILDLEVELUP,
  TID_GAME_GUILDMEM_APPDOWN,
  TID_GAME_GUILDMEM_APPUP,
  TID_GAME_GUILDMEM_CLASDOWN,
  TID_GAME_GUILDMEM_CLASUP,
  TID_GAME_GUILDMERITSTUFF,
  TID_GAME_GUILDNAMECHG,
  TID_GAME_GUILDNOTICEUPDATE,
  TID_GAME_GUILDONLYNUMBER,
  TID_GAME_GUILDWARDEAD,
  TID_GAME_GUILDWARDOING,
  TID_GAME_GUILDWARDRAW,
  TID_GAME_GUILDWARGIVEUP,
  TID_GAME_GUILDWARPEACE,
  TID_GAME_GUILDWARSTART,
  TID_GAME_GUILDWARSTILL,
  TID_GAME_GUILDWARWIN,
  TID_GAME_GUILDWARWIN_GIVEUP,
  TID_GAME_GUILDWARWIN_KILLNUM,
  TID_GAME_GUILDWARWIN_MASTER,
  TID_GAME_GUILDWARWIN_TIMEUP,
} from '../../gen/defs/text';
import type { BankLogKind } from '../../ui/windows/guild/ports';
import { ItemContainer } from '../inventory/container';
import type { InvItem } from '../inventory/items';
import { formatLog, type LogEntry, type LogEnv } from './bankLog';
import {
  type GuildState,
  GUILD_BANK_SLOTS,
  type MemberRecord,
  type OwnGuild,
  type WarSide,
  type WarState,
} from './store';

/** A text argument: a string, a number, or the text of another id (`{ tid }`, resolved by the module). */
export type LineArg = string | number | { readonly tid: number };

export type GuildEffect =
  /** A client-side system line (`g_WndMng.PutString( ..., color )`). */
  | { readonly kind: 'line'; readonly tid: number; readonly args: readonly LineArg[] }
  /** The invitation window for `guildId`, invited by `inviter`. */
  | { readonly kind: 'invite'; readonly guildId: number; readonly inviter: number }
  /** Every guild window except the guild window itself goes (the player lost the guild). */
  | { readonly kind: 'closeWindows' }
  /** The bank window opens (`GuildBankWndNtf` mode 0). */
  | { readonly kind: 'openBank' }
  /** The player-data cache learns a name (`AddPlayerData( id, name )`) and is asked for the rest. */
  | { readonly kind: 'need'; readonly id: number; readonly name: string }
  | { readonly kind: 'needAll'; readonly ids: readonly number[] }
  /** The `selfStore.guildId` of the own character. */
  | { readonly kind: 'selfGuild'; readonly id: number }
  /** The visuals of the movers (`visual.guild`, `visual.idWar`) are resolved again; `gone` = a guild that was dissolved. */
  | { readonly kind: 'visuals'; readonly gone?: number }
  /** `AddGold( gold, FALSE )`: the wallet of the own character grows. */
  | { readonly kind: 'wallet'; readonly gold: number }
  /** The item taken out of the bank goes into the inventory (a silent add). */
  | { readonly kind: 'inventoryAdd'; readonly item: InvItem }
  /** `DeclGuildWarNtf`: the request window. */
  | { readonly kind: 'warRequest'; readonly idDecl: number; readonly masterName: string }
  /** `QueryTruceNtf`: the truce approval window. */
  | { readonly kind: 'truceConfirm' }
  /** The power draft starts again from the guild's words. */
  | { readonly kind: 'powerRebase' }
  /** A presence message: the views are rebuilt. */
  | { readonly kind: 'refresh' };

export interface Reduced {
  readonly state: GuildState;
  readonly effects: readonly GuildEffect[];
}

/** What the reducers read of the client. */
export interface GuildEnv {
  /** The character id of the own character. */
  readonly selfId: number;
  /** The cached name of a character, `''` when unknown. */
  nameOf(id: number): string;
  /** `dwPackMax` of an item id (the bank `Add`). */
  packMax(itemId: number): number;
}

const same = (s: GuildState): Reduced => ({ state: s, effects: [] });
const line = (tid: number, args: readonly LineArg[] = []): GuildEffect => ({
  kind: 'line',
  tid,
  args,
});

// ---- small immutable helpers ---------------------------------------------------------------------------------------------------------

/** A brief with fields replaced (the wire record itself is never mutated: the movers share it). */
function brief(
  b: GuildBrief,
  over: Partial<Omit<GuildBrief, '$typeName' | '$unknown'>>,
): GuildBrief {
  return { ...b, ...over };
}

function withBrief(s: GuildState, b: GuildBrief): GuildState {
  const briefs = new Map(s.briefs);
  briefs.set(b.idGuild, b);
  return { ...s, briefs };
}

function withOwn(s: GuildState, own: OwnGuild): GuildState {
  return { ...s, own };
}

function withMember(own: OwnGuild, id: number, rec: MemberRecord): OwnGuild {
  const members = new Map(own.members);
  members.set(id, rec);
  return { ...own, members };
}

const NEW_MEMBER: MemberRecord = {
  rank: 4,
  cls: 0,
  alias: '',
  giveGold: 0,
  givePxp: 0,
  surrender: 0,
  win: 0,
  lose: 0,
};

/** The own guild when it is guild `id`. */
const ownOf = (s: GuildState, id: number): OwnGuild | null => (s.own?.id === id ? s.own : null);

// ---- invitation, creation, dissolution -----------------------------------------------------------------------------------------------

/** `GuildInviteNtf`: an unknown guild is ignored (KEEP), the window shows the guild name. */
export function onGuildInvite(
  s: GuildState,
  m: { readonly idGuild: number; readonly idInviter: number },
): Reduced {
  if (!s.briefs.has(m.idGuild)) return same(s);
  return { state: s, effects: [{ kind: 'invite', guildId: m.idGuild, inviter: m.idInviter }] };
}

/** `CreateGuild`: a guild with its master (broadcast to everybody); the founder gets the stub of the original until the full record arrives. */
export function onCreateGuild(
  s: GuildState,
  m: {
    readonly idPlayer: number;
    readonly idGuild: number;
    readonly masterName: string;
    readonly guildName: string;
  },
  env: GuildEnv,
): Reduced {
  if (s.briefs.has(m.idGuild)) return same(s);
  const b = {
    $typeName: 'flyff.v1.GuildBrief',
    idGuild: m.idGuild,
    idMaster: m.idPlayer,
    level: 1,
    name: m.guildName,
    logo: 0,
    guildGold: 0,
    win: 0,
    lose: 0,
    surrender: 0,
  } as GuildBrief;
  let next = withBrief(s, b);
  const effects: GuildEffect[] = [
    { kind: 'need', id: m.idPlayer, name: m.masterName },
    line(TID_GAME_COMCREATE, [m.masterName, m.guildName]),
  ];
  if (m.idPlayer === env.selfId) {
    next = {
      ...next,
      own: {
        id: m.idGuild,
        // `m_adwPower[ GUD_MASTER ] = 0x000000FF`
        powers: [0xff, 0, 0, 0, 0],
        salaries: [0, 0, 0, 0, 0],
        notice: '',
        pxp: 0,
        enemy: 0,
        members: new Map([[m.idPlayer, { ...NEW_MEMBER, rank: 0 }]]),
      },
    };
    effects.push({ kind: 'powerRebase' });
  }
  effects.push({ kind: 'visuals' });
  return { state: next, effects };
}

/** `DestroyGuildNtf`: the guild goes; the line only for the own guild. */
export function onDestroyGuild(
  s: GuildState,
  m: { readonly masterName: string; readonly idGuild: number },
): Reduced {
  const b = s.briefs.get(m.idGuild);
  if (!b) return same(s);
  const effects: GuildEffect[] = [];
  if (s.own?.id === m.idGuild) effects.push(line(TID_GAME_COMDISMESS, [m.masterName, b.name]));
  const briefs = new Map(s.briefs);
  briefs.delete(m.idGuild);
  let next: GuildState = { ...s, briefs };
  if (s.own?.id === m.idGuild) {
    next = { ...next, own: null };
    effects.push({ kind: 'closeWindows' }, { kind: 'powerRebase' });
  }
  // wars the dissolved guild fought are over
  if ([...s.wars.values()].some((w) => w.decl.guild === m.idGuild || w.acpt.guild === m.idGuild)) {
    const wars = new Map(s.wars);
    for (const [id, w] of s.wars) {
      if (w.decl.guild === m.idGuild || w.acpt.guild === m.idGuild) wars.delete(id);
    }
    next = { ...next, wars };
  }
  effects.push({ kind: 'visuals', gone: m.idGuild });
  return { state: next, effects };
}

/**
 * `SetGuild` of the OWN character (`obj_id` = self): `0` leaves the guild (`TID_GAME_COMLEAVE( old name )` only while the old brief is known - after a
 * `DestroyGuildNtf` it is gone and nothing is said), another id joins it (`TID_GAME_COMACCEPT( guild )`).
 */
export function onSetGuildSelf(s: GuildState, idGuild: number): Reduced {
  const effects: GuildEffect[] = [{ kind: 'selfGuild', id: idGuild }];
  if (idGuild === 0) {
    const old = s.own?.id ?? 0;
    const ob = old === 0 ? undefined : s.briefs.get(old);
    if (ob) effects.push(line(TID_GAME_COMLEAVE, [ob.name]));
    effects.push({ kind: 'closeWindows' }, { kind: 'powerRebase' });
    return {
      state: { ...s, own: null, selfWar: 0, bank: { container: null, open: false } },
      effects,
    };
  }
  const b = s.briefs.get(idGuild);
  if (b) effects.push(line(TID_GAME_COMACCEPT, [b.name]));
  // a stale record of another guild goes; the full record is on its way
  const own = s.own && s.own.id !== idGuild ? null : s.own;
  return { state: own === s.own ? s : { ...s, own }, effects };
}

/** `Guild`: the full record of the own guild replaces the previous one; the player data of the members is asked for. */
export function onGuildFull(s: GuildState, g: GuildFull): Reduced {
  const b = g.brief;
  if (!b) return same(s);
  const members = new Map<number, MemberRecord>();
  const sorted = [...g.members].sort((x, y) => x.idPlayer - y.idPlayer);
  for (const m of sorted) {
    members.set(m.idPlayer, {
      rank: m.memberLevel,
      cls: m.class,
      alias: m.alias,
      giveGold: m.giveGold,
      givePxp: m.givePxpCount,
      surrender: m.surrender,
      win: m.win,
      lose: m.lose,
    });
  }
  const five = (list: readonly number[]): number[] =>
    Array.from({ length: 5 }, (_, i) => (list[i] ?? 0) >>> 0);
  const own: OwnGuild = {
    id: b.idGuild,
    powers: five(g.power),
    salaries: five(g.penyaPerLevel),
    notice: g.notice,
    pxp: g.contributionPxp,
    enemy: g.idEnemyGuild,
    members,
  };
  return {
    state: withOwn(withBrief(s, b), own),
    effects: [
      { kind: 'needAll', ids: [...members.keys()] },
      { kind: 'powerRebase' },
      { kind: 'visuals' },
    ],
  };
}

/** `AllGuilds`: every guild of the server in brief replaces the table. */
export function onAllGuilds(
  s: GuildState,
  m: { readonly nextId: number; readonly guilds: readonly GuildBrief[] },
): Reduced {
  const briefs = new Map<number, GuildBrief>();
  for (const b of m.guilds) briefs.set(b.idGuild, b);
  return { state: { ...s, briefs, nextId: m.nextId }, effects: [{ kind: 'visuals' }] };
}

// ---- members -----------------------------------------------------------------------------------------------------------------------------------

/** `AddGuildMemberNtf` of the own guild: a rookie of class 0. */
export function onAddGuildMember(
  s: GuildState,
  m: { readonly idPlayer: number; readonly idGuild: number; readonly name: string },
): Reduced {
  const own = ownOf(s, m.idGuild);
  if (!own || own.members.has(m.idPlayer)) return same(s);
  return {
    state: withOwn(s, withMember(own, m.idPlayer, NEW_MEMBER)),
    effects: [
      { kind: 'need', id: m.idPlayer, name: m.name },
      line(TID_GAME_COMACCEPTWELCOME, [m.name]),
    ],
  };
}

/** `RemoveGuildMemberNtf`: the member goes; the farewell line only when the name is known. */
export function onRemoveGuildMember(
  s: GuildState,
  m: { readonly idPlayer: number; readonly idGuild: number },
  env: GuildEnv,
): Reduced {
  const own = ownOf(s, m.idGuild);
  if (!own?.members.has(m.idPlayer)) return same(s);
  const members = new Map(own.members);
  members.delete(m.idPlayer);
  const name = env.nameOf(m.idPlayer);
  return {
    state: withOwn(s, { ...own, members }),
    effects: name === '' ? [] : [line(TID_GAME_COMLEAVEBYE, [name])],
  };
}

/** `GuildMemberLevelNtf`: the new member level resets the class; PL-124: the promotion / demotion line. */
export function onGuildMemberLevel(
  s: GuildState,
  m: { readonly idPlayer: number; readonly rank: number },
  env: GuildEnv,
): Reduced {
  const own = s.own;
  const old = own?.members.get(m.idPlayer);
  if (!own || !old) return same(s);
  const effects: GuildEffect[] = [];
  if (m.rank !== old.rank) {
    // a LOWER member level number is a promotion (the master is 0)
    effects.push(
      line(m.rank < old.rank ? TID_GAME_GUILDMEM_APPUP : TID_GAME_GUILDMEM_APPDOWN, [
        env.nameOf(m.idPlayer),
      ]),
    );
  }
  return {
    state: withOwn(s, withMember(own, m.idPlayer, { ...old, rank: m.rank, cls: 0 })),
    effects,
  };
}

/** `GuildClassNtf`: the class (rank stars); a line for the change. */
export function onGuildClass(
  s: GuildState,
  m: { readonly idPlayer: number; readonly cls: number },
  env: GuildEnv,
): Reduced {
  const own = s.own;
  const old = own?.members.get(m.idPlayer);
  if (!own || !old) return same(s);
  const effects: GuildEffect[] = [];
  if (m.cls !== old.cls) {
    effects.push(
      line(m.cls > old.cls ? TID_GAME_GUILDMEM_CLASUP : TID_GAME_GUILDMEM_CLASDOWN, [
        env.nameOf(m.idPlayer),
      ]),
    );
  }
  return { state: withOwn(s, withMember(own, m.idPlayer, { ...old, cls: m.cls })), effects };
}

/** `ChgMasterNtf`: the old master becomes a rookie, the new one the master; both classes reset. */
export function onChgMaster(
  s: GuildState,
  m: { readonly oldMaster: number; readonly newMaster: number },
  env: GuildEnv,
): Reduced {
  const own = s.own;
  const a = own?.members.get(m.oldMaster);
  const b = own?.members.get(m.newMaster);
  if (!own || !a || !b) return same(s);
  let next = withMember(withMember(own, m.oldMaster, { ...a, rank: 4, cls: 0 }), m.newMaster, {
    ...b,
    rank: 0,
    cls: 0,
  });
  next = { ...next };
  let state = withOwn(s, next);
  const br = s.briefs.get(own.id);
  if (br) state = withBrief(state, brief(br, { idMaster: m.newMaster }));
  return {
    state,
    effects: [line(TID_GAME_CHANGE_GUILDMASTER, [env.nameOf(m.newMaster)]), { kind: 'refresh' }],
  };
}

/** `GuildNicknameNtf`. */
export function onGuildNickname(
  s: GuildState,
  m: { readonly idPlayer: number; readonly alias: string },
): Reduced {
  const own = s.own;
  const old = own?.members.get(m.idPlayer);
  if (!own || !old) return same(s);
  return {
    state: withOwn(s, withMember(own, m.idPlayer, { ...old, alias: m.alias })),
    effects: [],
  };
}

// ---- power, salary, name, logo, notice, contribution ----------------------------------------------------------------------------------

/** `GuildAuthorityNtf`: the five words; the power draft starts again from them. */
export function onGuildAuthority(s: GuildState, m: { readonly power: readonly number[] }): Reduced {
  const own = s.own;
  if (!own) return same(s);
  const powers = Array.from({ length: 5 }, (_, i) => (m.power[i] ?? 0) >>> 0);
  return {
    state: withOwn(s, { ...own, powers }),
    effects: [{ kind: 'powerRebase' }, line(TID_GAME_GUILDCHGWARRANT)],
  };
}

/** `GuildPenyaNtf`: the salary of one member level. */
export function onGuildPenya(
  s: GuildState,
  m: { readonly rank: number; readonly penya: number },
): Reduced {
  const own = s.own;
  if (!own || !Number.isInteger(m.rank) || m.rank < 0 || m.rank >= 5) return same(s);
  const salaries = own.salaries.map((v, i) => (i === m.rank ? m.penya : v));
  return { state: withOwn(s, { ...own, salaries }), effects: [line(TID_GAME_GUILDCHGPAY)] };
}

/** `GuildSetNameNtf`: the name changes for everybody; the line tells old and new. */
export function onGuildSetName(
  s: GuildState,
  m: { readonly idGuild: number; readonly name: string },
): Reduced {
  const b = s.briefs.get(m.idGuild);
  if (!b) return same(s);
  return {
    state: withBrief(s, brief(b, { name: m.name })),
    effects: [line(TID_GAME_GUILDNAMECHG, [b.name, m.name]), { kind: 'visuals' }],
  };
}

/** `GuildLogoNtf`. */
export function onGuildLogo(
  s: GuildState,
  m: { readonly idGuild: number; readonly logo: number },
): Reduced {
  const b = s.briefs.get(m.idGuild);
  if (!b) return same(s);
  return { state: withBrief(s, brief(b, { logo: m.logo })), effects: [{ kind: 'visuals' }] };
}

/**
 * `GuildContributionNtf` (`CONTRIBUTION_CHANGED_INFO`): the guild's level, points and penya are set, the contributor's counters grow by the deltas. The own contribution prints the
 * success line (FIX: the original printed it when the request was sent); a rising level prints `TID_GAME_GUILDLEVELUP`.
 */
export function onGuildContribution(
  s: GuildState,
  m: {
    readonly idGuild: number;
    readonly idPlayer: number;
    readonly pxpDelta: number;
    readonly penyaDelta: number;
    readonly guildPxp: number;
    readonly guildPenya: number;
    readonly guildLevel: number;
  },
  env: GuildEnv,
): Reduced {
  const b = s.briefs.get(m.idGuild);
  if (!b) return same(s);
  const effects: GuildEffect[] = [];
  if (m.idPlayer === env.selfId) effects.push(line(TID_GAME_GUILDMERITSTUFF));
  if (m.guildLevel > b.level) effects.push(line(TID_GAME_GUILDLEVELUP));
  let next = withBrief(s, brief(b, { level: m.guildLevel, guildGold: m.guildPenya }));
  const own = ownOf(next, m.idGuild);
  if (own) {
    let o: OwnGuild = { ...own, pxp: m.guildPxp };
    const rec = o.members.get(m.idPlayer);
    if (rec) {
      o = withMember(o, m.idPlayer, {
        ...rec,
        givePxp: (rec.givePxp + m.pxpDelta) >>> 0,
        giveGold: (rec.giveGold + m.penyaDelta) | 0,
      });
    }
    next = withOwn(next, o);
  }
  return { state: next, effects };
}

/** `GuildNoticeNtf`. */
export function onGuildNotice(
  s: GuildState,
  m: { readonly idGuild: number; readonly notice: string },
): Reduced {
  if (!s.briefs.has(m.idGuild)) return same(s);
  const own = ownOf(s, m.idGuild);
  const next = own ? withOwn(s, { ...own, notice: m.notice }) : s;
  return { state: next, effects: [line(TID_GAME_GUILDNOTICEUPDATE)] };
}

/** `GuildRealPenya` (the salary payout): the guild's penya; the paid member sees what the rank earns. */
export function onGuildRealPenya(
  s: GuildState,
  m: { readonly gold: number; readonly rank: number },
): Reduced {
  const own = s.own;
  const b = own ? s.briefs.get(own.id) : undefined;
  if (!own || !b) return same(s);
  const salary = own.salaries[m.rank] ?? 0;
  return {
    state: withBrief(s, brief(b, { guildGold: m.gold >>> 0 })),
    effects: salary > 0 ? [line(TID_GAME_GUILD_PAYTAKE, [salary])] : [],
  };
}

/** `GuildGameJoin` / `GuildGameLogin`: the presence overlay is the messenger's; the member views are rebuilt. */
export function onGuildPresence(s: GuildState): Reduced {
  return { state: s, effects: [{ kind: 'refresh' }] };
}

/** `GuildError`: 1 the name is taken, 2 the salary is out of range. */
export function onGuildError(s: GuildState, code: number): Reduced {
  if (code === 1) return { state: s, effects: [line(TID_GAME_COMOVERLAPNAME)] };
  if (code === 2) return { state: s, effects: [line(TID_GAME_GUILDONLYNUMBER)] };
  return same(s);
}

// ---- the bank ----------------------------------------------------------------------------------------------------------------------------------

/** `GuildBankWndNtf` (`OnGuildBankWindow`): mode 0 carries the guild penya and the 42 slots; the window opens. Other modes are ignored. */
export function onGuildBankWnd(
  s: GuildState,
  m: { readonly mode: number; readonly gold: number; readonly container: ItemContainer<InvItem> },
): Reduced {
  if (m.mode !== 0) return same(s);
  const own = s.own;
  const b = own ? s.briefs.get(own.id) : undefined;
  let next: GuildState = { ...s, bank: { container: m.container, open: true } };
  if (b) next = withBrief(next, brief(b, { guildGold: m.gold }));
  return { state: next, effects: [{ kind: 'openBank' }] };
}

/** `PutItemGuildBankNtf` (`OnPutItemGuildBank`): `m_GuildBank.Add( &itemElem )` whatever the mode (1 the receiver put it, 3 another member). */
export function onPutItemGuildBank(s: GuildState, item: InvItem, env: GuildEnv): Reduced {
  const c = s.bank.container;
  if (!c) return same(s);
  const next = c.clone();
  if (next.add(item, env.packMax(item.itemId)) === null) return same(s);
  return { state: { ...s, bank: { ...s.bank, container: next } }, effects: [] };
}

/** The guild penya never goes below zero (`if( gold > Gold ) gold -= Gold; else gold = 0`). */
function takeGold(s: GuildState, gold: number): GuildState {
  const own = s.own;
  const b = own ? s.briefs.get(own.id) : undefined;
  if (!b) return s;
  return withBrief(s, brief(b, { guildGold: b.guildGold > gold ? b.guildGold - gold : 0 }));
}

/** The contribution of a member falls by what he took (`DecrementMemberContribution`; may go negative) - not for the penya of a cloak. */
function decrementContribution(s: GuildState, idPlayer: number, gold: number): GuildState {
  const own = s.own;
  const rec = own?.members.get(idPlayer);
  if (!own || !rec) return s;
  return withOwn(s, withMember(own, idPlayer, { ...rec, giveGold: (rec.giveGold - gold) | 0 }));
}

/**
 * `GetItemGuildBankNtf{penya_mine}`: the penya taken out is the own purse's gain (the server adds nothing: `AddGold`), the guild's penya falls and - unless the penya paid
 * for a cloak - so does the contribution of the member who took it.
 */
export function onGetPenyaMine(
  s: GuildState,
  m: { readonly gold: number; readonly idPlayer: number; readonly cloak: boolean },
): Reduced {
  let next = takeGold(s, m.gold);
  if (!m.cloak) next = decrementContribution(next, m.idPlayer, m.gold);
  return { state: next, effects: [{ kind: 'wallet', gold: m.gold }] };
}

/** `GetItemGuildBankNtf{penya_all}`: another member took penya: the guild's penya and his contribution only. */
export function onGetPenyaAll(
  s: GuildState,
  m: { readonly gold: number; readonly idPlayer: number; readonly cloak: boolean },
): Reduced {
  let next = takeGold(s, m.gold);
  if (!m.cloak) next = decrementContribution(next, m.idPlayer, m.gold);
  return { state: next, effects: [] };
}

/** A stack of the bank is reduced by `taken` pieces (removed when nothing is left). */
function takeFromBank(s: GuildState, objId: number, taken: number): GuildState {
  const c = s.bank.container;
  const it = c?.getAtId(objId) ?? null;
  if (!c || !it) return s;
  const next = c.clone();
  if (it.num > taken) next.replaceAtId(objId, { ...it, num: it.num - taken });
  else next.removeAtId(objId);
  return { ...s, bank: { ...s.bank, container: next } };
}

/**
 * `GetItemGuildBankNtf{item_mine}`: the item taken out (its `objId` is the BANK object id, its count the amount taken) joins the inventory and leaves the bank; an item the
 * bank does not hold is ignored.
 */
export function onGetItemMine(s: GuildState, item: InvItem): Reduced {
  if (!s.bank.container?.getAtId(item.objId)) return same(s);
  return {
    state: takeFromBank(s, item.objId, item.num),
    effects: [{ kind: 'inventoryAdd', item }],
  };
}

/** `GetItemGuildBankNtf{item_all}`: another member took an item: the bank side only (the original's switch had no case for it, so the other mirrors went stale). */
export function onGetItemAll(s: GuildState, item: InvItem): Reduced {
  return { state: takeFromBank(s, item.objId, item.num), effects: [] };
}

/** `0xFFFFFFFF` (`NULL_ID`): the penya in `RemoveGuildBankItem`. */
export const NULL_OBJ_ID = 0xffff_ffff;

/** `RemoveGuildBankItem` (a game master): `NULL_ID` removes penya, else `num` pieces of that bank object (`CGuild::RemoveItem`). */
export function onRemoveGuildBankItem(
  s: GuildState,
  m: { readonly idGuild: number; readonly itemObjId: number; readonly num: number },
): Reduced {
  const b = s.briefs.get(m.idGuild);
  if (!b) return same(s);
  if (m.itemObjId >>> 0 === NULL_OBJ_ID) {
    return {
      state: withBrief(s, brief(b, { guildGold: b.guildGold > m.num ? b.guildGold - m.num : 0 })),
      effects: [],
    };
  }
  if (s.own?.id !== m.idGuild) return same(s);
  return { state: takeFromBank(s, m.itemObjId, m.num), effects: [] };
}

/** `GuildLogViewNtf`: the list of a kind is replaced (kept whether or not the window is open). */
export function onGuildLogView(
  s: GuildState,
  m: { readonly kind: number; readonly entries: readonly LogEntry[] },
  env: LogEnv,
): Reduced {
  if (!Number.isInteger(m.kind) || m.kind < 1 || m.kind > 4) return same(s);
  const log = s.log.map((l, i) =>
    i === m.kind - 1 ? formatLog(m.kind as BankLogKind, m.entries, env) : l,
  );
  return { state: { ...s, log }, effects: [] };
}

/** A tab of the log window was selected: its list is cleared until the answer comes (`m_vLogList.clear()`). */
export function onLogRequested(s: GuildState, kind: number): Reduced {
  if (!Number.isInteger(kind) || kind < 1 || kind > 4) return same(s);
  if ((s.log[kind - 1]?.length ?? 0) === 0) return same(s);
  return { state: { ...s, log: s.log.map((l, i) => (i === kind - 1 ? [] : l)) }, effects: [] };
}

/** The bank window was closed. */
export function onBankClosed(s: GuildState): Reduced {
  if (!s.bank.open) return same(s);
  return { state: { ...s, bank: { ...s.bank, open: false } }, effects: [] };
}

/** An empty bank container of 42 slots (tests and the module's fallback). */
export function emptyBank(): ItemContainer<InvItem> {
  return new ItemContainer<InvItem>(GUILD_BANK_SLOTS, 0);
}

// ---- guild wars -------------------------------------------------------------------------------------------------------------------------

const WAR_REASON: readonly number[] = [
  TID_GAME_GUILDWARWIN_MASTER,
  TID_GAME_GUILDWARWIN_GIVEUP,
  TID_GAME_GUILDWARWIN_TIMEUP,
  TID_GAME_GUILDWARWIN_KILLNUM,
];

/** `WR_*` codes: 0 DECL_GN, 1 ACPT_GN, 2 DECL_SR, 3 ACPT_SR, 4 DECL_AB, 5 ACPT_AB, 6 DECL_DD, 7 ACPT_DD, 8 TRUCE, 9 DRAW. */
export const WR = { TRUCE: 8, DRAW: 9 } as const;

const side = (e: WarSide): WarSide => e;

function sideOf(e: {
  readonly idGuild: number;
  readonly size: number;
  readonly surrender: number;
  readonly dead: number;
  readonly absent: number;
}): WarSide {
  return side({
    guild: e.idGuild,
    size: e.size,
    surrender: e.surrender,
    dead: e.dead,
    absent: e.absent,
  });
}

/** `DeclGuildWarNtf`: the declaration window (a declaring guild the client does not know is ignored). */
export function onDeclGuildWar(
  s: GuildState,
  m: { readonly idDeclGuild: number; readonly masterName: string },
): Reduced {
  if (!s.briefs.has(m.idDeclGuild)) return same(s);
  return {
    state: s,
    effects: [{ kind: 'warRequest', idDecl: m.idDeclGuild, masterName: m.masterName }],
  };
}

/** `QueryTruceNtf`: the truce approval window. */
export function onQueryTruce(s: GuildState): Reduced {
  return { state: s, effects: [{ kind: 'truceConfirm' }] };
}

/** `own.enemy` for a war the own guild fights in. */
function enemyIn(s: GuildState, decl: number, acpt: number): GuildState {
  const own = s.own;
  if (!own) return s;
  if (own.id === decl) return withOwn(s, { ...own, enemy: acpt });
  if (own.id === acpt) return withOwn(s, { ...own, enemy: decl });
  return s;
}

/** The member count of a guild as the client knows it (only the own guild has a roster; the others are briefs, as in the original). */
const sizeOf = (s: GuildState, id: number): number => (s.own?.id === id ? s.own.members.size : 0);

/** `AcptGuildWarNtf`: the war starts (both guilds must be known; a war with that id already known is ignored). */
export function onAcptGuildWar(
  s: GuildState,
  m: { readonly idWar: number; readonly idDecl: number; readonly idAcpt: number },
  now: number,
): Reduced {
  const decl = s.briefs.get(m.idDecl);
  const acpt = s.briefs.get(m.idAcpt);
  if (!decl || !acpt || s.wars.has(m.idWar)) return same(s);
  const war: WarState = {
    id: m.idWar,
    decl: { guild: m.idDecl, size: sizeOf(s, m.idDecl), surrender: 0, dead: 0, absent: 0 },
    acpt: { guild: m.idAcpt, size: sizeOf(s, m.idAcpt), surrender: 0, dead: 0, absent: 0 },
    // `WF_WARTIME`
    flag: 0x30,
    startAt: now,
  };
  const wars = new Map(s.wars);
  wars.set(m.idWar, war);
  return {
    state: enemyIn({ ...s, wars }, m.idDecl, m.idAcpt),
    effects: [line(TID_GAME_GUILDWARSTART, [decl.name, acpt.name])],
  };
}

/** `War` (the join batch and the re-attach): the war is stored (replacing the old one); the lines tell it is still going on. */
export function onWar(s: GuildState, w: GuildWarInfo): Reduced {
  if (!w.decl || !w.acpt) return same(s);
  const war: WarState = {
    id: w.idWar,
    decl: sideOf(w.decl),
    acpt: sideOf(w.acpt),
    flag: w.flag,
    startAt: Number(w.startAt),
  };
  const wars = new Map(s.wars);
  wars.set(w.idWar, war);
  const next = enemyIn({ ...s, wars }, war.decl.guild, war.acpt.guild);
  const decl = s.briefs.get(war.decl.guild);
  const acpt = s.briefs.get(war.acpt.guild);
  const effects: GuildEffect[] =
    decl && acpt
      ? [line(TID_GAME_GUILDWARSTILL, [decl.name, acpt.name]), line(TID_GAME_GUILDWARDOING)]
      : [];
  return { state: next, effects };
}

/** `SetWar` of the OWN character (`obj_id` = self): `g_pPlayer->m_idWar`. */
export function onSetWarSelf(s: GuildState, idWar: number): Reduced {
  if (s.selfWar === idWar) return same(s);
  return { state: { ...s, selfWar: idWar }, effects: [{ kind: 'visuals' }] };
}

function bumpWar(
  s: GuildState,
  idWar: number,
  decl: boolean,
  f: (side: WarSide) => WarSide,
): { state: GuildState; war: WarState; side: WarSide } | null {
  const war = s.wars.get(idWar);
  if (!war) return null;
  const nextWar: WarState = decl ? { ...war, decl: f(war.decl) } : { ...war, acpt: f(war.acpt) };
  const wars = new Map(s.wars);
  wars.set(idWar, nextWar);
  return { state: { ...s, wars }, war: nextWar, side: decl ? nextWar.decl : nextWar.acpt };
}

/** `SurrenderNtf`: the side's surrender counter and - for the own guild - the player's. */
export function onSurrender(
  s: GuildState,
  m: {
    readonly idWar: number;
    readonly idPlayer: number;
    readonly name: string;
    readonly decl: boolean;
  },
): Reduced {
  const r = bumpWar(s, m.idWar, m.decl, (x) => ({ ...x, surrender: x.surrender + 1 }));
  if (!r) return same(s);
  const g = s.briefs.get(r.side.guild);
  if (!g) return { state: r.state, effects: [] };
  let next = r.state;
  const own = ownOf(next, g.idGuild);
  const rec = own?.members.get(m.idPlayer);
  if (own && rec) {
    next = withOwn(next, withMember(own, m.idPlayer, { ...rec, surrender: rec.surrender + 1 }));
  }
  return { state: next, effects: [line(TID_GAME_GUILDWARGIVEUP, [g.name, m.name])] };
}

/** `WarDead`: the side's dead counter and a line. */
export function onWarDead(
  s: GuildState,
  m: { readonly idWar: number; readonly playerName: string; readonly decl: boolean },
): Reduced {
  const r = bumpWar(s, m.idWar, m.decl, (x) => ({ ...x, dead: x.dead + 1 }));
  if (!r) return same(s);
  const g = s.briefs.get(r.side.guild);
  return {
    state: r.state,
    effects: g ? [line(TID_GAME_GUILDWARDEAD, [g.name, m.playerName])] : [],
  };
}

/**
 * `WarEnd` (`CGuildWarMng::Result`): the result line, the win / loss counters of the guilds (results below 8), the war ends for the own guild (`own.enemy`, `selfWar`) and
 * disappears. A war or a guild the client does not know is ignored (an orphan war: the server may name a guild the client never heard of).
 */
export function onWarEnd(
  s: GuildState,
  m: { readonly idWar: number; readonly result: number },
): Reduced {
  const war = s.wars.get(m.idWar);
  if (!war) return same(s);
  const decl = s.briefs.get(war.decl.guild);
  const acpt = s.briefs.get(war.acpt.guild);
  if (!decl || !acpt) return same(s);
  const r = m.result;
  const winnerDecl = r % 2 === 0 || r === WR.TRUCE || r === WR.DRAW;
  const win = winnerDecl ? decl : acpt;
  const lose = winnerDecl ? acpt : decl;
  const effects: GuildEffect[] = [];
  if (r === WR.TRUCE) effects.push(line(TID_GAME_GUILDWARPEACE, [decl.name, acpt.name]));
  else if (r === WR.DRAW) effects.push(line(TID_GAME_GUILDWARDRAW, [decl.name, acpt.name]));
  else {
    const reason = WAR_REASON[Math.floor(r / 2)];
    if (reason !== undefined && r >= 0) {
      effects.push(line(TID_GAME_GUILDWARWIN, [decl.name, acpt.name, win.name, { tid: reason }]));
    }
  }
  let next = s;
  if (r < WR.TRUCE) {
    next = withBrief(
      withBrief(next, brief(win, { win: win.win + 1 })),
      brief(lose, { lose: lose.lose + 1 }),
    );
  }
  const wars = new Map(next.wars);
  wars.delete(m.idWar);
  next = { ...next, wars };
  const own = next.own;
  if (own && (own.id === war.decl.guild || own.id === war.acpt.guild)) {
    next = { ...next, own: { ...own, enemy: 0 }, selfWar: 0 };
  }
  effects.push({ kind: 'visuals' });
  return { state: next, effects };
}
