// `TooltipService` (`CToolTip` + `CWndMgr::PutToolTip_*`, tooltip.cpp, WndManager.cpp:6559-6646; Plan/42 §9): the tooltip
// host. Tooltips are INSTANT (no delay, no fade), wrap at 200 px, get the 9-tile frame, are placed by position 0-3 of the
// owner rect and clamped to the screen, and an item can bring up to two comparison tooltips of the worn items.
//
//   ui.tooltip.put(owner, 'text', ownerScreenRect, 0);
//   ui.tooltip.putItem(owner, item, cellRect, APP_INVENTORY, { flag, now, equippedAt });
//
// The service listens to the window manager: a window or control with a non-empty `toolTip` string shows it when the
// pointer enters it (`PutToolTip(this, m_strToolTip, m_rectCurrentClient, point, m_nToolTipPos)`); the tooltip goes away
// when the pointer leaves the owner rect, on any pointer press, when a shortcut is dragged and when the owner's window
// closes. The React layer is `TooltipLayer`.

import { TID_GAME_EQUIPED_ITEM } from '../../gen/defs/text';
import {
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_CLOTH,
  IK2_CLOTHETC,
  IK2_CLOTHWIG,
  IK2_JEWELRY,
  IK2_RIDING,
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
} from '../../gen/defs/itemkind';
import {
  PARTS_EARRING1,
  PARTS_EARRING2,
  PARTS_LWEAPON,
  PARTS_RING1,
  PARTS_RING2,
  PARTS_RWEAPON,
} from '../../gen/defs/neuz';
import { type Rect, rectContains } from '../layout/geometry';
import type { UiHost } from '../host';
import { FONT, lineHeight } from '../text/fonts';
import { layoutRich, type RichLine, textExtent } from '../text/layout';
import { DEFAULT_TEXT_COLOR, plainRuns, type RichRun } from '../text/markup';
import type { WindowManager, WindowManagerEvent } from '../wm/WindowManager';
import type { WndBase } from '../wnd/WndBase';
import { Observable } from '../wnd/observable';
import { placeTooltips, TOOLTIP_WRAP, tooltipBox, type TooltipPos } from './placement';
import type {
  ItemNumbersProvider,
  ItemTooltipComposer,
  ItemTooltipCtx,
  ItemTooltipNumbers,
  ItemView,
  SkillTooltipComposer,
  TooltipComposers,
  TooltipContent,
  TooltipExtra,
  TroupeTooltipComposer,
  UltimateBand,
} from './types';

/** The item kinds that bring comparison tooltips (`IK2_WEAPON_DIRECT` ... `IK2_CLOTHWIG`, §9.3). */
export const COMPARABLE_KIND2: readonly number[] = [
  IK2_WEAPON_DIRECT,
  IK2_WEAPON_MAGIC,
  IK2_ARMOR,
  IK2_ARMORETC,
  IK2_CLOTH,
  IK2_CLOTHETC,
  IK2_JEWELRY,
  IK2_RIDING,
  IK2_CLOTHWIG,
];

/**
 * The worn items an item is compared with (§9.3; `PutToolTipItemAndSubToolTip`): the item worn in the same `parts`
 * slot, and for a right weapon, a ring or an earring both items of the pair (right hand + left hand, ring 1 + ring 2,
 * earring 1 + earring 2); with only one of the pair worn, that one. At most two.
 */
export function comparisonItems(
  item: ItemView,
  equippedAt: (parts: number) => ItemView | null,
): ItemView[] {
  if (!COMPARABLE_KIND2.includes(item.kind2)) return [];
  const pair = (a: number, b: number): ItemView[] => {
    const x = equippedAt(a);
    const y = equippedAt(b);
    return [x, y].filter((i): i is ItemView => i !== null);
  };
  if (item.parts === PARTS_RWEAPON) return pair(PARTS_RWEAPON, PARTS_LWEAPON);
  if (item.parts === PARTS_RING1) return pair(PARTS_RING1, PARTS_RING2);
  if (item.parts === PARTS_EARRING1) return pair(PARTS_EARRING1, PARTS_EARRING2);
  const same = equippedAt(item.parts);
  return same ? [same] : [];
}

/** One tooltip box as the layer renders it: the text rect and what is drawn in it. */
export interface TooltipBoxView {
  kind: 'main' | 'sub1' | 'sub2';
  /** The text rect (the frame is drawn 8 px around it). */
  x: number;
  y: number;
  w: number;
  h: number;
  lines: readonly RichLine[];
  ultimate?: UltimateBand;
}

/** The visible tooltip(s). */
export interface TooltipView {
  owner: object;
  ownerRect: Rect;
  boxes: readonly TooltipBoxView[];
}

/** The default item composer until 45 registers its own: the name line only. */
export const defaultItemComposer: ItemTooltipComposer = (item) => ({
  runs: plainRuns(item.name, DEFAULT_TEXT_COLOR),
});

export interface TooltipServiceDeps {
  wm: WindowManager;
  host: UiHost;
}

export class TooltipService {
  readonly #wm: WindowManager;
  readonly #host: UiHost;
  readonly #view = new Observable<TooltipView | null>(null);
  #composers: TooltipComposers = {
    item: defaultItemComposer,
    skill: () => null,
    troupe: () => null,
  };
  #numbers: ItemNumbersProvider | null = null;
  readonly #cache = new Map<string, TooltipContent[]>();
  /**
   * The item tooltip that is shown and depends on the clock (`TooltipContent.volatile`: a cooldown, a keep time): the original
   * composes the tooltip every frame; here it is composed again once a second, bypassing the cache.
   */
  #live: {
    owner: object;
    item: ItemView;
    ownerRect: Rect;
    flag: number;
    ctx: Partial<ItemTooltipCtx>;
    at: number;
  } | null = null;
  /** The counters of the budget "at most one wasm call and one composition per hover" (read by the probe). */
  readonly stats = { numberCalls: 0, compositions: 0, cacheHits: 0 };

  constructor(deps: TooltipServiceDeps) {
    this.#wm = deps.wm;
    this.#host = deps.host;
  }

  /** React subscription (the `TooltipLayer`). */
  readonly subscribe = this.#view.subscribe;
  readonly getSnapshot = this.#view.get;

  /** The visible tooltip, `null` when none. */
  current(): TooltipView | null {
    return this.#view.get();
  }

  /** The text of the main tooltip (`__flyffDebug.ui.tooltip()`), `null` when none. */
  text(): string | null {
    const main = this.#view.get()?.boxes[0];
    if (!main) return null;
    return main.lines.map((l) => l.runs.map((r) => r.text).join('')).join('\n');
  }

  /** 45 registers the composers of items, skills and party skills (WP-CFEAT-16/17). */
  registerComposers(c: Partial<TooltipComposers>): void {
    this.#composers = { ...this.#composers, ...c };
    this.#cache.clear();
  }

  /**
   * The numbers side of item tooltips (§9.6): `item_tooltip_numbers` when the wasm module has it, else the existing
   * `item_multiplier` / `equip_check` exports (the adapter is `features/tooltip`). The provider runs once per composition.
   */
  setNumbersProvider(provider: ItemNumbersProvider | null): void {
    this.#numbers = provider;
    this.#cache.clear();
  }

  /** Forgets the composed tooltips (the inventory, the equipment or the shop context changed). */
  invalidate(): void {
    this.#cache.clear();
  }

  /**
   * `PutToolTip`: shows `content` for `owner`. A string is literal text (`CString` path, black); runs come from
   * `parseMarkup` (the `CEditString` path). `ownerRect` is in virtual screen coordinates. A repeated call for the same
   * owner and rect with the same text changes nothing.
   */
  put(
    owner: object,
    content: string | readonly RichRun[],
    ownerRect: Rect,
    pos: TooltipPos = 0,
    extra: TooltipExtra = {},
  ): void {
    const runs =
      typeof content === 'string' ? plainRuns(content, DEFAULT_TEXT_COLOR) : [...content];
    const c: TooltipContent = { runs, ...(extra.ultimate ? { ultimate: extra.ultimate } : {}) };
    this.#show(owner, ownerRect, pos, [c]);
  }

  /**
   * `PutToolTipItemAndSubToolTip`: the item tooltip plus, for equipment, the tooltips of the worn items to compare with
   * (each headed by `[ Equipped ]` in bold black).
   */
  putItem(
    owner: object,
    item: ItemView,
    ownerRect: Rect,
    flag: number,
    ctx: Partial<ItemTooltipCtx> = {},
  ): void {
    const full: ItemTooltipCtx = { flag, now: this.#host.now(), ...ctx };
    const key =
      item.serial === undefined
        ? null
        : `${String(item.serial)}|${String(flag)}|${String(full.version ?? '')}`;
    const cached = key === null ? undefined : this.#cache.get(key);
    if (cached) {
      this.stats.cacheHits += 1;
      this.#live = null;
      this.#show(owner, ownerRect, 0, cached);
      return;
    }
    const contents = this.#composeItem(item, full);
    const live = contents.some((c) => c.volatile === true);
    this.#live = live
      ? { owner, item, ownerRect: { ...ownerRect }, flag, ctx: { ...ctx }, at: full.now }
      : null;
    if (key !== null && !live) {
      this.#cache.set(key, contents);
      if (this.#cache.size > 24) {
        const oldest = this.#cache.keys().next();
        if (!oldest.done) this.#cache.delete(oldest.value);
      }
    }
    this.#show(owner, ownerRect, 0, contents);
  }

  /** One composition: the numbers (one provider call), the main tooltip and the headed comparison tooltips. */
  #composeItem(item: ItemView, ctx: ItemTooltipCtx): TooltipContent[] {
    this.stats.compositions += 1;
    let numbers: ItemTooltipNumbers | null | undefined = ctx.numbers;
    if (numbers === undefined && this.#numbers) {
      this.stats.numberCalls += 1;
      numbers = this.#numbers(item, ctx);
    }
    const withNumbers: ItemTooltipCtx = { ...ctx, numbers: numbers ?? null };
    const main = this.#composers.item(item, withNumbers);
    const contents: TooltipContent[] = [main];
    if (ctx.equippedAt) {
      for (const worn of comparisonItems(item, ctx.equippedAt)) {
        // each worn item has its own numbers (its upgrade level, its denied state): one more provider call per tooltip
        let wornNumbers: ItemTooltipNumbers | null = null;
        if (this.#numbers) {
          this.stats.numberCalls += 1;
          wornNumbers = this.#numbers(worn, ctx);
        }
        const c = this.#composers.item(worn, { ...ctx, numbers: wornNumbers });
        contents.push({
          ...c,
          runs: [...this.#equippedHeader(), ...c.runs],
        });
      }
    }
    return contents.slice(0, 3);
  }

  /** `PutToolTip_Skill`. */
  putSkill(owner: object, skillId: number, level: number, ownerRect: Rect, active: boolean): void {
    const c = this.#composers.skill(skillId, level, active);
    if (c) this.#show(owner, ownerRect, 0, [c]);
  }

  /** `PutToolTip_Troupe`. */
  putTroupe(owner: object, partySkillId: number, ownerRect: Rect): void {
    const c = this.#composers.troupe(partySkillId);
    if (c) this.#show(owner, ownerRect, 0, [c]);
  }

  /** `CancelToolTip`. */
  cancel(): void {
    this.#live = null;
    this.#view.set(null);
  }

  /** Composes the shown clock-dependent item tooltip again (once a second, from the window manager's tick). */
  #refreshLive(now: number): void {
    const l = this.#live;
    if (!l || now - l.at < 1000) return;
    const v = this.#view.get();
    if (v?.owner !== l.owner) {
      this.#live = null;
      return;
    }
    const full: ItemTooltipCtx = { flag: l.flag, ...l.ctx, now };
    const contents = this.#composeItem(l.item, full);
    l.at = now;
    if (!contents.some((c) => c.volatile === true)) this.#live = null;
    this.#show(l.owner, l.ownerRect, 0, contents);
  }

  /** `"[ " + getText(TID_GAME_EQUIPED_ITEM) + " ]\n"` bold black (`PutEquipItemText`). */
  #equippedHeader(): RichRun[] {
    const t = this.#host.bundles.texts?.text(TID_GAME_EQUIPED_ITEM);
    const label = t === undefined || t === '' || t === 'error' ? 'Equipped' : t;
    return [
      {
        text: `[ ${label} ]\n`,
        color: DEFAULT_TEXT_COLOR,
        bold: true,
        underline: false,
        strike: false,
      },
    ];
  }

  #show(
    owner: object,
    ownerRect: Rect,
    pos: TooltipPos,
    contents: readonly TooltipContent[],
  ): void {
    const cell = lineHeight(FONT.text);
    const laid = contents.map((c) => {
      // `SetUltimateToolTip`: the name is shifted right by 13 spaces to make room for the item icon at the top left
      const runs = c.ultimate ? ultimateRuns(c.runs) : c.runs;
      const lines = layoutRich(runs, TOOLTIP_WRAP, FONT.text, true);
      const ext = textExtent(lines, cell);
      return { c, lines, box: tooltipBox(ext.w, ext.h, lines.length) };
    });
    const placed = placeTooltips(
      pos,
      ownerRect,
      laid.map((l) => ({ ...l.box, ...(l.c.ultimate ? { ultimate: true } : {}) })),
      this.#wm.viewport,
    );
    const kinds = ['main', 'sub1', 'sub2'] as const;
    const boxes: TooltipBoxView[] = laid.flatMap((l, i) => {
      const p = placed[i];
      const kind = kinds[i];
      if (!p || !kind) return [];
      return [
        {
          kind,
          x: p.x,
          y: p.y,
          w: p.w,
          h: p.h,
          lines: l.lines,
          ...(l.c.ultimate ? { ultimate: l.c.ultimate } : {}),
        },
      ];
    });
    this.#view.set({ owner, ownerRect: { ...ownerRect }, boxes });
  }

  // ---- the window manager's events -------------------------------------------------------------------------------

  /**
   * Hooks the service to the window manager: automatic tooltips for windows with a `toolTip` string, and the hiding
   * rules. Returns the unsubscribe function.
   */
  attach(): () => void {
    return this.#wm.onEvent((e) => {
      this.#onEvent(e);
    });
  }

  #onEvent(e: WindowManagerEvent): void {
    switch (e.type) {
      case 'pointerDown':
        this.cancel();
        return;
      case 'closed': {
        const v = this.#view.get();
        if (
          v &&
          v.owner instanceof Object &&
          (v.owner as Partial<WndBase>).getFrameWnd?.() === e.frame
        ) {
          this.cancel();
        }
        return;
      }
      case 'hover': {
        this.cancel();
        const t = e.target;
        if (t && t.toolTip !== '' && !this.#wm.isDraggingShortcut()) {
          this.put(t, t.toolTip, t.getScreenClientRect(), t.toolTipPos);
        }
        return;
      }
      case 'pointerMove': {
        const v = this.#view.get();
        if (!v) return;
        if (this.#wm.isDraggingShortcut() || !rectContains(v.ownerRect, e.pt)) this.cancel();
        return;
      }
      case 'tick':
        this.#refreshLive(e.now);
        return;
      case 'opened':
      case 'focus':
      case 'viewport':
      case 'pointerUp':
        return;
    }
  }
}

/** `SetUltimateToolTip`: the first run (the name) gets 13 spaces in front. */
export function ultimateRuns(runs: readonly RichRun[]): RichRun[] {
  const first = runs[0];
  if (!first) return [];
  return [{ ...first, text: ULTIMATE_PREFIX + first.text }, ...runs.slice(1)];
}

/** The 13 spaces in front of the name of an ultimate weapon. */
export const ULTIMATE_PREFIX = ' '.repeat(13);

export type { ItemTooltipComposer, SkillTooltipComposer, TroupeTooltipComposer };
