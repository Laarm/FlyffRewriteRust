// The debug probe of the guild feature (`__flyffDebug.features.guild()`, `guildPoints()`, `guildControl( app, name )`; Plan/45 §2.6): read-only snapshots and the CSS pixel centres of rows,
// cells and buttons that the Playwright specs click (the specs read the probe, not pixels). `guildPoints()` is to the guild flows what `partyPoints()` is to the party ones.

import type { ClientContext } from '../../app/context';
import { EntityKind } from '../../game/entities';
import {
  APP_GUILD,
  APP_GUILD_BANK,
  APP_GUILD_BANK_LOG,
  APP_GUILD_DISMISS,
  APP_GUILD_INVATE,
  APP_GUILD_MERIT1,
  APP_GUILD_NICKNAME,
  APP_GUILD_NOTICE,
  APP_GUILD_PAYCONFIRM,
  APP_GUILD_SETLOGO,
  APP_GUILD_WAR,
  APP_GUILD_WARGIVEUP,
  APP_GUILD_WARPEACE,
  APP_GUILD_WARPEACECONFIRM,
  APP_GUILD_WARREQUEST,
  APP_GUILDNAME,
} from '../../gen/defs/resdata';
import type { Rect } from '../../ui/layout/geometry';
import { GUILD_LOGO_COUNT, logoCell } from '../../ui/windows/guild';
import type { GuildPorts } from '../../ui/windows/guild/ports';
import type { GuildBankView, BankLogView } from '../../ui/windows/guild/ports';
import { fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import type { ViewSource } from '../common/view';
import type { GuildFeature } from './api';
import type { PowerDraft } from './power';
import { ownBrief } from './store';

export interface ProbeParts {
  readonly ports: GuildPorts;
  readonly bankView: ViewSource<GuildBankView>;
  readonly logView: ViewSource<BankLogView>;
  readonly draft: PowerDraft;
  isOnline(id: number): boolean;
  rank(): number;
}

const ZERO: Rect = { l: 0, t: 0, r: 0, b: 0 };

export function registerGuildProbe(
  ctx: ClientContext,
  feature: GuildFeature,
  parts: ProbeParts,
): void {
  const store = feature.store;

  // the CSS pixel centre of a rectangle given in the coordinates of `origin` (root coordinates when `origin` is {0,0,0,0})
  const css = (origin: Rect, r: Rect): { x: number; y: number } => {
    const ui = fullUi(ctx);
    const scale = ui?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    return {
      x: Math.round((root?.left ?? 0) + (origin.l + (r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + (origin.t + (r.t + r.b) / 2) * scale),
    };
  };

  addProbeMember(ctx, 'guild', () => {
    const s = store.getState();
    const ui = fullUi(ctx);
    const view = parts.ports.view.get();
    const open = (app: number): boolean => ui?.wm.isOpen(app) ?? false;
    const players: unknown[] = [];
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      if (rec.visual) {
        players.push({
          objId: rec.objId,
          name: rec.visual.name,
          self: rec.isSelf,
          guild: rec.visual.guild
            ? {
                id: rec.visual.guild.idGuild,
                name: rec.visual.guild.name,
                level: rec.visual.guild.level,
                logo: rec.visual.guild.logo,
              }
            : null,
          idWar: rec.visual.idWar,
        });
      }
    });
    const own = s.own;
    const b = ownBrief(s);
    return {
      briefs: [...s.briefs.values()].map((g) => ({
        id: g.idGuild,
        name: g.name,
        level: g.level,
        logo: g.logo,
        master: g.idMaster,
        gold: g.guildGold,
        win: g.win,
        lose: g.lose,
        surrender: g.surrender,
      })),
      own: own
        ? {
            id: own.id,
            name: b?.name ?? '',
            level: b?.level ?? 0,
            logo: b?.logo ?? 0,
            gold: b?.guildGold ?? 0,
            pxp: own.pxp,
            notice: own.notice,
            powers: own.powers,
            salaries: own.salaries,
            enemy: own.enemy,
            members: view.members.map((m) => ({ ...m })),
          }
        : null,
      selfId: ctx.stores.selfStore.getState().idPlayer,
      selfGuildId: ctx.stores.selfStore.getState().guildId,
      selfWar: s.selfWar,
      gold: ctx.stores.selfStore.getState().gold,
      rank: parts.rank(),
      isMaster: view.isMaster,
      draft: { words: parts.draft.words, base: parts.draft.base, dirty: parts.draft.dirty },
      wars: [...s.wars.values()],
      bank: {
        open: s.bank.open,
        gold: b?.guildGold ?? 0,
        items: Array.from({ length: 42 }, (_, slot) => s.bank.container?.getAtSlot(slot) ?? null)
          .map((it, slot) =>
            it
              ? {
                  slot,
                  objId: it.objId,
                  itemId: it.itemId,
                  num: it.num,
                  idGuild: it.idGuild,
                  name: ctx.data?.item(it.itemId)?.name ?? '',
                }
              : null,
          )
          .filter((x) => x !== null),
      },
      log: s.log,
      windows: {
        guild: open(APP_GUILD),
        invite: open(APP_GUILD_INVATE),
        name: open(APP_GUILDNAME),
        nickname: open(APP_GUILD_NICKNAME),
        dismiss: open(APP_GUILD_DISMISS),
        notice: open(APP_GUILD_NOTICE),
        logo: open(APP_GUILD_SETLOGO),
        pay: open(APP_GUILD_PAYCONFIRM),
        merit: open(APP_GUILD_MERIT1),
        bank: open(APP_GUILD_BANK),
        bankLog: open(APP_GUILD_BANK_LOG),
        warDecl: open(APP_GUILD_WAR),
        warGiveUp: open(APP_GUILD_WARGIVEUP),
        warPeace: open(APP_GUILD_WARPEACE),
        warPeaceConfirm: open(APP_GUILD_WARPEACECONFIRM),
        warRequest: open(APP_GUILD_WARREQUEST),
      },
      tab: ui?.guild.guild()?.tab?.getCurSel() ?? null,
      view: {
        hasGuild: view.hasGuild,
        name: view.name,
        level: view.level,
        masterName: view.masterName,
        memberCount: view.memberCount,
        maxMembers: view.maxMembers,
        pxp: view.pxp,
        penya: view.penya,
        notice: view.notice,
        logo: view.logo,
        salaries: view.salaries,
        rankCounts: view.rankCounts,
        war: view.war,
      },
      players,
    };
  });

  addProbeMember(ctx, 'guildPoints', () => {
    const ui = fullUi(ctx);
    if (!ui) return null;
    const win = ui.guild.guild();
    const page = win?.memberPage;
    const list = page?.list;
    const tab = win?.tab;
    const menu = win?.memberMenu;
    const logoWin = ui.guild.logo();
    const logTab = ui.guild.bankLog()?.tab;
    const bank = ui.guild.bank();
    const grid = bank?.grid;
    const rows = list
      ? page.rows.map((m, i) => {
          const r = list.rowRect(i);
          return {
            index: i,
            id: m.id,
            ...(r ? css(list.getScreenClientRect(), r) : { x: 0, y: 0 }),
          };
        })
      : [];
    return {
      // the five caption strips of the guild window's tab control (the strip is the last 18 px of the control)
      tabs: tab
        ? [0, 1, 2, 3, 4].map((i) => {
            const r = tab.getScreenRect();
            const width = r.r - r.l;
            return {
              index: i,
              ...css(ZERO, {
                l: r.l + (width * i) / 5,
                t: r.b - 18,
                r: r.l + (width * (i + 1)) / 5,
                b: r.b,
              }),
            };
          })
        : [],
      // the four caption strips of the bank log's tab control (item added / removed, penya added / removed)
      logTabs: logTab
        ? [0, 1, 2, 3].map((i) => {
            const r = logTab.getScreenRect();
            const width = r.r - r.l;
            return {
              index: i,
              ...css(ZERO, {
                l: r.l + (width * i) / 4,
                t: r.b - 18,
                r: r.l + (width * (i + 1)) / 4,
                b: r.b,
              }),
            };
          })
        : [],
      rows,
      menu: menu?.shown
        ? menu.items.map((item) => ({
            id: item.id,
            text: item.getTitle(),
            ...css(ZERO, item.getScreenRect()),
          }))
        : [],
      logoCells: logoWin
        ? Array.from({ length: GUILD_LOGO_COUNT }, (_, i) => i + 1)
            .map((n) => {
              const c = logoCell(n);
              const origin = logoWin.getScreenClientRect();
              return c ? { n, ...css(origin, c) } : null;
            })
            .filter((x) => x !== null)
        : [],
      bankCells:
        bank && grid
          ? Array.from({ length: 42 }, (_, slot) => {
              const r = grid.cellScreenRect(slot);
              return r ? { slot, ...css(ZERO, r) } : null;
            }).filter((x) => x !== null)
          : [],
      bankCoin: bank?.coin ? css(ZERO, bank.coin.getScreenRect()) : null,
    };
  });

  // the centre of a control of a page of the guild window, by page and WIDC name (`guildPageControl( 'app', 'WIDC_CHECK21' )`); the pages are tab pages, so their controls are
  // not children of the window that `guildControl` searches
  addProbeMember(ctx, 'guildPageControl', (pageName: string, name: string) => {
    const win = fullUi(ctx)?.guild.guild();
    const page =
      pageName === 'info'
        ? win?.infoPage
        : pageName === 'member'
          ? win?.memberPage
          : pageName === 'app'
            ? win?.appPage
            : pageName === 'war'
              ? win?.warPage
              : pageName === 'power'
                ? win?.powerPage
                : null;
    const c = page?.getDlgItemByName(name);
    return c ? css(ZERO, c.getScreenRect()) : null;
  });

  // the centre of a control of an open window, by app id and WIDC name (`guildControl( 336, 'WIDC_BUTTON1' )`)
  addProbeMember(ctx, 'guildControl', (app: number, name: string) => {
    const ui = fullUi(ctx);
    const w = ui?.wm.get(app);
    const c = w?.getDlgItemByName(name);
    return c ? css(ZERO, c.getScreenRect()) : null;
  });
}
