// The `mail` feature (Plan/45 §16, Plan/19 §11; WP-CFEAT-23): the post window and everything around it. The post NPC (`MMI_POST`) opens the inventory and the post
// window; the window asks for the mailbox (`QueryMailBoxReq`, at most one per 3 s like the server) and waits behind the requesting box for `QueryMailBoxNtf`,
// which fills the inbox. A double click on a mail opens the reader and tells the server it was read (`ReadMail`); the item is taken by dragging it onto the
// inventory or with a right click (after the custody fee warning once the free 7 days are over), the penya with a right click; Delete asks first
// (`QueryRemoveMail`). `RemoveMail` answers each of these (0 gone, 1 item taken, 2 penya taken, 3 read). The send tab attaches an inventory item (the
// stack asks for the amount; the item is reserved in the bag meanwhile) and sends `QueryPostMail` after the confirmation; the server answers with a text
// ("You have sent mail." as a message box, or the refusal). `MODE_MAILBOX` newly set on the own character (a mail arrived, or one waits at the join) prints
// "New mail has arrived" in the chat and asks for the mailbox again when the post window is open (`PostMail` is not pushed, Plan/19 §11.2).
// The messenger's instant-message window `APP_INSTANTMSG` "Mail" of the original (WP-UI-44, M6) does not exist yet.
//
// Other features reach it with `mailOf(ctx)`.

import type { ClientContext } from '../../app/context';
import {
  APP_POST,
  APP_POST_READ,
  APP_REQUESTING_POST,
  APP_TRADE_GOLD,
} from '../../gen/defs/resdata';
import { MMI_POST } from '../../gen/defs/neuz';
import {
  TID_GAME_CANNOT_POST,
  TID_GAME_CHARGED_NOTUSE,
  TID_GAME_EQUIPPUT,
  TID_GAME_MAILBOX_KEEP_PAY,
  TID_GAME_MOVECOUNT,
  TID_GAME_NUMCOUNT,
  TID_MAIL_RECEIVE,
} from '../../gen/defs/text';
import { wallNow } from '../../platform/time';
import { SC, type Shortcut } from '../../ui/dnd/shortcut';
import {
  MAX_KEEP_BASIC_DAY,
  MAX_MAILTITLE,
  type MailDraft,
  type MailRow,
  type MailSendForm,
  type MailView,
  type PostActions,
  type ReadActions,
  type SendPorts,
} from '../../ui/windows/mail/ports';
import { cutText, goldOf } from '../../ui/windows/mail/form';
import { rulesRuntime } from '../../game/rules';
import { type FeatureModule, fullUi } from '../common/module';
import { playerDataOf } from '../common/playerData';
import { addProbeMember } from '../common/probe';
import { formatTid, sysChat } from '../common/text';
import { derivedView, localState, type ViewSource } from '../common/view';
import { isBound, isCharged, isGuildCloak, isQuestItem, isReserved } from '../inventory/info';
import { inventoryOf } from '../inventory/window/inventoryModule';
import { messengerOf } from '../messenger/api';
import { menuOf } from '../npc/menuModule';
import { postBusy } from './busy';
import { createMailSenders, NO_ATTACHMENT, type MailSenders } from './senders';
import {
  applyBox,
  applyPost,
  applyRemove,
  defineMailStore,
  type MailEntry,
  type MailState,
  type MailStore,
} from './state';
import { MailViewBuilder } from './view';
import { ITEM_FLAG, type InvItem } from '../inventory/items';

/** `MODE_MAILBOX` of the mode word (`authorization.h`). */
export const MODE_MAILBOX = 0x8000;
/** `m_timer.Set( MIN( 1 ) )`: the "Mail" popup stays one minute. */
const MAIL_POPUP_TTL_MS = 60_000;
/** `MAILBOX_QUERY_MIN_INTERVAL_MS`: the server answers at most one `QueryMailBoxReq` per 3 s. */
export const QUERY_INTERVAL_MS = 3000;

/** The public face of the mail feature of a context. */
export interface MailFeature {
  readonly store: MailStore;
  readonly view: ViewSource<MailView>;
  readonly senders: MailSenders;
  /** The post window's actions (the throttled box query, the reader). */
  readonly actions: PostActions;
  readonly readActions: ReadActions;
  /** The ports of the send tab. */
  readonly send: SendPorts;
  state(): MailState;
  /** `MMI_POST` on a post NPC: the busy checks, then the inventory and the post window. */
  requestOpen(): boolean;
  /** Closes every mail window (a transactional close). */
  close(): void;
  /** The post window is open. */
  isOpen(): boolean;
  /** The mail being written. */
  draft(): { readonly objId: number | null; readonly count: number };
}

const features = new WeakMap<ClientContext, MailFeature>();

/** The mail feature of a context; `undefined` before the module registered. */
export function mailOf(ctx: ClientContext): MailFeature | undefined {
  return features.get(ctx);
}

export const mailModule: FeatureModule = {
  id: 'mail',
  milestone: 4,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const store = defineMailStore(ctx.stores.registry);
    const senders = createMailSenders(ctx);
    const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
    const say = (tid: number, args: readonly string[] = []): void => {
      sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, args);
    };
    const stats = { opened: 0, queries: 0, throttled: 0, read: 0, sent: 0, notices: 0 };
    const inv = () => inventoryOf(ctx);
    const prop = (id: number) => ctx.data?.item(id);

    // ---- the view ---------------------------------------------------------------------------------------------------------

    const nowSec = (): number => Math.floor(wallNow() / 1000);
    const builder = new MailViewBuilder({
      data: () => ctx.data,
      text,
      nowSec,
      senderName: (id) => playerDataOf(ctx).nameOf(id),
      fee: (item, elapsed) => {
        const r = rulesRuntime(ctx).rules();
        if (!r) return 0;
        // `GetCost()`: the unit price of the rules is the item table's cost raised by the upgrade level
        const cost = r.buyUnitPrice({
          itemId: item.itemId,
          abilityOption: item.abilityOption,
        }).unit;
        return r.mailStorageFee(cost, elapsed);
      },
      friends: () => [],
      newFontColor: () => ctx.features.features.new_font_color,
      formatDate: (sec) => new Date(sec * 1000).toLocaleString(),
    });
    const dataSource = {
      subscribe: (fn: () => void) =>
        ctx.data?.onBundle(() => {
          fn();
        }) ?? (() => undefined),
    };
    const view = derivedView(
      () => builder.build(store.getState()),
      [store, playerDataOf(ctx).store, dataSource],
      (a, b) => a === b,
    );

    // ---- the mailbox query ------------------------------------------------------------------------------------------------

    let lastQueryMs = Number.NEGATIVE_INFINITY;
    let pendingQuery = false;
    const sendQuery = (): void => {
      lastQueryMs = ctx.now();
      pendingQuery = false;
      stats.queries += 1;
      senders.queryBox();
    };
    /** At most one query per 3 s; a request inside the window is sent when it is over. */
    const queryBox = (): void => {
      if (ctx.now() - lastQueryMs >= QUERY_INTERVAL_MS) sendQuery();
      else {
        stats.throttled += 1;
        pendingQuery = true;
      }
    };
    ctx.tickSystems.register('mail.query', () => {
      if (pendingQuery && ctx.now() - lastQueryMs >= QUERY_INTERVAL_MS) sendQuery();
    });

    // ---- the draft (the send tab) ----------------------------------------------------------------------------------------

    const draftState = localState<{ objId: number | null; count: number }>({
      objId: null,
      count: 0,
    });
    const releaseDraft = (): void => {
      const d = draftState.get();
      if (d.objId !== null) inv()?.setExtra(d.objId, 0);
      draftState.set({ objId: null, count: 0 });
    };
    const draftView: ViewSource<MailDraft> = derivedView(
      () => {
        const d = draftState.get();
        const it = d.objId === null ? null : (inv()?.item(d.objId) ?? null);
        const view = it ? inv()?.itemView(it) : null;
        return it && view ? { item: view, count: d.count } : { item: null, count: 0 };
      },
      [draftState, ...(inv() ? [inv()?.store] : [])].filter(
        (x): x is NonNullable<typeof x> => x !== undefined,
      ),
      (a, b) => a.item === b.item && a.count === b.count,
    );

    /** The quantity box for the amount of a stack (`TID_GAME_MOVECOUNT` / `TID_GAME_NUMCOUNT`). */
    const askCount = (max: number, then: (n: number) => void, cancel: () => void): void => {
      fullUi(ctx)?.msgbox.quantity(max, then, undefined, cancel, {
        prompt: text(TID_GAME_MOVECOUNT),
        label: text(TID_GAME_NUMCOUNT),
      });
    };

    /** Why the original or the server would refuse an item as an attachment, as the text id; `0` = fine. */
    const refusal = (it: InvItem, worn: boolean): number => {
      if (worn) return TID_GAME_EQUIPPUT;
      if (isCharged(it)) return TID_GAME_CHARGED_NOTUSE;
      const p = prop(it.itemId);
      // the item of the draft itself is reserved by this window: that is no reason to refuse it again
      const reserved = isReserved(it) && draftState.get().objId !== it.objId;
      if (
        isQuestItem(p) ||
        isBound(it) ||
        (it.flag & ITEM_FLAG.ISUSING) !== 0 ||
        isGuildCloak(it, p) ||
        reserved
      ) {
        return TID_GAME_CANNOT_POST;
      }
      return 0;
    };

    const send: SendPorts = {
      view,
      draft: draftView,
      attach(sc: Shortcut) {
        if (sc.kind !== SC.ITEM || sc.data === 0) return null;
        const i = inv();
        const it = i?.item(sc.id) ?? null;
        if (!i || !it) return null;
        const why = refusal(it, sc.index === 0xffff_ffff || i.state().container.isEquip(it.objId));
        if (why !== 0) {
          say(why);
          return null;
        }
        // an item that was attached before gives its place up
        releaseDraft();
        const objId = it.objId;
        const name = prop(it.itemId)?.name ?? '';
        if (it.num > 1) {
          // the stack asks for the amount; until it is answered the item is reserved as a whole (Cancel gives it back)
          i.setExtra(objId, it.num);
          draftState.set({ objId, count: it.num });
          askCount(
            it.num,
            (n) => {
              const cur = inv()?.item(objId);
              if (!cur) return;
              const count = Math.max(1, Math.min(n, cur.num));
              inv()?.setExtra(objId, count);
              draftState.set({ objId, count });
            },
            () => {
              if (draftState.get().objId === objId) releaseDraft();
            },
          );
        } else {
          i.setExtra(objId, 1);
          draftState.set({ objId, count: 1 });
        }
        return { name };
      },
      detach() {
        releaseDraft();
      },
      send(form: MailSendForm) {
        const d = draftState.get();
        const item = d.objId === null ? null : (inv()?.item(d.objId) ?? null);
        stats.sent += 1;
        senders.postMail({
          objId: item ? item.objId : NO_ATTACHMENT,
          num: item ? d.count : 0,
          receiver: form.receiver,
          gold: goldOf(form),
          title: form.title,
          text: form.text,
        });
        // `ClearData`: the item is released, the fields keep their text
        releaseDraft();
      },
    };

    // ---- the reader ---------------------------------------------------------------------------------------------------------

    const entryOf = (id: number): MailEntry | null =>
      store.getState().box.find((e) => e.id === id) ?? null;
    const rowOf = (id: number): MailRow | null => view.get().rows.find((r) => r.id === id) ?? null;

    const readActions: ReadActions = {
      takeItem(id) {
        const e = entryOf(id);
        const r = rowOf(id);
        if (!e?.item || !r) return;
        if (r.custody) {
          // the free keep time is over: the custody fee is announced first (`CWndPostItemWarning`)
          const textWarn = formatTid(fc.texts, TID_GAME_MAILBOX_KEEP_PAY, [
            MAX_KEEP_BASIC_DAY,
            r.fee,
          ]);
          fullUi(ctx)?.dialogs.openQueItemWarning({
            text: textWarn,
            yes: () => {
              senders.getItem(id);
            },
          });
          return;
        }
        senders.getItem(id);
      },
      takeGold(id) {
        const e = entryOf(id);
        if (e && e.gold > 0) senders.getGold(id);
      },
      confirmDelete(id) {
        fullUi(ctx)?.mail.openDeleteConfirm({
          yes: () => {
            senders.removeMail(id);
            fullUi(ctx)?.wm.get(APP_POST_READ)?.destroy();
          },
        });
      },
      reply(id) {
        const ui = fullUi(ctx);
        const r = rowOf(id);
        if (!ui || !r) return;
        // FIX: the whole title (the original took the cut one with its dots), clipped to what the server accepts
        const title = cutText(`Re:${r.title}`, MAX_MAILTITLE - 1, 'over').replace(/\.\.\.$/, '');
        ui.mail.post?.showSend({ receiver: r.sender, title, text: '' });
        ui.wm.get(APP_POST_READ)?.destroy();
      },
      closed() {
        // the reader goes away: nothing is reserved by it
      },
    };

    const openMail = (id: number): void => {
      const ui = fullUi(ctx);
      const e = entryOf(id);
      if (!ui || !e) return;
      if (!e.read) {
        stats.read += 1;
        senders.readMail(id);
      }
      ui.mail.openRead({ mail: () => rowOf(id), view, actions: readActions });
    };

    // ---- the post window ----------------------------------------------------------------------------------------------------

    const actions: PostActions = {
      queryBox,
      openMail,
      closed() {
        // `~CWndPostSend` -> `ClearData`; the reader and the delete question belong to the window
        releaseDraft();
        pendingQuery = false;
        const ui = fullUi(ctx);
        ui?.wm.get(APP_POST_READ)?.destroy();
      },
    };

    const requestOpen = (): boolean => {
      const ui = fullUi(ctx);
      if (!ui) return false;
      const busy = postBusy((app) => ui.wm.isOpen(app));
      if (busy !== null) {
        say(busy);
        return false;
      }
      stats.opened += 1;
      // `CreateApplet( APP_INVENTORY )`, then a fresh post window
      ui.char.openInventory();
      ui.mail.openPost({ actions, view, send });
      return true;
    };

    const menu = menuOf(ctx);
    if (!menu)
      throw new Error('the mail feature needs the npc menu table (register the npc module first)');
    menu.registerAction(MMI_POST, ({ npcKey }) => {
      if (npcKey !== null) requestOpen();
    });

    // an item or the penya dragged from the reader onto the inventory
    let unregisterDrop: (() => void) | null = null;
    const hookInventory = (): void => {
      const i = inv();
      if (!i || unregisterDrop) return;
      unregisterDrop = i.registerDropIn(APP_POST_READ, (sc) => {
        const reader = fullUi(ctx)?.mail.reader;
        const id = reader?.mailId ?? null;
        if (id === null || sc.kind !== SC.ITEM) return false;
        if (sc.data !== 0) readActions.takeItem(id);
        else readActions.takeGold(id);
        return true;
      });
    };
    ctx.tickSystems.register('mail.inventory', hookInventory);
    hookInventory();

    // ---- the handlers ---------------------------------------------------------------------------------------------------------

    fc.net.on('queryMailBoxNtf', (m) => {
      store.setState(applyBox(store.getState(), m.mails));
      // the senders' names (`CPlayerDataCenter::GetPlayerString`), one bulk query
      const ids = [...new Set(m.mails.map((x) => x.idSender).filter((id) => id !== 0))];
      if (ids.length > 0) playerDataOf(ctx).needAll(ids);
      fullUi(ctx)?.mail.mailboxArrived();
    });
    fc.net.on('removeMail', (m) => {
      store.setState(applyRemove(store.getState(), m.mailId, m.kind));
    });
    fc.net.on('postMail', (m) => {
      store.setState(applyPost(store.getState(), m.mail));
    });

    // `MODE_MAILBOX` newly set on the own character
    selfStore.subscribe((s, prev) => {
      if ((s.modeFlags & MODE_MAILBOX) === 0 || (prev.modeFlags & MODE_MAILBOX) !== 0) return;
      stats.notices += 1;
      say(TID_MAIL_RECEIVE);
      // the "Mail" popup of the messenger (`OpenInstantMsg( "Mail" )`, kept one minute; a click does nothing)
      messengerOf(ctx)?.popup({
        key: 'Mail',
        text: text(TID_MAIL_RECEIVE),
        ttlMs: MAIL_POPUP_TTL_MS,
      });
      // `PostMail` is not pushed: a window that is open asks for the box again
      if (fullUi(ctx)?.wm.isOpen(APP_POST) === true) queryBox();
    });

    // ---- transactional close ---------------------------------------------------------------------------------------------------

    const close = (): void => {
      const ui = fullUi(ctx);
      ui?.mail.closeAll();
      releaseDraft();
      if (ui) ui.wm.get(APP_TRADE_GOLD, 'quantity')?.destroy();
    };
    fc.transactional.register({ id: 'mail', reasons: 'all', close });

    const api: MailFeature = {
      store,
      view,
      senders,
      actions,
      readActions,
      send,
      state: () => store.getState(),
      requestOpen,
      close,
      isOpen: () => fullUi(ctx)?.wm.isOpen(APP_POST) ?? false,
      draft: () => draftState.get(),
    };
    features.set(ctx, api);

    addProbeMember(ctx, 'mail', () => {
      const s = store.getState();
      const ui = fullUi(ctx);
      return {
        open: api.isOpen(),
        requesting: ui?.wm.isOpen(APP_REQUESTING_POST) ?? false,
        readerOpen: ui?.wm.isOpen(APP_POST_READ) ?? false,
        ready: s.ready,
        box: s.box.map((e) => ({
          id: e.id,
          senderId: e.senderId,
          title: e.title,
          gold: e.gold,
          itemId: e.item?.itemId ?? 0,
          itemNum: e.item?.num ?? 0,
          read: e.read,
        })),
        draft: { ...draftState.get() },
        lastQueryMs: Number.isFinite(lastQueryMs) ? lastQueryMs : null,
        stats: { ...stats },
      };
    });
  },
};
