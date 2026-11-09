// The `messenger` feature module (Plan/45 §24.1, §8.3, §8.6, §0.4; 21 §9, §3.2-§3.5, §4.11; 23 §6.11; WP-CFEAT-39): the friend list and the messenger windows, the instant messages,
// the notes, the privacy settings and the ignore list in server mode, the friend summon and the rename.
//
//   AddFriendGameJoin ... AddFriendLogout        the friend list and its states (`reducers.ts`), the cache of names (`needAll`), `GetFriendStateReq` after the list and a new friend
//   AddFriendRequestNtf, AddFriendNameRequestNtf  `APP_FRIEND_CONFIRM` (Yes `AddFriendReq`, No `AddFriendCancelReq`); a second one is answered with `BlockReq{ 6 }`; ignored in world 202
//   Say                                           the IM windows (`APP_MESSAGE`, one per partner) and, without a window, the popup (`APP_INSTANTMSG`) plus the chat line and the cue
//   Tag, InsertTagResult                          the notes of the join (`APP_MESSENGER_NOTE`, replaced by a replay) and the answer of a note
//   ChatSettingsNtf                               the server owns the privacy flags and the ignore list; `/ignorelist` opens `APP_CHATTING_BLOCKING_LIST`
//   Summon{ friend }, Summon{ friend_confirm }    `APP_SUMMON_FRIEND` and `APP_MESSAGEBOX_SUMMONFRINED`; the word `data` is echoed unchanged (the scroll's object id in the LOW word)
//   QuerySetPlayerNameNtf                         a rename: the IM window, the popup, the own name, the nameplate and the effect
//   Guild, GuildGameJoin, GuildGameLogin ...      the roster and the presence overlay of the guild page
//
// Other features reach it with `messengerOf(ctx)` (`api.ts`).

import { AUTH_GAMEMASTER } from '../../app/hosts';
import { entityInfoOf } from '../../app/inputHost';
import { EntityKind, type EntityRecord } from '../../game/entities';
import { fxRuntime } from '../../game/fx';
import { II_SYS_SYS_SCR_FRIENDSUMMON_A } from '../../gen/defs/item';
import { MMI_ADD_MESSENGER } from '../../gen/defs/neuz';
import { XI_SYS_EXCHAN01 } from '../../gen/defs/obj';
import {
  TID_DIAG_0051,
  TID_DIAG_0056,
  TID_DIAG_0057,
  TID_DIAG_0071,
  TID_DIAG_SUMMONFRIEND_CONFIRM,
  TID_ERROR_SUMMONFRIEND_NOITEM,
  TID_GAME_CHANGENAME,
  TID_GAME_FROM3,
  TID_GAME_GUILDCOMBAT_CANNOT_FRIENDADD,
  TID_GAME_LOGIN,
  TID_GAME_MESSFULLMSG,
  TID_GAME_MSGBLOCKCHR,
  TID_GAME_MSGINVATE,
  TID_GAME_NOTLOGIN,
  TID_GAME_SUMMON_FRIEND_MY_NOUSE,
} from '../../gen/defs/text';
import { WI_WORLD_GUILDWAR } from '../../gen/defs/world';
import type { InputInstance } from '../../input/input';
import type {
  AddFriendRequest,
  FriendMenuEntry,
  ImRequest,
  MessengerActions,
  MessengerPage,
  MessengerPorts,
  MessengerRow,
  MessengerView,
  ViewSource,
} from '../../ui/windows/messenger/ports';
import { FRIEND_MENU, FRS } from '../../ui/windows/messenger/ports';
import type { SpawnPlayer } from '../../gen/proto/flyff/v1/world_pb';
import { chatOf } from '../chat/chatModule';
import { COLOR_MESSENGER, STYLE_CLIENT_LINE } from '../chat/display';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { type PlayerDataApi, registerPlayerData } from '../common/playerData';
import { formatTid, SYSTEM_CHAT, sysChat } from '../common/text';
import { inventoryOf } from '../inventory';
import { MODE_MAILBOX } from '../mail';
import { menuOf } from '../npc/menuModule';
import { type Observable, deepEqual } from '../party/view';
import { partyOf } from '../party/partyModule';
import { taskbarOf } from '../taskbar/taskbarModule';
import {
  type GuildRosterSource,
  type MessengerFeature,
  type PopupRequest,
  setMessengerOf,
  type VisitPorts,
} from './api';
import { ImRegistry, ImSession } from './im';
import {
  onAddFriendCancel,
  onAddFriendError,
  onAddFriendGameJoin,
  onAddFriendJoin,
  onAddFriendLogout,
  onAddFriendNameNotFound,
  onAddFriendNtf,
  onAddGuildMember,
  onChatSettings,
  onDestroyGuild,
  onFriendIntercept,
  onFriendNoIntercept,
  onGetFriendState,
  onGuild,
  onGuildGameJoin,
  onGuildGameLogin,
  onInsertTagResult,
  onNotesChecked,
  onPresenceGone,
  onRemoveFriendNtf,
  onRemoveFriendState,
  onRemoveGuildMember,
  onSetFriendState,
  onSetGuild,
  onTag,
  type MessengerEffect,
  type Reduced,
} from './reducers';
import { messengerSenders } from './senders';
import { blockListView } from './settings';
import { registerProbe } from './probe';
import { defineMessengerStore, initialMessengerState } from './store';
import { summonCandidates, summonOutcome } from './summon';
import { friendMenuEntries, friendReachable, friendRows, guildRowsOf, type RowFacts } from './view';

/** The life of a conversation popup (`m_timer.Set( SEC( 10 ) )`) and of the mail popup (`MIN( 1 )`). */
export const POPUP_TTL_MS = 10_000;
export const MAIL_POPUP_TTL_MS = 60_000;

/** `CWndMgr::m_timerMessenger` (`MIN( 5 )`): the shortest time between two `SetMessengerAutoState` replies. */
const AUTO_STATE_MS = 5 * 60_000;

const byteLength = (s: string): number => new TextEncoder().encode(s).length;

/** A view that is computed when one of its sources changes (the messenger rows do not move without a message) and read for free by the window every frame. */
function sourceView<T>(
  compute: () => T,
  sources: readonly Observable[],
): ViewSource<T> & {
  invalidate(): void;
} {
  let value: T | undefined;
  const listeners = new Set<() => void>();
  const recompute = (): void => {
    const next = compute();
    if (value !== undefined && deepEqual(value, next)) return;
    value = next;
    for (const fn of [...listeners]) fn();
  };
  for (const s of sources) s.subscribe(recompute);
  return {
    get() {
      value ??= compute();
      return value;
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => {
        listeners.delete(fn);
      };
    },
    invalidate: recompute,
  };
}

/** The state of a conversation popup: who it belongs to and the last line (the click hands over only that one). */
interface PopupMeta {
  name: string;
  id: number | null;
  last: string | null;
}

export const messengerModule: FeatureModule = {
  id: 'messenger',
  milestone: 6,
  register(fc) {
    registerMessenger(fc);
  },
};

function registerMessenger(fc: FeatureContext): MessengerFeature {
  const ctx = fc.app;
  const { selfStore, sessionStore, worldStore } = ctx.stores;
  const store = defineMessengerStore(ctx.stores.registry);
  const playerData: PlayerDataApi = registerPlayerData(fc);
  const senders = messengerSenders((msg) => ctx.net.sender.send(msg));
  const ims = new ImRegistry();
  const popups = new Map<string, PopupMeta>();
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  // the lines of the join batch (`Tag`: "N new message(s)") arrive before the world is entered, and entering it empties the chat log (`OpenField`): they wait for it
  let waiting: { tid: number; args: readonly string[] }[] = [];
  const sysLine = (tid: number, args: readonly string[] = []): void => {
    if (ctx.machine.screen.s !== 'InGame') {
      waiting.push({ tid, args });
      return;
    }
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
  };
  ctx.machine.subscribe((screen, previous) => {
    if (screen.s === 'InGame' && previous.s !== 'InGame') {
      const lines = waiting;
      waiting = [];
      for (const l of lines) sysChat(fc.texts, fullUi(ctx)?.chat ?? null, l.tid, l.args);
    } else if (screen.s !== 'InGame' && previous.s === 'InGame' && screen.s !== 'Reconnecting') {
      waiting = [];
    }
  });
  const selfId = (): number => selfStore.getState().idPlayer;
  const selfName = (): string => selfStore.getState().name;
  const info = entityInfoOf(ctx);

  let roster: GuildRosterSource | null = null;
  let visit: VisitPorts | null = null;
  let campusPoints: (() => number) | null = null;

  // ---- the character ids of the movers in view ------------------------------------------------------------------------------------

  const valid = (rec: EntityRecord | undefined): rec is EntityRecord =>
    rec !== undefined && !rec.removed;
  const characterOf = (rec: EntityRecord): number | undefined =>
    rec.idPlayer ?? (rec.spawn as SpawnPlayer).player?.idPlayer;
  const playerRecords = (id: number): EntityRecord[] => {
    const out: EntityRecord[] = [];
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (!rec.removed && characterOf(rec) === id) out.push(rec);
    });
    return out;
  };
  /** The id of a character by name, from the cache (case-insensitive); `null` when it is not known. */
  const idOfName = (name: string): number | null => {
    const lower = name.toLowerCase();
    if (lower === '') return null;
    if (selfName().toLowerCase() === lower) return selfId();
    for (const [id, e] of playerData.store.getState().entries) {
      if (e.name !== '' && e.name.toLowerCase() === lower) return id;
    }
    return null;
  };

  // ---- the read models -------------------------------------------------------------------------------------------------------------

  const rowFacts = (): RowFacts => ({
    cached: (id) => {
      const e = playerData.get(id);
      return e ? { name: e.name, level: e.level, job: e.job, online: e.online } : undefined;
    },
    job: (j) => {
      const row = ctx.data?.job(j);
      return { type: row?.type ?? 0, name: row?.name ?? '' };
    },
    ...(visit ? { visitAllowed: (id: number) => visit?.allowed(id) ?? false } : {}),
  });
  const rosterIds = (): readonly number[] => roster?.ids() ?? store.getState().guild;
  const compute = (): MessengerView => {
    const s = store.getState();
    const self = selfStore.getState();
    const facts = rowFacts();
    return {
      myState: s.myState,
      myName: self.name,
      serverName: sessionStore.getState().server.name,
      campusPoints: campusPoints?.() ?? 0,
      mailFlash: (self.modeFlags & MODE_MAILBOX) !== 0,
      noteFlash: s.notes.length > 0 && !s.tagChecked,
      friends: friendRows(s, facts),
      guild: guildRowsOf(rosterIds(), s.presence, facts),
    };
  };
  const view = sourceView(compute, [store, playerData.store, selfStore, sessionStore]);
  ctx.machine.subscribe(() => {
    view.invalidate();
  });

  const friendRow = (id: number): MessengerRow | undefined =>
    friendRows(store.getState(), rowFacts()).find((r) => r.id === id);

  // ---- carrying out the effects of a reducer ------------------------------------------------------------------------------------------

  const popupJoin = (id: number): void => {
    const name = playerData.nameOf(id);
    if (name === '') return;
    showPopup({
      key: name,
      id,
      head: `#cff0000ff${name} ${text(TID_GAME_LOGIN)}\n`,
      body: '',
      last: null,
      ttlMs: POPUP_TTL_MS,
    });
  };

  const run = (effects: readonly MessengerEffect[]): void => {
    for (const e of effects) {
      switch (e.kind) {
        case 'line':
          sysLine(e.tid, e.args);
          break;
        case 'box':
          fullUi(ctx)?.msgbox.open(formatTid(fc.texts, e.tid, e.args));
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
        case 'getState':
          senders.getFriendState();
          break;
        case 'imBlocked':
          ims.byId(e.id)?.setBlocked(e.blocked);
          break;
        case 'popupJoin':
          popupJoin(e.id);
          break;
      }
    }
  };
  const apply = (r: Reduced): void => {
    if (r.state !== store.getState()) store.setState(r.state);
    run(r.effects);
  };

  // ---- the IM windows -----------------------------------------------------------------------------------------------------------------

  const maxChars = (name: string): number =>
    Math.max(1, ctx.features.gameplay.chat_max_chars - (8 + Array.from(name).length));

  const openMessage = (name: string, idPlayer?: number): ImSession | null => {
    const ui = fullUi(ctx);
    if (!ui || name === '') return null;
    let session = ims.get(name);
    if (!session) {
      const id = idPlayer ?? idOfName(name);
      const blocked = id !== null && store.getState().friends.get(id)?.blocked === true;
      session = new ImSession(name, id, blocked);
      ims.add(session);
    } else if (idPlayer !== undefined && session.partnerId === null) {
      session.setPartnerId(idPlayer);
    }
    const s = session;
    const request: ImRequest = {
      view: s,
      maxChars: maxChars(name),
      send: (line) => {
        chatOf(ctx)?.submit(`/say "${s.partner}" ${line}`);
      },
      closed: () => {
        ims.remove(s);
      },
    };
    ui.messenger.openMessage(request);
    return s;
  };

  const closeMessage = (name: string): void => {
    fullUi(ctx)?.messenger.message(name)?.destroy();
  };

  // ---- the popups ---------------------------------------------------------------------------------------------------------------------------

  interface PopupShow {
    readonly key: string;
    readonly id: number | null;
    readonly head: string;
    readonly body: string;
    readonly last: string | null;
    readonly ttlMs: number;
    readonly click?: () => boolean;
  }

  /** `OpenInstantMsg` + `AddMessage`: opens the popup of a key or adds a line to the open one (the timer starts again); one cue per line. */
  function showPopup(p: PopupShow): void {
    const ui = fullUi(ctx);
    if (!ui) return;
    const meta: PopupMeta = popups.get(p.key) ?? { name: p.key, id: p.id, last: null };
    meta.last = p.last ?? meta.last;
    if (p.id !== null) meta.id = p.id;
    popups.set(p.key, meta);
    const open = ui.messenger.instant(p.key);
    const click =
      p.click ??
      ((): boolean => {
        // a click on a conversation popup opens the IM window with the LAST line only (KEEP)
        const session = openMessage(meta.name, meta.id ?? undefined);
        if (session && meta.last !== null && meta.last !== '') {
          session.append({ mine: false, from: meta.name, text: meta.last });
        }
        return true;
      });
    if (open) {
      open.addText({ head: p.head, body: p.body });
    } else {
      ui.messenger.openInstant({
        key: p.key,
        line: { head: p.head, body: p.body },
        ttlMs: p.ttlMs,
        click,
        closed: () => {
          for (const [k, m] of popups) if (m === meta) popups.delete(k);
        },
      });
    }
    ctx.audio.cues?.instantMessage();
  }

  /** The popup service for the other features (`Mail`, M7's `Event`). */
  const popup = (r: PopupRequest): void => {
    showPopup({
      key: r.key,
      id: null,
      head: '',
      body: r.text,
      last: null,
      ttlMs: r.ttlMs,
      // `Mail` stays on a click, `Event` closes: the request says
      click: r.onClick ?? ((): boolean => false),
    });
  };

  // ---- the messages: the list ---------------------------------------------------------------------------------------------------------------

  fc.net.on('addFriendGameJoin', (m) => {
    apply(
      onAddFriendGameJoin(store.getState(), {
        myState: m.messenger?.myState ?? 0,
        friends: (m.messenger?.friends ?? []).map((f) => ({
          id: f.idFriend,
          blocked: f.blocked,
          state: f.state,
        })),
      }),
    );
  });
  fc.net.on('addFriendNtf', (m) => {
    apply(onAddFriendNtf(store.getState(), { idPlayer: m.idPlayer, name: m.name }));
  });
  fc.net.on('removeFriendNtf', (m) => {
    apply(onRemoveFriendNtf(store.getState(), m.idPlayer));
  });
  fc.net.on('removeFriendState', (m) => {
    apply(onRemoveFriendState(store.getState(), m.remover));
  });
  fc.net.on('getFriendStateNtf', (m) => {
    apply(
      onGetFriendState(store.getState(), {
        friends: m.friends.map((f) => ({ id: f.idFriend, state: f.state })),
        blocked: m.blocked.map((f) => ({ id: f.idFriend, state: f.state })),
      }),
    );
  });
  fc.net.on('setFriendStateNtf', (m) => {
    apply(onSetFriendState(store.getState(), { idPlayer: m.idPlayer, state: m.state }, selfId()));
  });
  fc.net.on('oneFriendState', (m) => {
    apply(onSetFriendState(store.getState(), { idPlayer: m.idPlayer, state: m.state }, -1));
  });
  fc.net.on('friendInterceptStateNtf', (m) => {
    apply(
      onFriendIntercept(store.getState(), { idPlayer: m.idPlayer, idFriend: m.idFriend }, selfId()),
    );
  });
  fc.net.on('friendNoIntercept', (m) => {
    apply(onFriendNoIntercept(store.getState(), { idFriend: m.idFriend, state: m.state }));
  });
  fc.net.on('addFriendJoin', (m) => {
    apply(onAddFriendJoin(store.getState(), { idFriend: m.idFriend, state: m.state }));
  });
  fc.net.on('addFriendLogout', (m) => {
    apply(onAddFriendLogout(store.getState(), m.idFriend));
  });
  fc.net.on('addFriendCancelNtf', () => {
    apply(onAddFriendCancel(store.getState()));
  });
  fc.net.on('addFriendError', (m) => {
    apply(onAddFriendError(store.getState(), { error: m.error, name: m.name }));
  });
  fc.net.on('addFriendNameNotFound', (m) => {
    apply(onAddFriendNameNotFound(store.getState(), m.name));
  });
  fc.net.on('tag', (m) => {
    apply(
      onTag(
        store.getState(),
        m.notes.map((n) => ({ from: n.fromName, date: n.date, text: n.text })),
      ),
    );
  });
  fc.net.on('insertTagResult', (m) => {
    apply(onInsertTagResult(store.getState(), m.ok));
  });
  fc.net.on('chatSettingsNtf', (m) => {
    apply(
      onChatSettings(store.getState(), {
        trade: m.trade,
        whisper: m.whisper,
        party: m.party,
        shout: m.shout,
        messenger: m.messenger,
        messengerJoin: m.messengerJoin,
        ignored: [...m.ignored],
      }),
    );
  });

  // ---- the messages: the guild roster and the presence ------------------------------------------------------------------------------------

  fc.net.on('guild', (m) => {
    const g = m.guild;
    if (!g) return;
    const r = onGuild(store.getState(), {
      idGuild: g.brief?.idGuild ?? 0,
      members: g.members.map((x) => x.idPlayer),
    });
    // the guild feature (the roster source) queries its members itself: one `QueryPlayerData2` per chunk, not two
    apply(roster ? { state: r.state, effects: [] } : r);
  });
  fc.net.on('setGuild', (m) => {
    apply(onSetGuild(store.getState(), m.idGuild));
  });
  fc.net.on('addGuildMemberNtf', (m) => {
    apply(
      onAddGuildMember(store.getState(), {
        idGuild: m.idGuild,
        idPlayer: m.info?.idPlayer ?? 0,
        name: m.name,
      }),
    );
  });
  fc.net.on('removeGuildMemberNtf', (m) => {
    apply(onRemoveGuildMember(store.getState(), { idGuild: m.idGuild, idPlayer: m.idPlayer }));
  });
  fc.net.on('destroyGuildNtf', (m) => {
    apply(onDestroyGuild(store.getState(), m.idGuild));
  });
  fc.net.on('guildGameJoin', (m) => {
    apply(onGuildGameJoin(store.getState(), { ids: m.ids, multis: m.multis }));
  });
  fc.net.on('guildGameLogin', (m) => {
    apply(
      onGuildGameLogin(store.getState(), {
        login: m.login,
        idPlayer: m.idPlayer,
        multiNo: m.multiNo,
      }),
    );
  });
  // second handlers next to the cache's (`registerPlayerData`): a character who left or went offline is not online in the overlay any more
  fc.net.on('presenceLogout', (m) => {
    apply(onPresenceGone(store.getState(), m.idPlayer));
  });
  fc.net.on('updatePlayerData', (m) => {
    if (m.data?.login === 0) apply(onPresenceGone(store.getState(), m.idPlayer));
  });

  // ---- friend requests ----------------------------------------------------------------------------------------------------------------------------

  const onFriendRequest = (requester: number, name: string): void => {
    // `WI_WORLD_GUILDWAR`: no friend requests in the guild-war world
    if (worldStore.getState().worldId === WI_WORLD_GUILDWAR) return;
    const ui = fullUi(ctx);
    if (!ui) return;
    // a window is open already: the newcomer is told (`SendBlock( 6 )`), it does not get a window
    if (ui.messenger.isFriendConfirmOpen()) {
      senders.block(6, name);
      return;
    }
    ui.messenger.openFriendConfirm({
      text: formatTid(fc.texts, TID_DIAG_0071, [name]),
      yes: () => {
        senders.addFriend(requester);
      },
      no: () => {
        senders.friendCancel(requester);
      },
    });
  };
  fc.net.on('addFriendRequestNtf', (m) => {
    onFriendRequest(m.requester, m.name);
  });
  fc.net.on('addFriendNameRequestNtf', (m) => {
    onFriendRequest(m.requester, m.requesterName);
  });

  /** `MMI_ADD_MESSENGER` (the player menu): a friend already -> the box; else the request and the line. */
  menuOf(ctx)?.registerAction(MMI_ADD_MESSENGER, ({ targetId }) => {
    const rec = ctx.entities.get(targetId);
    if (!valid(rec) || rec.kind !== EntityKind.Player) return;
    const id = characterOf(rec);
    if (id === undefined) return;
    if (store.getState().friends.has(id)) {
      fullUi(ctx)?.msgbox.open(text(TID_DIAG_0051));
      return;
    }
    senders.addFriendRequest(id);
    sysLine(TID_GAME_MSGINVATE, [rec.visual?.name ?? '']);
  });

  /** `CWndAddFriend::OnChildNotify( WIDC_OK )`: the checks in the order of the original, then `AddFriendNameRequestReq`. */
  const addFriendRequest: AddFriendRequest = {
    submit(name) {
      const ui = fullUi(ctx);
      if (byteLength(name) >= 64) {
        ui?.msgbox.open(text(TID_DIAG_0057));
        return 'clear';
      }
      if (name === selfName()) {
        ui?.msgbox.open(text(TID_DIAG_0056));
        return 'clear';
      }
      if (worldStore.getState().worldId === WI_WORLD_GUILDWAR) {
        ui?.msgbox.open(text(TID_GAME_GUILDCOMBAT_CANNOT_FRIENDADD));
        return 'close';
      }
      senders.addFriendNameRequest(name);
      sysLine(TID_GAME_MSGINVATE, [name]);
      return 'close';
    },
  };

  // ---- instant messages --------------------------------------------------------------------------------------------------------------------------

  fc.net.on('say', (m) => {
    // `search != 0` (the partner is not online) is the chat module's line
    if (m.search !== 0) return;
    const ui = fullUi(ctx);
    if (!ui) return;
    const me = selfName();
    if (m.from === me) {
      // my own line comes back from the server: the window of the partner opens when there is none
      const session = openMessage(m.to, m.idTo !== 0 ? m.idTo : undefined);
      session?.append({ mine: true, from: me, text: m.text });
      return;
    }
    const open = ims.get(m.from);
    if (open) {
      open.append({ mine: false, from: m.from, text: m.text });
      return;
    }
    showPopup({
      key: m.from,
      id: m.idFrom !== 0 ? m.idFrom : null,
      head: `#cff0000ff${m.from}${text(TID_GAME_FROM3)} :#nc\n  `,
      body: `${m.text}\n`,
      last: m.text,
      ttlMs: POPUP_TTL_MS,
    });
    ui.chat.putString(`[${m.from}] ${m.text}`, null, COLOR_MESSENGER, STYLE_CLIENT_LINE);
  });

  // ---- the actions of the messenger window ------------------------------------------------------------------------------------------------------

  const targeting = (): InputInstance['core']['targeting'] | null =>
    (ctx.input as Partial<InputInstance>).core?.targeting ?? null;

  /** The line of a refused double click (`TID_GAME_NOTLOGIN` and its colour for both texts, as the original). */
  const refusedLine = (line: string): void => {
    fullUi(ctx)?.chat.putString(line, null, fc.texts.textColor(TID_GAME_NOTLOGIN), SYSTEM_CHAT);
  };

  const activate = (page: MessengerPage, id: number): void => {
    const rows = page === 'friend' ? view.get().friends : view.get().guild;
    const row = rows.find((r) => r.id === id);
    if (!row) return;
    if (page === 'friend') {
      if (friendReachable(row)) {
        openMessage(row.name, row.id);
      } else if (row.state === FRS.OFFLINE) {
        refusedLine(text(TID_GAME_NOTLOGIN));
      } else {
        refusedLine(formatTid(fc.texts, TID_GAME_MSGBLOCKCHR, [row.name]));
      }
      return;
    }
    if (row.state !== FRS.OFFLINE) openMessage(row.name, row.id);
    else refusedLine(text(TID_GAME_NOTLOGIN));
  };

  const isPartyMember = (id: number): boolean =>
    partyOf(ctx)
      ?.store.getState()
      .members.some((x) => x.id === id) ?? false;

  const friendMenu = (id: number): readonly FriendMenuEntry[] => {
    const row = friendRow(id);
    if (!row) return [];
    return friendMenuEntries(row, {
      text,
      isGm: sessionStore.getState().authority >= AUTH_GAMEMASTER,
      inParty: isPartyMember,
      visitAllowed: visit ? visit.allowed(id) : undefined,
    });
  };

  const openNoteWindow = (id: number, name: string): void => {
    fullUi(ctx)?.messenger.openNote({
      name,
      send: (body) => {
        // `strlen( lpSendMessage ) <= 255`: the byte rule, else the line and no send
        if (byteLength(body) <= 255) senders.messageNote(id, body);
        else sysLine(TID_GAME_MESSFULLMSG);
      },
    });
  };

  const pickFriendMenu = (id: number, entry: number): void => {
    const row = friendRow(id);
    if (!row) return;
    switch (entry) {
      case FRIEND_MENU.MESSAGE:
        openMessage(row.name, row.id);
        break;
      case FRIEND_MENU.BLOCK:
        senders.friendInterceptState(id);
        break;
      case FRIEND_MENU.DELETE:
        senders.removeFriend(id);
        break;
      case FRIEND_MENU.MOVE:
        chatOf(ctx)?.submit(`/teleport "${row.name}"`);
        break;
      case FRIEND_MENU.INVITE_PARTY:
        partyOf(ctx)?.inviteById(id);
        break;
      case FRIEND_MENU.NOTE:
        openNoteWindow(id, row.name);
        break;
      case FRIEND_MENU.PROHIBIT_VISIT:
        visit?.set(id, false);
        break;
      case FRIEND_MENU.ALLOW_VISIT:
        visit?.set(id, true);
        break;
      default:
        break;
    }
  };

  const openNotes = (): void => {
    const ui = fullUi(ctx);
    if (!ui || ui.messenger.notes()) return;
    ui.messenger.openNotes({
      notes: store.getState().notes.map((n) => ({ from: n.from, date: n.date, text: n.text })),
    });
    // `g_Neuz.m_bCheckTag = TRUE`
    if (ui.messenger.notes()) apply(onNotesChecked(store.getState()));
  };

  const blockListSource = sourceView(
    () => blockListView(store.getState().settings, selfName(), text),
    [store, selfStore],
  );

  const openBlockList = (): void => {
    const ui = fullUi(ctx);
    if (!ui) return;
    // the window follows the settings while it is open
    ui.messenger.openBlockList({
      view: blockListSource,
    });
  };
  const actions: MessengerActions = {
    setState(state) {
      senders.setState(state);
    },
    refreshFriends() {
      // `if( g_WndMng.m_RTMessenger.size() )`
      if (store.getState().friends.size > 0) senders.getFriendState();
    },
    openAddFriend() {
      fullUi(ctx)?.messenger.openAddFriend(addFriendRequest);
    },
    openNotes,
    activate,
    friendMenu,
    pickFriendMenu,
    clearTarget() {
      targeting()?.setFocus(null);
    },
  };

  // ---- the friend summon and the rename --------------------------------------------------------------------------------------------------------

  fc.net.on('summon', (m) => {
    const ui = fullUi(ctx);
    if (!ui) return;
    if (m.sub.case === 'friend') {
      const c = m.sub.value;
      // `GetItemId( wId )`: the scroll must still be in the bag
      if (!inventoryOf(ctx)?.item(c.itemObjId)) return;
      ui.messenger.openSummon({
        friends: summonCandidates(view.get().friends),
        submit: (name) => {
          const still = (inventoryOf(ctx)?.item(c.itemObjId) ?? null) !== null;
          const outcome = summonOutcome(name, selfName(), still);
          switch (outcome.kind) {
            case 'keep':
              return false;
            case 'self':
              sysLine(TID_GAME_SUMMON_FRIEND_MY_NOUSE);
              return true;
            case 'noItem':
              sysLine(TID_ERROR_SUMMONFRIEND_NOITEM, [
                ctx.data?.item(II_SYS_SYS_SCR_FRIENDSUMMON_A)?.name ?? '',
                name,
              ]);
              return true;
            case 'send':
              // the word is echoed unchanged: the scroll's object id is in its LOW word
              senders.summonFriend(c.data, outcome.name);
              return true;
          }
        },
      });
    } else if (m.sub.case === 'friendConfirm') {
      const c = m.sub.value;
      ui.messenger.openSummonMsg({
        text: formatTid(fc.texts, TID_DIAG_SUMMONFRIEND_CONFIRM, [c.summonerName, c.worldName]),
        ok: () => {
          senders.summonFriendConfirm(c.summonerObj, c.data);
        },
        cancel: () => {
          senders.summonFriendCancel(c.summonerObj, c.data);
        },
      });
    }
  });

  fc.net.on('querySetPlayerNameNtf', (m) => {
    const ui = fullUi(ctx);
    const id = m.idPlayer;
    const name = m.name;
    // the IM window and the popup of that player follow the new name (`UpdateMessage`)
    const session = ims.byId(id);
    if (session) {
      const old = session.partner;
      if (ims.rename(old, name)) ui?.messenger.renameMessage(old, name);
    }
    for (const [key, meta] of [...popups]) {
      if (meta.id !== id || key === name) continue;
      if (ui?.messenger.renameInstant(key, name) === true) {
        popups.delete(key);
        meta.name = name;
        popups.set(name, meta);
      }
    }
    // the own character: the name of the stores; every mover in view: the nameplate
    if (id === selfId()) selfStore.setState({ name });
    for (const rec of playerRecords(id)) {
      if (rec.visual) rec.visual.name = name;
      if (rec.isSelf) sysLine(TID_GAME_CHANGENAME);
      // `CreateSfx( XI_SYS_EXCHAN01 )` on the mover
      const fx = fxRuntime(ctx).fx();
      if (fx) {
        const pos = info(rec.objId)?.pos;
        fx.sfx(XI_SYS_EXCHAN01, { src: rec.objId, dest: rec.objId, ...(pos ? { pos } : {}) });
      }
    }
    // the player is not in view but is the own character (the world is being entered)
    if (id === selfId() && playerRecords(id).length === 0) sysLine(TID_GAME_CHANGENAME);
  });

  // ---- the OBJECT shortcut ------------------------------------------------------------------------------------------------------------------------

  /** `ObjectExecutor( SHORTCUT_OBJECT )`: the IM window of the mover with that object id, only while it is in view (ids are per session). */
  const openMessageOfObject = (objId: number): void => {
    const rec = ctx.entities.get(objId);
    if (!valid(rec)) return;
    const name = rec.visual?.name ?? '';
    if (name === '') return;
    openMessage(name, characterOf(rec));
  };
  taskbarOf(ctx)?.setShortcutPorts({ messenger: { openMessage: openMessageOfObject } });

  // ---- sends of the other features and small API ----------------------------------------------------------------------------------------------------

  const sendBlock = (gu: number, name: string): void => {
    senders.block(gu, name);
  };

  // ---- the user is back: SetMessengerAutoState ----------------------------------------------------------------------------------------------------------

  let nextAuto = 0;
  const onUserInput = (): void => {
    const s = store.getState();
    if (!s.connected || s.myState !== FRS.AUTOABSENT) return;
    const now = ctx.now();
    if (now < nextAuto) return;
    nextAuto = now + AUTO_STATE_MS;
    senders.setState(FRS.ONLINE);
  };
  if (typeof document !== 'undefined') {
    document.addEventListener('keydown', onUserInput, { passive: true });
    document.addEventListener('pointerdown', onUserInput, { passive: true });
  }

  // ---- the windows ------------------------------------------------------------------------------------------------------------------------------------

  const installed = new WeakSet<object>();
  const ensureUi = (): void => {
    const ui = fullUi(ctx);
    if (!ui || installed.has(ui)) return;
    installed.add(ui);
    const ports: MessengerPorts = { view, actions };
    ui.messenger.setPorts({ messenger: ports });
  };
  ctx.tickSystems.register('messenger.ui', ensureUi);
  ctx.machine.subscribe(ensureUi);
  ensureUi();

  // the conversations, popups and notes of a character that left the world (logout, kick; not a world change, which keeps the notes the server will not send again) go
  // with it: the next join replaces the friends and the settings, but a `Tag` is only sent while notes are undelivered
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    popups.clear();
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    const ui = fullUi(ctx);
    for (const key of ui?.messenger.instantKeys() ?? []) ui?.messenger.closeInstant(key);
    for (const session of ims.all()) closeMessage(session.partner);
    ui?.messenger.closeFriendConfirm();
    store.setState(initialMessengerState());
  });

  const feature: MessengerFeature = {
    store,
    senders,
    playerData,
    view,
    ims,
    openMessage,
    closeMessage,
    sendBlock,
    popup,
    openNotes,
    openBlockList,
    setGuildRoster(source) {
      roster = source;
      view.invalidate();
      source?.subscribe(() => {
        view.invalidate();
      });
    },
    setVisitPorts(ports) {
      visit = ports;
      view.invalidate();
      ports?.subscribe(() => {
        view.invalidate();
      });
    },
    setCampusPoints(read) {
      campusPoints = read;
      view.invalidate();
    },
    isFriend: (id) => store.getState().friends.has(id),
  };
  setMessengerOf(ctx, feature);

  // ---- the debug probe --------------------------------------------------------------------------------------------------------------------------------

  registerProbe(ctx, feature, view, popups);
  return feature;
}
