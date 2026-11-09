// The reducers of the shop messages (Plan/45 §15; `CDPClient::OnPVendor*`, `OnRegisterPVendorItem`, `OnChatting`, `src:Neuz/DPClient.cpp:13873-14140, 15152-15260`):
// pure functions of the state and the message. The module turns the difference between two states into windows, reservations and sends.

import {
  emptyChat,
  MAX_VENDITEM,
  type Browse,
  type BrowseGood,
  type ChatRoom,
  type OwnGood,
  type SavedGood,
  type VendorState,
} from './store';

const bump = (s: VendorState, patch: Partial<VendorState>): VendorState => ({
  ...s,
  ...patch,
  version: s.version + 1,
});

const setAt = <T>(list: readonly (T | null)[], i: number, v: T | null): (T | null)[] => {
  const out = list.slice();
  out[i] = v;
  return out;
};

const inRange = (slot: number): boolean =>
  Number.isInteger(slot) && slot >= 0 && slot < MAX_VENDITEM;

// ---- the own shop ---------------------------------------------------------------------------------------------------------------

/** `OnRegisterPVendorItem` (self): the good enters the slot (`VendorSetItem`). */
export function onRegister(
  s: VendorState,
  slot: number,
  good: { objId: number; num: number; cost: number },
): VendorState {
  if (!inRange(slot)) return s;
  const g: OwnGood = { objId: good.objId, count: good.num, price: good.cost };
  return bump(s, { own: setAt(s.own, slot, g) });
}

/** `OnUnregisterPVendorItem` (self): the slot is cleared and the saved good with it. */
export function onUnregister(s: VendorState, slot: number): VendorState {
  if (!inRange(slot)) return s;
  return bump(s, { own: setAt(s.own, slot, null), saved: setAt(s.saved, slot, null) });
}

/** `OnPVendorOpen` (self): the shop is open with this title. */
export function onOpenSelf(s: VendorState, title: string): VendorState {
  return bump(s, { open: true, title });
}

/** `VendorClose( clearTitle )` of the own shop: the goods and the title go, the saved list stays for the next opening. */
export function onCloseSelf(s: VendorState): VendorState {
  return bump(s, { open: false, own: s.own.map(() => null), stopping: false });
}

/** The window is destroyed: the goods are kept for `ReloadItemList` (`OnDestroy` of `CWndVendor`). */
export function saveGoods(s: VendorState, itemIdOf: (objId: number) => number): VendorState {
  const saved: (SavedGood | null)[] = s.own.map((g) =>
    g ? { objId: g.objId, itemId: itemIdOf(g.objId), count: g.count, price: g.price } : null,
  );
  return bump(s, { saved });
}

/** `ReloadItemList`: a saved good that does not fit its item any more (the item is gone or smaller) is forgotten. */
export function forgetSaved(s: VendorState, slot: number): VendorState {
  if (!inRange(slot) || !s.saved[slot]) return s;
  return bump(s, { saved: setAt(s.saved, slot, null) });
}

/** `Reset`: every saved good is forgotten. */
export function clearSaved(s: VendorState): VendorState {
  return bump(s, { saved: s.saved.map(() => null) });
}

/** The "stop selling" answer was yes. */
export function setStopping(s: VendorState, stopping: boolean): VendorState {
  return s.stopping === stopping ? s : bump(s, { stopping });
}

/**
 * `OnPVendorItemNum`: `remaining` is what is still for sale after a purchase. For the OWN shop (`own`) the slot's count follows (0 clears it) and
 * the saved good is forgotten (`m_aSavedInven[nItem] = 0`); for a visited shop the slot of the list follows.
 */
export function onItemNum(
  s: VendorState,
  own: boolean,
  slot: number,
  remaining: number,
): VendorState {
  if (!inRange(slot)) return s;
  const saved = setAt(s.saved, slot, null);
  if (own) {
    const g = s.own[slot];
    if (!g) return bump(s, { saved });
    return bump(s, {
      saved,
      own: setAt(s.own, slot, remaining > 0 ? { ...g, count: remaining } : null),
    });
  }
  const b = s.browse;
  if (!b) return bump(s, { saved });
  const g = b.goods[slot];
  if (!g) return bump(s, { saved });
  const goods = setAt(b.goods, slot, remaining > 0 ? { ...g, count: remaining } : null);
  return bump(s, { saved, browse: { ...b, goods } });
}

// ---- a visited shop ---------------------------------------------------------------------------------------------------------------

/** `OnPVendorItem`: the shop opens in visitor mode with its goods. */
export function onBrowse(
  s: VendorState,
  browse: {
    vendor: number;
    vendorId: number;
    title: string;
    goods: (BrowseGood | null)[];
    chatOpen: boolean;
  },
): VendorState {
  const b: Browse = {
    vendor: browse.vendor,
    vendorId: browse.vendorId,
    title: browse.title,
    goods: browse.goods,
  };
  return bump(s, { browse: b, chat: { ...emptyChat(), open: browse.chatOpen } });
}

/** The visited shop is gone (the window closed or `PVendorCloseNtf` said so). */
export function endBrowse(s: VendorState): VendorState {
  if (!s.browse && s.chat.members.length === 0 && s.chat.talk.length === 0) return s;
  return bump(s, { browse: null, chat: emptyChat() });
}

// ---- the chat room ----------------------------------------------------------------------------------------------------------------

const setChat = (s: VendorState, patch: Partial<ChatRoom>): VendorState =>
  bump(s, { chat: { ...s.chat, ...patch } });

/** `CHATTING_NEWCHATTING`: the member list (all members, the receiver included). */
export function onNewChatting(
  s: VendorState,
  members: readonly { id: number; name: string }[],
): VendorState {
  return setChat(s, { members: members.map((m) => ({ id: m.id, name: m.name })) });
}

/** `CHATTING_ENTERMEMBER`: a member joined; the owner's info page gets `info` (the `TID_GAME_VENCHATIN` line) when given. */
export function onEnterMember(
  s: VendorState,
  member: { id: number; name: string },
  info: string | null,
): VendorState {
  const members = s.chat.members.some((m) => m.id === member.id)
    ? s.chat.members
    : [...s.chat.members, { id: member.id, name: member.name }];
  return setChat(s, { members, info: info === null ? s.chat.info : [...s.chat.info, info] });
}

/** `CHATTING_REMOVEMEMBER`: a member left; the owner's info page gets the `TID_GAME_VENCHATOUT` line when given. */
export function onRemoveMember(s: VendorState, id: number, info: string | null): VendorState {
  return setChat(s, {
    members: s.chat.members.filter((m) => m.id !== id),
    info: info === null ? s.chat.info : [...s.chat.info, info],
  });
}

/** `CHATTING_DELETECHATTING`: the room is gone. */
export function onDeleteChatting(s: VendorState): VendorState {
  return setChat(s, { members: [] });
}

/** `CHATTING_CHATTING`: one line on the talk page. */
export function onTalk(s: VendorState, line: string): VendorState {
  return setChat(s, { talk: [...s.chat.talk, line] });
}

/** An info line of the owner (a sale). */
export function onInfo(s: VendorState, line: string): VendorState {
  return setChat(s, { info: [...s.chat.info, line] });
}

/** `CHATTING_CHATTINGROOMSTATE`: the room opened or closed (`g_Chatting.m_bState`). */
export function onRoomState(s: VendorState, open: boolean): VendorState {
  return s.chat.open === open ? s : setChat(s, { open });
}

/** The chat is over (the shop closed, the window left): members and lines are dropped. */
export function endChat(s: VendorState): VendorState {
  const c = s.chat;
  if (c.members.length === 0 && c.talk.length === 0 && c.info.length === 0 && !c.open) return s;
  return bump(s, { chat: emptyChat() });
}

/** The state after the shop and everything around it ended (world change, logout). */
export function resetAll(s: VendorState): VendorState {
  return bump(s, {
    title: '',
    open: false,
    own: s.own.map(() => null),
    browse: null,
    stopping: false,
    chat: emptyChat(),
  });
}
