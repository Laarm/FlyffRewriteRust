// The public face of the messenger feature (`messengerOf(ctx)`), in a module of its own so that the features that only SEND a block reply (trade, party) or open a popup
// (mail) can import it without importing the messenger's module (which imports the party feature: a cycle otherwise).

import type { ClientContext } from '../../app/context';
import type { PlayerDataApi } from '../common/playerData';
import type { ViewSource, MessengerView } from '../../ui/windows/messenger/ports';
import type { ImRegistry, ImSession } from './im';
import type { MessengerSenders } from './senders';
import type { MessengerStore } from './store';

/** `{ key, text, ttlMs, onClick? }`: the popup service (`CWndMgr::OpenInstantMsg` + `AddPostMessage`); M7 adds the "Event" popup through it. */
export interface PopupRequest {
  /** `Mail`, `Event` or a name: one popup per key, a second request adds a line and starts the timer again. */
  readonly key: string;
  /** The text, shown as it is. */
  readonly text: string;
  readonly ttlMs: number;
  /** A click on the popup: `true` closes it. Without a handler a click does nothing. */
  readonly onClick?: () => boolean;
}

/** The roster of the own guild: `ids` in display order; ui-guild repoints it to its `guildStore`. */
export interface GuildRosterSource {
  ids(): readonly number[];
  subscribe(listener: () => void): () => void;
}

/** Housing (M7): who may visit my house. Absent = no housing, no menu entry and no green names. */
export interface VisitPorts {
  allowed(idPlayer: number): boolean;
  set(idPlayer: number, allow: boolean): void;
  subscribe(listener: () => void): () => void;
}

export interface MessengerFeature {
  readonly store: MessengerStore;
  readonly senders: MessengerSenders;
  readonly playerData: PlayerDataApi;
  /** The read model of the messenger window. */
  readonly view: ViewSource<MessengerView>;
  /** The open IM conversations. */
  readonly ims: ImRegistry;
  /** `CWndMgr::OpenMessage`: the IM window of a partner (opened, or focused when it is open); the session of its window. */
  openMessage(name: string, idPlayer?: number): ImSession | null;
  /** The IM window of a partner goes (`BlockNtf{ 1 }`: the partner refuses my messages). */
  closeMessage(name: string): void;
  /** `SendBlock( gu, name )`: the only `BlockReq` sender (4 repair, 5 trade busy, 6 friend window open, 7 party window open, 8 guild bank / angel window). */
  sendBlock(gu: number, name: string): void;
  /** The popup service. */
  popup(request: PopupRequest): void;
  /** `APP_MESSENGER_NOTE`. */
  openNotes(): void;
  /** `/ignorelist`: `APP_CHATTING_BLOCKING_LIST`. */
  openBlockList(): void;
  /** Repoints the guild page to another roster (ui-guild); `null` goes back to the messenger's own. */
  setGuildRoster(source: GuildRosterSource | null): void;
  /** Housing ports of M7. */
  setVisitPorts(ports: VisitPorts | null): void;
  /** The campus points of the header (`g_pPlayer->GetCampusPoint()`, ui-couple-honor). */
  setCampusPoints(read: (() => number) | null): void;
  isFriend(idPlayer: number): boolean;
}

const features = new WeakMap<ClientContext, MessengerFeature>();

/** The messenger feature of a context; `undefined` before the module registered. */
export function messengerOf(ctx: ClientContext): MessengerFeature | undefined {
  return features.get(ctx);
}

/** Called by the module once it exists. */
export function setMessengerOf(ctx: ClientContext, feature: MessengerFeature): void {
  features.set(ctx, feature);
}
