// The debug probe of the lord windows (`__flyffDebug.features.lord()`, `lordPoints()` and `gc1to1Windows()`, Plan/45 §2.6): what the Playwright flow (`e2e/lord`) reads instead of pixels.
//
//   lord()             the slice and the view, the lord windows that are open, the plain texts of the open windows and the LORDSKILL slots of the taskbar
//   lordPoints()       the CSS pixel centres the spec clicks that no `controlPoint( app, id )` reaches: the rows of the State and Vote lists, the Event buttons, the 8 skill cells, the Pledge text
//   gc1to1Windows()    the texts of the open 1:1 application status window (ui-minigames' flows reuse it)

import type { ClientContext } from '../../../app/context';
import {
  APP_MESSAGEBOX,
  WIDC_BUTTON1,
  WIDC_BUTTON2,
  WIDC_BUTTON3,
  WIDC_BUTTON4,
  WIDC_BUTTON5,
  WIDC_TEXT1,
} from '../../../gen/defs/resdata';
import type { ListBox } from '../../../ui/controls/ListBox';
import { MessageBoxWnd } from '../../../ui/msgbox/MessageBoxWnd';
import { SC, type Shortcut } from '../../../ui/dnd/shortcut';
import type { Rect } from '../../../ui/layout/geometry';
import type { LordView } from '../../../ui/windows/lord/ports';
import { fullUi } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { taskbarOf } from '../../taskbar';
import type { LordSlice } from './slice';

const ZERO: Rect = { l: 0, t: 0, r: 0, b: 0 };

interface Pt {
  x: number;
  y: number;
}

/** The text of a markup string without its colour and style tags (the probe compares plain texts). */
const plain = (text: string): string =>
  text
    .replace(/#c[0-9a-fA-F]{8}|#nc|#nb|#b|#nu|#u/g, '')
    .replace(/\s+/g, ' ')
    .trim();

function listTexts(list: ListBox | null | undefined): string[] {
  if (!list) return [];
  return Array.from({ length: list.getCount() }, (_, i) => plain(list.getPlainText(i)));
}

export function registerLordProbe(
  ctx: ClientContext,
  src: { slice(): LordSlice; view: { get(): LordView } },
): void {
  // the CSS pixel centre of a rectangle given in the coordinates of `origin` (root coordinates when `origin` is zero)
  const css = (origin: Rect, r: Rect): Pt => {
    const ui = fullUi(ctx);
    const scale = ui?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    return {
      x: Math.round((root?.left ?? 0) + (origin.l + (r.l + r.r) / 2) * scale),
      y: Math.round((root?.top ?? 0) + (origin.t + (r.t + r.b) / 2) * scale),
    };
  };
  const rows = (list: ListBox | null | undefined): { index: number; x: number; y: number }[] => {
    if (!list) return [];
    const origin = list.getScreenClientRect();
    const out: { index: number; x: number; y: number }[] = [];
    for (let i = 0; i < list.getCount(); i += 1) {
      const r = list.itemRect(i);
      if (r) out.push({ index: i, ...css(origin, r) });
    }
    return out;
  };
  // the centre of a rectangle given relative to the CLIENT origin of a window (the cells of the skill window are layout placeholders in client coordinates)
  const centre = (
    w: { getScreenClientRect(): Rect } | null | undefined,
    r: Rect | undefined,
  ): Pt | null => {
    if (!w || !r) return null;
    const s = w.getScreenClientRect();
    return css(ZERO, { l: s.l + r.l, t: s.t + r.t, r: s.l + r.r, b: s.t + r.b });
  };
  // the centre of a control of a window
  const ctlCentre = (
    w: { getDlgItem(id: number): { getScreenRect(): Rect } | null } | null | undefined,
    id: number,
  ): Pt | null => {
    const c = w?.getDlgItem(id);
    return c ? css(ZERO, c.getScreenRect()) : null;
  };

  const boxText = (key: string): string | null => {
    const w = fullUi(ctx)?.wm.get(APP_MESSAGEBOX, key);
    return w instanceof MessageBoxWnd ? plain(w.message) : null;
  };

  addProbeMember(ctx, 'lord', () => {
    const ui = fullUi(ctx);
    const l = ui?.lord;
    const s = src.slice();
    const e = s.election;
    const state = l?.state();
    const tender = l?.tender();
    const vote = l?.vote();
    const event = l?.event();
    const info = l?.info();
    const skill = l?.skill();
    const pledge = l?.pledge();
    const confirm = l?.confirm();
    const skillConfirm = l?.skillConfirm();
    const ports = taskbarOf(ctx)?.ports;
    const slots: {
      row: string;
      page: number;
      slot: number;
      id: number;
      icon: string | null;
      cooldown: { startMs: number; endMs: number } | null;
      tooltip: string | null;
    }[] = [];
    const slotInfo = (sc: Shortcut) => ({
      icon: ports?.iconOf?.(sc) ?? null,
      cooldown: ports?.slotIcon(sc)?.cooldown ?? null,
      tooltip: ports?.slotTooltip?.(sc) ?? null,
    });
    const tb = ui?.taskbar.model.get();
    tb?.items.forEach((page, p) => {
      page.forEach((sc, i) => {
        if (sc?.kind === SC.LORDSKILL) {
          slots.push({ row: 'item', page: p, slot: i, id: sc.id, ...slotInfo(sc) });
        }
      });
    });
    tb?.applets.forEach((sc, i) => {
      if (sc?.kind === SC.LORDSKILL) {
        slots.push({ row: 'applet', page: 0, slot: i, id: sc.id, ...slotInfo(sc) });
      }
    });
    return {
      lord: s.lord,
      election: {
        id: e.id,
        state: e.state,
        beginAt: e.beginAt,
        requirement: e.requirement,
        candidates: e.candidates.map((c) => ({
          id: c.id,
          deposit: c.deposit,
          votes: c.votes,
          pledge: c.pledge,
        })),
      },
      events: s.events,
      skillTicks: s.skillTicks,
      view: src.view.get(),
      open: l?.openApps() ?? [],
      state: state
        ? {
            live: state.liveStatic?.getTitle() ?? '',
            rows: listTexts(state.list),
            select: state.selectedRank,
          }
        : null,
      tender: tender
        ? {
            edit: tender.edit?.getString() ?? '',
            info: plain(tender.infoText?.getString() ?? ''),
            list: plain(tender.listText?.getString() ?? ''),
          }
        : null,
      pledge: pledge
        ? {
            rank: pledge.rank,
            candidate: pledge.candidateId,
            text: pledge.textCtrl?.getString() ?? '',
            editing: pledge.edit?.isVisible() ?? false,
            first: pledge.isFirst,
          }
        : null,
      confirm: confirm
        ? { text: plain(confirm.textCtrl?.getString() ?? ''), visible: confirm.isVisible() }
        : null,
      vote: vote
        ? {
            edit: vote.edit?.getString() ?? '',
            rows: vote.texts?.items.map((i) => i.text) ?? [],
          }
        : null,
      event: event
        ? {
            exp: plain(event.expText?.getString() ?? ''),
            item: plain(event.itemText?.getString() ?? ''),
            cost: plain(event.costText?.getString() ?? ''),
            expIndex: event.expIndex,
            itemIndex: event.itemIndex,
          }
        : null,
      info: info
        ? {
            status: info.statusStatic?.getTitle() ?? '',
            name: info.nameStatic?.getTitle() ?? '',
            time: info.timeStatic?.getTitle() ?? '',
            text: plain(info.infoText?.getString() ?? ''),
          }
        : null,
      skill: skill
        ? {
            icons: skill.sprites?.entries.length ?? 0,
            text: plain(skill.infoText?.getString() ?? ''),
          }
        : null,
      skillConfirm: skillConfirm
        ? { skill: skillConfirm.skill, text: plain(skillConfirm.textCtrl?.getString() ?? '') }
        : null,
      rpInfo: plain(l?.rpInfo()?.infoText?.getString() ?? ''),
      // the standard box and the upper box (`OpenMessageBoxUpper`: the deposit and the final-ten boxes), their plain texts
      boxes: {
        main: boxText('main'),
        upper: boxText('upper'),
      },
      slots,
    };
  });

  addProbeMember(ctx, 'lordPoints', () => {
    const l = fullUi(ctx)?.lord;
    if (!l) return null;
    const state = l.state();
    const vote = l.vote();
    const event = l.event();
    const skill = l.skill();
    const pledge = l.pledge();
    return {
      stateRows: rows(state?.list),
      voteRows: rows(vote?.list),
      skillCells: skill
        ? [0, 1, 2, 3, 4, 5, 6, 7].map((i) => centre(skill, skill.cellRect(i)))
        : [],
      pledgeText: ctlCentre(pledge, WIDC_TEXT1),
      event: event
        ? {
            expDown: ctlCentre(event, WIDC_BUTTON1),
            expUp: ctlCentre(event, WIDC_BUTTON2),
            itemDown: ctlCentre(event, WIDC_BUTTON3),
            itemUp: ctlCentre(event, WIDC_BUTTON4),
            start: ctlCentre(event, WIDC_BUTTON5),
          }
        : null,
    };
  });

  addProbeMember(ctx, 'gc1to1Windows', () => {
    const g = fullUi(ctx)?.gc1to1;
    const w = g?.offerState();
    if (!w) return { open: false };
    return {
      open: true,
      title: w.getTitle(),
      time: w.timeStatic?.getTitle() ?? '',
      rank: w.rankStatic?.getTitle() ?? '',
      bid: w.bidStatic?.getTitle() ?? '',
      hasPrizeLine: w.prizeStatic !== null,
      rows: listTexts(w.list),
    };
  });
}
