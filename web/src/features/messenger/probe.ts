// The debug probe of the messenger (`window.__flyffDebug.features.messenger()` and `messengerPoints()`, Plan/45 §2.6; WP-CFEAT-39): what the Playwright specs read instead of pixels - the
// state, the rows, the settings, the notes, the open windows, the conversations and the popups - and the CSS pixel centres of the controls the specs click.

import type { ClientContext } from '../../app/context';
import { APP_MESSENGER_EX } from '../../gen/defs/resdata';
import { fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import type { MessengerFeature } from './api';
import type { MessengerView, ViewSource } from '../../ui/windows/messenger/ports';
import { TAB, type WndMessenger } from '../../ui/windows/messenger/WndMessenger';
import { COL, ROW_PITCH, ROWS_TOP } from '../../ui/windows/messenger/icons';
import type { Rect } from '../../ui/layout/geometry';

interface Point {
  x: number;
  y: number;
}

/** The CSS pixel centre of a rect of root (virtual UI) coordinates. */
function cssCenter(ctx: ClientContext, r: Rect): Point | null {
  const ui = fullUi(ctx);
  if (!ui || typeof document === 'undefined') return null;
  const scale = ui.scaler.get().scale;
  const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
  return {
    x: Math.round((root?.left ?? 0) + ((r.l + r.r) / 2) * scale),
    y: Math.round((root?.top ?? 0) + ((r.t + r.b) / 2) * scale),
  };
}

export function registerProbe(
  ctx: ClientContext,
  feature: MessengerFeature,
  view: ViewSource<MessengerView>,
  popups: ReadonlyMap<string, { name: string; id: number | null; last: string | null }>,
): void {
  addProbeMember(ctx, 'messenger', () => {
    const s = feature.store.getState();
    const ui = fullUi(ctx);
    const v = view.get();
    const m = ui?.messenger;
    const w = m?.messenger() ?? null;
    return {
      connected: s.connected,
      myState: s.myState,
      settings: s.settings,
      tagChecked: s.tagChecked,
      notes: s.notes,
      guildId: s.guildId,
      presence: [...s.presence.entries()],
      friends: v.friends.map((r) => ({
        id: r.id,
        name: r.name,
        state: r.state,
        blocked: r.blocked,
        level: r.level,
        job: r.job,
        channel: r.channel,
      })),
      guild: v.guild.map((r) => ({
        id: r.id,
        name: r.name,
        state: r.state,
        level: r.level,
        job: r.job,
        channel: r.channel,
      })),
      view: v,
      messengerOpen: ui?.wm.isOpen(APP_MESSENGER_EX) ?? false,
      tab: w?.tabIndex ?? null,
      friendOrder: w?.friendPage?.rows.map((r) => r.name) ?? [],
      confirmOpen: m?.isFriendConfirmOpen() ?? false,
      addFriendOpen: m?.addFriend() != null,
      noteOpen: m?.note() != null,
      notesOpen: m?.notes() != null,
      blockListOpen: m?.blockList() != null,
      summonOpen: m?.summon() != null,
      summonMsgOpen: m?.summonMsg() != null,
      ims: feature.ims.all().map((x) => {
        const win = m?.message(x.partner) ?? null;
        return {
          partner: x.partner,
          partnerId: x.partnerId,
          blocked: x.blocked,
          lines: x.lines.map((l) => ({ mine: l.mine, from: l.from, text: l.text })),
          title: win?.getTitle() ?? null,
          open: win !== null,
          enabled: win?.enabledInput ?? false,
        };
      }),
      popups: (m?.instantKeys() ?? []).map((key) => ({
        key,
        text: m?.instant(key)?.plainText ?? '',
        endsAt: m?.instant(key)?.endsAt ?? 0,
        meta: popups.get(key) ?? null,
      })),
      blockList: m?.blockList()?.names ?? [],
      menu: w?.friendMenu?.isVisible() === true ? w.friendMenu.items.map((i) => i.getTitle()) : [],
    };
  });

  addProbeMember(ctx, 'messengerPoints', () => {
    const ui = fullUi(ctx);
    const m = ui?.messenger;
    if (!ui || !m) return null;
    const w: WndMessenger | null = m.messenger();
    const ctl = (c: { getScreenRect(): Rect } | null | undefined): Point | null =>
      c ? cssCenter(ctx, c.getScreenRect()) : null;
    const rowsOf = (page: 'friend' | 'guild') => {
      const p = page === 'friend' ? w?.friendPage : w?.guildPage;
      const list = p?.list;
      if (!list) return [];
      const sc = list.getScreenClientRect();
      return p.rows.map((r, i) => ({
        id: r.id,
        name: r.name,
        ...cssCenter(ctx, {
          l: sc.l + COL.name,
          t: sc.t + ROWS_TOP + (i - list.scrollRow) * ROW_PITCH,
          r: sc.l + COL.name + 40,
          b: sc.t + ROWS_TOP + (i - list.scrollRow) * ROW_PITCH + ROW_PITCH,
        }),
      }));
    };
    const strip = w?.tab?.getScreenRect();
    const tabs = strip
      ? [0, 1, 2].map((i) => {
          const width = strip.r - strip.l;
          return cssCenter(ctx, {
            l: strip.l + (width * i) / 3,
            t: strip.b - 18,
            r: strip.l + (width * (i + 1)) / 3,
            b: strip.b,
          });
        })
      : [];
    const menuItems = (
      menu:
        | { items: readonly { getTitle(): string; getScreenRect(): Rect }[]; isVisible(): boolean }
        | null
        | undefined,
    ) =>
      menu?.isVisible() === true ? menu.items.map((i) => ({ text: i.getTitle(), ...ctl(i) })) : [];
    const im = (name: string) => {
      const win = m.message(name);
      return win
        ? { edit: ctl(win.edit), send: ctl(win.sendButton), text: ctl(win.text), title: ctl(win) }
        : null;
    };
    const names = feature.ims.all().map((x) => x.partner);
    return {
      tabs,
      tabIndex: w?.tabIndex ?? null,
      state: ctl(w?.getButton(933)),
      add: ctl(w?.getButton(742)),
      notes: ctl(w?.getButton(889)),
      sort: {
        channel: ctl(w?.getButton(648)),
        status: ctl(w?.getButton(545)),
        level: ctl(w?.getButton(584)),
        job: ctl(w?.getButton(516)),
        name: ctl(w?.getButton(177)),
      },
      friends: rowsOf('friend'),
      guild: rowsOf('guild'),
      stateMenu: menuItems(w?.stateMenu),
      friendMenu: menuItems(w?.friendMenu),
      ims: Object.fromEntries(names.map((n) => [n, im(n)])),
      popups: Object.fromEntries(m.instantKeys().map((k) => [k, ctl(m.instant(k))])),
      confirm: { yes: ctl(m.friendConfirm()?.yesButton), no: ctl(m.friendConfirm()?.noButton) },
      addFriend: {
        edit: ctl(m.addFriend()?.edit),
        ok: ctl(m.addFriend()?.okButton),
        cancel: ctl(m.addFriend()?.cancelButton),
      },
      note: { edit: ctl(m.note()?.edit), send: ctl(m.note()?.getDlgItem(933)) },
      notesWindow: ctl(m.notes()),
      summon: {
        field: ctl(m.summon()?.field),
        ok: ctl(m.summon()?.okButton),
        cancel: ctl(m.summon()?.cancelButton),
      },
      summonMsg: { ok: ctl(m.summonMsg()?.okButton), cancel: ctl(m.summonMsg()?.cancelButton) },
      blockList: ctl(m.blockList()),
      friendTab: TAB.friend,
    };
  });
}
