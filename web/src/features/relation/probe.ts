// The debug probe of the relation feature (`window.__flyffDebug.features.relation()`, `relationPoints()` and the three hooks, Plan/45 §2.6; WP-CFEAT-40): what the Playwright specs read
// instead of pixels - the couple, the campus, the honor list and its selection, the cheer points, the titles above the names, the open windows - and the CSS pixel centres of the
// controls the specs click. The hooks `relationPropose`, `relationCheer` and `relationInvite` call the SAME functions as the motion and the menu entries; a spec uses them only when the
// pick ray misses a character and says so in its log.

import type { ClientContext } from '../../app/context';
import type { InputInstance } from '../../input/input';
import type { UiApi } from '../../ui';
import { EntityKind } from '../../game/entities';
import { APP_CHARACTER3, APP_CONFIRM_ENTER, APP_MOTION } from '../../gen/defs/resdata';
import { MOT_BASE_CHEER, MOT_BASE_COUPLE } from '../../gen/defs/neuz';
import type { Control } from '../../ui/wnd/Control';
import type { Rect } from '../../ui/layout/geometry';
import { motionCell } from '../../ui/windows/char/MotionPages';
import type { WndCharacter } from '../../ui/windows/char/WndCharacter';
import type { WndMotion } from '../../ui/windows/char/WndMotion';
import type { CampusView } from '../../ui/windows/messenger/ports';
import type {
  CoupleInfoView,
  CoupleSkillView,
  HonorView,
  ViewSource,
} from '../../ui/windows/couple/ports';
import { fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { motionsOf } from '../motions/motionsModule';
import type { RelationFeature } from './relationModule';
import type { RelationStore } from './store';

interface Point {
  x: number;
  y: number;
}

/** The CSS pixel centre of a rect of root (virtual UI) coordinates. */
function cssCenter(ui: UiApi, r: Rect): Point | null {
  if (typeof document === 'undefined') return null;
  const scale = ui.scaler.get().scale;
  const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
  return {
    x: Math.round((root?.left ?? 0) + ((r.l + r.r) / 2) * scale),
    y: Math.round((root?.top ?? 0) + ((r.t + r.b) / 2) * scale),
  };
}

export interface RelationProbeDeps {
  readonly feature: RelationFeature;
  readonly store: RelationStore;
  readonly couple: ViewSource<CoupleInfoView>;
  readonly skill: ViewSource<CoupleSkillView>;
  readonly stats: { readonly honorAcks: number };
  readonly campus: ViewSource<CampusView>;
  readonly honorView: ViewSource<HonorView>;
}

/** A window that has a yes and a no button (`WndYesNoText`): its text and buttons, read without importing the lazy class. */
interface YesNo {
  text: { getString(): string } | null;
  yesButton: Control | null;
  noButton: Control | null;
}

export function registerRelationProbe(ctx: ClientContext, d: RelationProbeDeps): void {
  const { store, feature } = d;

  addProbeMember(ctx, 'relation', () => {
    const s = store.getState();
    const ui = fullUi(ctx);
    const info = d.couple.get();
    const confirm = ui?.wm.get(APP_CONFIRM_ENTER) as unknown as YesNo | null | undefined;
    const character = ui?.wm.get(APP_CHARACTER3) as WndCharacter | null | undefined;
    const honorPage = character?.honorPage ?? null;
    const titles: { objId: number; name: string; title: string; self: boolean }[] = [];
    ctx.entities.forEach(EntityKind.Player, (rec) => {
      titles.push({
        objId: rec.objId,
        name: rec.visual?.name ?? '',
        title: rec.visual?.title ?? '',
        self: rec.isSelf,
      });
    });
    const now = ctx.now();
    return {
      focus: (ctx.input as Partial<InputInstance>).core?.targeting.focus ?? null,
      couple: s.couple && {
        exp: s.couple.exp,
        level: s.couple.level,
        first: s.couple.first,
        second: s.couple.second,
      },
      coupleView: info,
      coupleSkill: d.skill.get().cells.map((c) => ({ name: c.name, lit: c.lit })),
      campus: s.campus,
      campusView: d.campus.get(),
      campusPoints: s.campusPoints,
      honor: {
        earned: s.honor.earned,
        selected: s.honor.selected,
        acks: d.stats.honorAcks,
        view: d.honorView.get().titles.map((t) => t.title),
      },
      cheer: { points: s.cheer.points, restMs: Math.max(0, s.cheer.restUntil - now) },
      titles,
      coupleOpen: ui?.couple.manager() != null,
      coupleTab: ui?.couple.manager()?.tab?.getCurSel() ?? null,
      coupleMessage: ui?.couple.message()?.text ?? null,
      confirmOpen: confirm != null,
      confirmText: confirm?.text?.getString() ?? null,
      characterOpen: character != null,
      characterTab: character?.tab?.getCurSel() ?? null,
      honorPage: honorPage && {
        items: Array.from({ length: honorPage.list?.getCount() ?? 0 }, (_v, i) =>
          honorPage.list?.getPlainText(i),
        ),
        okEnabled: honorPage.okButton?.isWindowEnabled() ?? false,
        pending: honorPage.pending,
        empty: honorPage.textLayer?.items.map((i) => i.text) ?? [],
        selected: honorPage.list?.getCurSel() ?? -1,
      },
      motion: motionsOf(ctx)?.executor.last ?? null,
    };
  });

  addProbeMember(ctx, 'relationPoints', () => {
    const ui = fullUi(ctx);
    if (!ui) return null;
    const at = (r: Rect | null | undefined): Point | null => (r ? cssCenter(ui, r) : null);
    const ctl = (c: { getScreenRect(): Rect } | null | undefined): Point | null =>
      c ? at(c.getScreenRect()) : null;
    const tabs = (
      tab: { getScreenRect(): Rect } | null | undefined,
      n: number,
    ): (Point | null)[] => {
      if (!tab) return [];
      const strip = tab.getScreenRect();
      const width = strip.r - strip.l;
      return Array.from({ length: n }, (_v, i) =>
        at({
          l: strip.l + (width * i) / n,
          t: strip.b - 18,
          r: strip.l + (width * (i + 1)) / n,
          b: strip.b,
        }),
      );
    };
    const manager = ui.couple.manager();
    const skill = manager?.skillPage ?? null;
    const message = ui.couple.message();
    const confirm = ui.wm.get(APP_CONFIRM_ENTER) as unknown as YesNo | null;
    const character = ui.wm.get(APP_CHARACTER3) as WndCharacter | null;
    const honor = character?.honorPage ?? null;
    const list = honor?.list ?? null;
    const motionWnd = ui.wm.get(APP_MOTION) as WndMotion | null;
    const motionIndex = (id: number): number =>
      motionsOf(ctx)
        ?.view.get()
        .motions.findIndex((m) => m.id === id) ?? -1;
    const motionCellPoint = (id: number): Point | null => {
      const page = motionWnd?.motionPage;
      const i = motionIndex(id);
      if (!page || i < 0) return null;
      const s = page.getScreenClientRect();
      const c = motionCell(i);
      return at({ l: s.l + c.l, t: s.t + c.t, r: s.l + c.r, b: s.t + c.b });
    };
    const rows = (): { text: string; x: number; y: number }[] => {
      if (!list) return [];
      const out: { text: string; x: number; y: number }[] = [];
      const s = list.getScreenClientRect();
      for (let i = 0; i < list.getCount(); i += 1) {
        const r = list.itemRect(i);
        const p = r ? at({ l: s.l + r.l, t: s.t + r.t, r: s.l + r.r, b: s.t + r.b }) : null;
        if (p) out.push({ text: list.getPlainText(i), ...p });
      }
      return out;
    };
    const menu = ui.npc.moverMenu;
    return {
      menu: menu.shown ? menu.items.map((i) => ({ id: i.id, text: i.getTitle(), ...ctl(i) })) : [],
      couple: {
        window: ctl(manager),
        tabs: tabs(manager?.tab, 2),
        breakUp: ctl(manager?.infoPage?.breakButton),
        skillCells: [0, 1, 2].map((i) => {
          const g = skill?.getWndCtrl(929)?.rect;
          const s = skill?.getScreenClientRect();
          if (!g || !s) return null;
          const l = s.l + g.l + 4 + 32 * i;
          const t = s.t + g.t + 2;
          return at({ l, t, r: l + 32, b: t + 32 });
        }),
      },
      message: {
        ok: ctl(message?.buttons[0]),
        cancel: ctl(message?.buttons[1]),
      },
      confirm: { yes: ctl(confirm?.yesButton), no: ctl(confirm?.noButton) },
      character: { tabs: tabs(character?.tab, 2), rows: rows(), ok: ctl(honor?.okButton) },
      motion: {
        cheer: motionCellPoint(MOT_BASE_CHEER),
        couple: motionCellPoint(MOT_BASE_COUPLE),
      },
    };
  });

  // the hooks call the same functions as the motion and the menu entries
  addProbeMember(ctx, 'relationPropose', (objId: number) => feature.proposeTo(objId));
  addProbeMember(ctx, 'relationCheer', (objId: number) => feature.cheer(objId));
  addProbeMember(ctx, 'relationInvite', (objId: number) => feature.invite(objId));
}
