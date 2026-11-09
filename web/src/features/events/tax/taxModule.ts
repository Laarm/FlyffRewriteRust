// The `tax` feature (Plan/45 §28.3; WP-CFEAT-52; 22 §10.5; ref/09b §3.16, §7.9; `src:Neuz/DPClient.cpp:17856-17888`, `_Common/Tax.cpp`): the tax tables of the continents and the player's own tax area.
//
//   TaxAllInfo                  replaces `eventStore.tax` (East 15, West 240, All 255; owner `0xFFFFFFFF` = none) and closes the rate window (715)
//   TaxSetTaxRateOpenWnd        opens the rate window (715) for that continent with the CURRENT sales and purchase rates and the presets `max( current, min )`
//   taxOf( ctx )                `area()` (the tax area of the player), `rate( cont, kind )` (the integer percent), `purchaseRate()` / `salesRate()` (the f32 fraction of the player's area,
//                               bit-exact with the server), `applies()`, `ownerOf( cont )`, `subscribe( fn )`, `openCheck()` (menu 89, the check window)
//   shop                        `features/npc/shop` reads the rates for the prices and the tooltip (the occupation refusal is the server's)
//
// The slice is reset when the character leaves the world (not on a world change or a reconnect).

import type { ClientContext } from '../../../app/context';
import { APP_SECRETROOM_TAXRATE_CHANGE } from '../../../gen/defs/resdata';
import type { ContinentDef } from '../../../gen/bundles';
import { fullUi, type FeatureContext } from '../../common/module';
import { addProbeMember } from '../../common/probe';
import { playerDataOf } from '../../common/playerData';
import { derivedView } from '../../common/view';
import { guildOf } from '../../guild/api';
import { type ContinentPoly, continentPolys } from '../../navigator/continent';
import { type EventFeature, eventOf } from '../api';
import { registerEvents } from '../eventsModule';
import { helpPanelsKey } from './panels';
import { withHelpPanels } from '../helpPanels';
import { type EventStore, initialTax } from '../store';
import { taxAreaOf, CONT_NODATA } from './area';
import { ownerOf, ratePreset, rateOf, sliceOfAllInfo, taxFraction, TAX_KIND } from './rates';
import { taxSenders, type TaxSenders } from './senders';

/** The public face of the tax feature of a context. */
export interface TaxFeature {
  readonly store: EventStore;
  readonly senders: TaxSenders;
  /** The tax area of the player: 15 East, 240 West, 0 none (`CTax::GetContinent( g_pPlayer )`). */
  area(): number;
  /** The integer percent of a kind of a continent (0 when absent). */
  rate(cont: number, kind: number): number;
  /** The integer percent of the player's area (0 for none). */
  salesPercent(): number;
  purchasePercent(): number;
  /** The f32 fractions the prices use (`Math.fround( Math.fround( rate * 0.1f ) * 0.1f )`), 0 for no area. */
  salesRate(): number;
  purchaseRate(): number;
  /** The player stands in a tax area (`IsApplyTaxRate`'s area half; the item half is `isTaxableItem`). */
  applies(): boolean;
  /** The owner guild of a continent, `null` for none. */
  ownerOf(cont: number): number | null;
  /** Called when the tax slice or the player's area changes. */
  subscribe(fn: () => void): () => void;
  /** `MMI_SECRET_TAXRATES_CHECK`: the check window of the player's area. */
  openCheck(): void;
}

const features = new WeakMap<ClientContext, TaxFeature>();

/** The tax feature of a context; `undefined` before the module registered. */
export function taxOf(ctx: ClientContext): TaxFeature | undefined {
  return features.get(ctx);
}

/** Registers the tax (after the events feature). */
export function registerTax(fc: FeatureContext): TaxFeature {
  const ctx = fc.app;
  const { selfStore } = ctx.stores;
  const events: EventFeature = eventOf(ctx) ?? registerEvents(fc);
  const store = events.store;
  const senders = taxSenders((msg) => ctx.net.sender.send(msg));
  const playerData = playerDataOf(ctx);

  // ---- the area of the player ---------------------------------------------------------------------------------------------------

  let polyDefs: readonly ContinentDef[] | undefined;
  let polys: readonly ContinentPoly[] = [];
  const madrigalPolys = (): readonly ContinentPoly[] => {
    const defs = ctx.data?.worldFolder('wdmadrigal')?.continents;
    if (defs !== polyDefs) {
      polyDefs = defs;
      polys = defs ? continentPolys(defs) : [];
    }
    return polys;
  };
  const area = (): number => {
    const s = selfStore.getState();
    return taxAreaOf(s.worldId, s.pos[0], s.pos[2], madrigalPolys());
  };
  const rate = (cont: number, kind: number): number => rateOf(store.getState().tax, cont, kind);
  const percent = (kind: number): number => {
    const a = area();
    return a === CONT_NODATA ? 0 : rate(a, kind);
  };

  // ---- the windows -------------------------------------------------------------------------------------------------------------

  const openChange = (continent: number, min: number, max: number): void => {
    const salesNow = rate(continent, TAX_KIND.Sales);
    // `__TAX_FIX` is defined: the purchase default is the purchase rate
    const purchaseNow = rate(continent, TAX_KIND.Purchase);
    withHelpPanels(ctx, (panels) => {
      fullUi(ctx)?.secretRoom.openChangeTaxRate({
        continent,
        salesNow,
        purchaseNow,
        salesPreset: ratePreset(salesNow, min),
        purchasePreset: ratePreset(purchaseNow, min),
        min,
        max,
        info: panels[helpPanelsKey.changeTaxRate] ?? '',
        setRate: (cont, sales, purchase) => {
          senders.taxRate(cont, sales, purchase);
        },
      });
    });
  };

  const openCheck = (): void => {
    const owner = ownerOf(store.getState().tax, area());
    const g = guildOf(ctx);
    const guild = owner === null ? null : (g?.nameOf(owner) ?? null);
    const masterId = owner === null ? 0 : (g?.briefOf(owner)?.idMaster ?? 0);
    const master = derivedView<string | null>(
      () => {
        if (guild === null || masterId === 0) return null;
        // `GetPlayerString( idMaster )`: asks the cache once, the view follows it
        playerData.need(masterId);
        return playerData.nameOf(masterId);
      },
      [playerData.store],
      (a, b) => a === b,
    );
    const a = area();
    withHelpPanels(ctx, (panels) => {
      fullUi(ctx)?.secretRoom.openCheckTaxRate({
        salesRate: a === CONT_NODATA ? 0 : rate(a, TAX_KIND.Sales),
        purchaseRate: a === CONT_NODATA ? 0 : rate(a, TAX_KIND.Purchase),
        guild,
        master,
        info: panels[helpPanelsKey.checkTaxRate] ?? '',
      });
    });
  };

  // ---- the messages ------------------------------------------------------------------------------------------------------------

  fc.net.on('taxAllInfo', (m) => {
    store.setState({ tax: sliceOfAllInfo(m) });
    // `TAX_ALLINFO` destroys the rate window (the later rates are the new ones)
    fullUi(ctx)?.secretRoom.close(APP_SECRETROOM_TAXRATE_CHANGE);
  });
  fc.net.on('taxSetTaxRateOpenWnd', (m) => {
    openChange(m.continent, m.minRate, m.maxRate);
  });

  // the slice of a character that left the world goes with it; a world change keeps it, the join batch sends it again
  ctx.machine.subscribe((screen, previous) => {
    if (previous.s !== 'InGame') return;
    if (screen.s === 'InGame' || screen.s === 'Reconnecting') return;
    if (screen.s === 'Loading' || screen.s === 'Entering') return;
    store.setState({ tax: initialTax() });
  });

  // ---- the public face ------------------------------------------------------------------------------------------------------------

  const feature: TaxFeature = {
    store,
    senders,
    area,
    rate,
    salesPercent: () => percent(TAX_KIND.Sales),
    purchasePercent: () => percent(TAX_KIND.Purchase),
    salesRate: () => taxFraction(percent(TAX_KIND.Sales)),
    purchaseRate: () => taxFraction(percent(TAX_KIND.Purchase)),
    applies: () => area() !== CONT_NODATA,
    ownerOf: (cont) => ownerOf(store.getState().tax, cont),
    subscribe(fn) {
      let key = `${String(area())}|${String(percent(TAX_KIND.Sales))}|${String(percent(TAX_KIND.Purchase))}`;
      const check = (): void => {
        const next = `${String(area())}|${String(percent(TAX_KIND.Sales))}|${String(percent(TAX_KIND.Purchase))}`;
        if (next === key) return;
        key = next;
        fn();
      };
      const a = store.subscribe(check);
      const b = selfStore.subscribe(check);
      return () => {
        a();
        b();
      };
    },
    openCheck,
  };
  features.set(ctx, feature);

  addProbeMember(ctx, 'tax', () => ({
    continents: [...store.getState().tax.continents].map((c) => ({
      cont: c.cont,
      owner: c.owner,
      rates: [...c.rates],
    })),
    area: area(),
    purchaseRate: feature.purchaseRate(),
    salesRate: feature.salesRate(),
  }));
  return feature;
}
