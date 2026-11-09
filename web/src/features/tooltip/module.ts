// The `tooltip` feature module (Plan/45 §10, WP-CFEAT-16/17): registers the item composer and the numbers provider with 42's
// tooltip host once the UI exists, feeds the composition with the live state (the own character as the viewer, the worn items for
// the set count and the +N set bonus, the shop window that is open, the cooldown groups) and invalidates the host's cache when
// any of it changes. The shop and the inspection windows tell it about their context with `tooltipOf(ctx).setShop(...)`.

import type { ClientContext } from '../../app/context';
import type { ClientDataIndex } from '../../data/clientData';
import { peekRulesRuntime } from '../../game/rules';
import { II_SYS_SYS_FEED_01 } from '../../gen/defs/item';
import {
  PARTS_CAP,
  PARTS_FOOT,
  PARTS_HAND,
  PARTS_RWEAPON,
  PARTS_UPPER_BODY,
} from '../../gen/defs/neuz';
import { APP_INVENTORY } from '../../gen/defs/resdata';
import { wallNow } from '../../platform/time';
import {
  splitRandomOptionWord,
  type ItemTooltipInput,
  type RulesFacade,
  type TooltipShopJs,
  type TooltipWornJs,
} from '../../rules';
import type { ItemTooltipNumbers } from '../../rules/types';
import type { ItemNumbersProvider, ItemTooltipCtx, ItemView } from '../../ui/tooltip/types';
import { defaultItemComposer } from '../../ui/tooltip/TooltipService';
import { type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { guildOf } from '../guild/api';
import { inventoryOf } from '../inventory';
import { isExpired, partsOf } from '../inventory/info';
import { defineOptionsStore } from '../options';
import { createItemComposer } from './compose';
import { createComposeEnv } from './env';
import { jobTypeByLayout } from './jobs';
import {
  isTooltipItemView,
  type ItemTooltipExtra,
  type ShopTooltipContext,
  type TooltipItemView,
  type TooltipOwnerInfo,
} from './model';

/** The shop window that is open, as the price lines and the rules call need it. */
export interface ShopTooltipSource {
  readonly context: ShopTooltipContext;
  /** `rates.shop_cost`, the event factors and the tax rates for `item_tooltip_numbers`. */
  readonly rules: TooltipShopJs;
}

export interface TooltipFeature {
  /** The shop window opened / closed (`null`): prices show on the shop grid and, for sale prices, on the inventory. */
  setShop(source: ShopTooltipSource | null): void;
  /** The viewer of the tooltips of another mover's items (`APP_QUERYEQUIP`); `null` = the own character. */
  setInspected(
    owner: { info: TooltipOwnerInfo; worn: readonly TooltipWornJs[]; rweaponItem: number } | null,
  ): void;
  /** Forgets the composed tooltips (the host re-composes on the next hover). */
  invalidate(): void;
  /** The numbers provider the host calls (exposed for the tests and the shop windows). */
  readonly numbers: ItemNumbersProvider;
}

const features = new WeakMap<ClientContext, TooltipFeature>();

/** The tooltip feature of a context; `undefined` before the module registered. */
export function tooltipOf(ctx: ClientContext): TooltipFeature | undefined {
  return features.get(ctx);
}

/** The reduced numbers of a rules answer (the host contract) with the full answer in `detail`. */
export function hostNumbers(n: ItemTooltipNumbers): ReturnType<ItemNumbersProvider> {
  return {
    multiplier: n.multiplier,
    ability: n.ability,
    equipDenied: n.equipDenied,
    prices: {
      buyUnit: n.prices.buyUnit,
      buyTax: n.prices.buyTax,
      sellUnit: n.prices.sellUnit,
      sellTax: n.prices.sellTax,
    },
    detail: n,
  };
}

/** The `ItemTooltipNumbers` of a rules facade without `itemTooltipNumbers` (a fake): the old two exports and empty lists. */
function fallbackNumbers(
  rules: Pick<RulesFacade, 'itemMultiplier' | 'itemMinMax' | 'equipCheck'>,
  input: ItemTooltipInput,
): ItemTooltipNumbers {
  const multiplier = rules.itemMultiplier(
    input.itemId,
    input.abilityOption ?? 0,
    input.expired === true,
  );
  const [min, max] = rules.itemMinMax(
    input.itemId,
    input.abilityOption ?? 0,
    input.expired === true,
  );
  const o = input.owner;
  const equipDenied = o
    ? rules.equipCheck(
        input.itemId,
        input.randomOptionHi ?? 0,
        o.sex,
        o.job,
        o.level,
        o.rweaponItem,
      )
    : 0;
  return {
    known: true,
    multiplier,
    ability: { min, max },
    equipDenied,
    prices: { cost: -1, buyUnit: 0, buyTax: 0, sellUnit: 0, sellTax: 0, chipCost: -1 },
    awakeningProtected: false,
    randomOptions: [],
    randomStat: null,
    set: null,
    piercingDsts: [],
    gemDsts: [],
    elementResistDsts: [],
  };
}

export const tooltipModule: FeatureModule = {
  id: 'tooltip',
  milestone: 4,
  register(fc) {
    const ctx = fc.app;
    const { selfStore } = ctx.stores;
    const options = defineOptionsStore(ctx.stores.registry);
    let shop: ShopTooltipSource | null = null;
    let inspected: {
      info: TooltipOwnerInfo;
      worn: readonly TooltipWornJs[];
      rweaponItem: number;
    } | null = null;

    const prop = (id: number) => ctx.data?.item(id);

    const wornOfSelf = (): TooltipWornJs[] => {
      const inv = inventoryOf(ctx);
      if (!inv) return [];
      const c = inv.state().container;
      const out: TooltipWornJs[] = [];
      for (let part = 0; part < c.max - c.indexNum; part += 1) {
        const it = c.getEquip(part);
        if (it) out.push({ part, itemId: it.itemId, expired: isExpired(it) });
      }
      return out;
    };

    const ownerInfo = (): TooltipOwnerInfo => {
      if (inspected) return inspected.info;
      const s = selfStore.getState();
      return { sex: s.sex, job: s.job, level: s.level, jobType: jobTypeByLayout(s.job) ?? 0 };
    };

    const rulesInput = (view: TooltipItemView): ItemTooltipInput => {
      const it = view.item;
      const owner = ownerInfo();
      const inv = inventoryOf(ctx);
      const rweapon = inspected
        ? inspected.rweaponItem
        : (inv?.state().container.getEquip(PARTS_RWEAPON)?.itemId ?? 0);
      return {
        itemId: it.itemId,
        abilityOption: it.abilityOption,
        expired: (it.flag & 1) !== 0,
        element: it.itemResist,
        elementLevel: it.resistAbilityOption,
        ...splitRandomOptionWord(it.randomOpt),
        eggLevelC: (it.pet?.level ?? 0) >= 2,
        piercing: it.cards,
        ultimateGems: it.gems,
        owner: { sex: owner.sex, job: owner.job, level: owner.level, rweaponItem: rweapon },
        worn: inspected ? inspected.worn : wornOfSelf(),
        ...(shop ? { shop: shop.rules } : {}),
      };
    };

    const rulesOf = (): RulesFacade | null =>
      peekRulesRuntime(ctx)?.rules() ?? (ctx.rules as RulesFacade | null);

    const numbers: ItemNumbersProvider = (item: ItemView) => {
      if (!isTooltipItemView(item)) return null;
      const rules = rulesOf();
      if (!rules || !prop(item.item.itemId)) return null;
      const input = rulesInput(item);
      const answer =
        typeof rules.itemTooltipNumbers === 'function'
          ? rules.itemTooltipNumbers(input)
          : fallbackNumbers(rules, input);
      return hostNumbers(answer);
    };

    const setBonus = (view: TooltipItemView, flag: number): ItemTooltipExtra['armorSetBonus'] => {
      // `PutEnchantOpt`: only on a worn set piece, only while at least +1 of every piece is worn
      if (inspected) return null;
      const inv = inventoryOf(ctx);
      if (!inv || flag !== APP_INVENTORY) return null;
      const c = inv.state().container;
      const objId = view.item.objId;
      if (objId === undefined || !c.isEquip(objId)) return null;
      const part = partsOf(prop(view.item.itemId));
      if (![PARTS_UPPER_BODY, PARTS_HAND, PARTS_FOOT, PARTS_CAP].includes(part)) return null;
      let level = 10;
      for (const p of [PARTS_UPPER_BODY, PARTS_HAND, PARTS_FOOT, PARTS_CAP]) {
        const w = c.getEquip(p);
        const v = w ? w.abilityOption : 0;
        if (v === 0) return null;
        level = Math.min(level, v);
      }
      if (level <= 0) return null;
      return ctx.data?.bundle('tables')?.exp.set_item_avail[level - 1] ?? null;
    };

    const env = createComposeEnv({
      data: () => ctx.data,
      features: () => ctx.features,
      colorSet: () => options.getState().ui.tooltipColorSet,
      nowSec: () => Math.floor(wallNow() / 1000),
      // a guild cloak reads `TID_GUILD_CLOAK( guild )`
      guildName: (id) => guildOf(ctx)?.nameOf(id) ?? null,
      feedCount: () => inventoryOf(ctx)?.countOf(II_SYS_SYS_FEED_01) ?? 0,
      cooldownEnd: (group) => {
        const cd = inventoryOf(ctx)?.state().cooldowns[group - 1];
        return cd ? cd.endMs : null;
      },
    });

    const baseComposer = createItemComposer(env, defaultItemComposer);
    const composer = (view: ItemView, c: ItemTooltipCtx) => {
      if (!isTooltipItemView(view)) return baseComposer(view, c);
      // the module's own context fills what the hovering window did not pass
      const given = (c.extra ?? {}) as ItemTooltipExtra;
      const extra: ItemTooltipExtra = {
        owner: given.owner ?? ownerInfo(),
        shop: given.shop !== undefined ? given.shop : (shop?.context ?? null),
        vendorPrice: given.vendorPrice,
        armorSetBonus:
          given.armorSetBonus !== undefined ? given.armorSetBonus : setBonus(view, c.flag),
      };
      return baseComposer(view, { ...c, extra });
    };

    const api: TooltipFeature = {
      setShop(source) {
        shop = source;
        fullUi(ctx)?.tooltip.invalidate();
      },
      setInspected(owner) {
        inspected = owner;
        fullUi(ctx)?.tooltip.invalidate();
      },
      invalidate() {
        fullUi(ctx)?.tooltip.invalidate();
      },
      numbers,
    };
    features.set(ctx, api);

    const installed = new WeakSet<object>();
    const ensureUi = (): void => {
      const ui = fullUi(ctx);
      if (!ui || installed.has(ui)) return;
      installed.add(ui);
      ui.tooltip.registerComposers({ item: composer });
      ui.tooltip.setNumbersProvider(numbers);
    };
    ctx.tickSystems.register('tooltip.ports', ensureUi);
    ctx.machine.subscribe(() => {
      ensureUi();
    });
    ensureUi();

    // anything a composed line depends on changed: the host forgets its cache
    options.subscribe((s, prev) => {
      if (s.ui.tooltipColorSet !== prev.ui.tooltipColorSet) api.invalidate();
    });
    // a bundle arrived (texts, items): the composed tooltips are stale; the data index may come after the boot, so it is watched
    // from the tick (once per index)
    let watched: object | null = null;
    const watchData = (): void => {
      const data = ctx.data as Partial<ClientDataIndex> | null | undefined;
      if (!data || data === watched || typeof data.onBundle !== 'function') return;
      watched = data;
      data.onBundle(() => {
        api.invalidate();
      });
    };
    ctx.tickSystems.register('tooltip.data', watchData);
    watchData();
    addProbeMember(ctx, 'tooltip', () => ({
      shop: shop?.context ?? null,
      inspected: inspected !== null,
      stats: fullUi(ctx)?.tooltip.stats ?? null,
      text: fullUi(ctx)?.tooltip.text() ?? null,
    }));
  },
};
