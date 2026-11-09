// The `appearance` feature module (Plan/45 §17, §18.4; WP-CFEAT-15, WP-UI-35): the appearance flows the item dispatch (rules R14, R20, R25) asks for
// through the optional members of `DispatchWindows`, over the windows of `ui.appearance` and the beauty shops of the `npc` feature:
//
//   beautyCoupon  R14: the hair coupon (`II_SYS_SYS_SCR_HAIRCHANGE`) and the make-up coupon (`II_SYS_SYS_SCR_FACEOFFFREE`): "You have a Free Coupon.
//                 Do you want to use it now?"; Yes opens the hair or face shop in coupon mode (`beautyOf(ctx).askCoupon`), where the change is
//                 free and `SetHairReq` / `ChangeFaceReq` carry `use_coupon`
//   changeSex     R20: the scroll of sex change (`II_CHR_SYS_SCR_TRANSY`): `APP_CHANGESEX` with a 3-D preview of the other sex and five faces;
//                 OK sends `DoUseItem(scroll, target, face)`; the answers `SexChange` and `ChangeFaceNtf` are the NPC feature's reducers
//   itemTransy    R25: the scrolls of item transy (`II_CHR_SYS_SCR_ITEMTRANSY_A` for armour of level 1-60, `_B` for 61-120): `APP_ITEM_TRANSY`;
//                 an armour dropped on the first slot finds its opposite-gender twin (`GetTransyItem`, `twin.ts`), OK sends `ItemTransy`
//   openChangeFace  `APP_CHANGEFACE`: the five-face window of the original, which no menu or item opens in the shipped client; OK sends the chat
//                 command `/ChangeFace <n>` (the server's command table has it commented out)
//
// The penya path of the transy window (`MMI_ITEM_TRANSY`, `bMenu`) is compiled out on the server (`cash = false` is ignored), so no menu opens it.

import type { ClientContext } from '../../app/context';
import type { MoverVisualState } from '../../game/entities';
import { II_CHR_SYS_SCR_ITEMTRANSY_A } from '../../gen/defs/item';
import { MI_FEMALE, MI_MALE } from '../../gen/defs/obj';
import { APP_BEAUTY_SHOP_EX } from '../../gen/defs/resdata';
import {
  TID_GAME_EQUIPPUT,
  TID_GAME_ITEM_TRANSY,
  TID_GAME_ITEM_TRANSY_NOT_LEVEL_0,
  TID_GAME_ITEM_TRANSY_NOT_LEVEL_1,
  TID_GAME_TRANSITEM_PUT,
  TID_GAME_TRANSITEM_PUT1,
} from '../../gen/defs/text';
import type { ChangeSexPreview, TransyPair } from '../../ui/windows/appearance/ports';
import { previewPortOf } from '../charselect/previews';
import { chatOf } from '../chat';
import { type FeatureContext, type FeatureModule, fullUi } from '../common/module';
import { addProbeMember } from '../common/probe';
import { sysChat } from '../common/text';
import { inventoryOf } from '../inventory';
import type { DispatchWindows } from '../inventory/dispatch/env';
import { HD_TWO } from '../../gen/defs/attribute';
import { partsOf } from '../inventory/info';
import type { InvItem } from '../inventory/items';
import type { InvItemView } from '../inventory/view';
import { beautyOf } from '../npc';
import { createAppearanceSenders, type AppearanceSenders } from './senders';
import { findTransyTwin, setMemberIds } from './twin';

/** `II_CHR_SYS_SCR_ITEMTRANSY_A` takes armour up to level 60, `_B` from 61 (`dwLimitLevel1`). */
const TRANSY_LEVEL_SPLIT = 61;

type AppearanceWindowKey = 'beautyCoupon' | 'changeSex' | 'itemTransy';

export interface AppearanceFeature {
  readonly senders: AppearanceSenders;
  /** The windows this module registered with the dispatch (the tests drive them directly). */
  readonly windows: {
    readonly [K in AppearanceWindowKey]-?: NonNullable<DispatchWindows[K]>;
  };
  /** Opens `APP_CHANGEFACE`; `false` without a UI. */
  openChangeFace(): boolean;
}

const features = new WeakMap<ClientContext, AppearanceFeature>();

/** The appearance feature of a context; `undefined` before the module registered. */
export function appearanceOf(ctx: ClientContext): AppearanceFeature | undefined {
  return features.get(ctx);
}

export const appearanceModule: FeatureModule = {
  id: 'appearance',
  milestone: 4,
  register(fc) {
    registerAppearance(fc);
  },
};

function registerAppearance(fc: FeatureContext): AppearanceFeature | undefined {
  const ctx = fc.app;
  const inv = inventoryOf(ctx);
  if (!inv) return undefined;
  const senders = createAppearanceSenders(ctx);
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const say = (tid: number): void => {
    sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid);
  };
  const stats = { opened: {} as Record<string, number>, sent: 0, refused: 0 };
  const opened = (name: string): void => {
    stats.opened[name] = (stats.opened[name] ?? 0) + 1;
  };
  const fresh = (item: InvItem): InvItem | null => inv.item(item.objId);
  const reserve = (objId: number, on: boolean): void => {
    const it = inv.item(objId);
    if (!it) return;
    inv.setExtra(objId, on ? it.extra + 1 : 0);
  };

  // ---- R14: the hair and make-up coupons ----------------------------------------------------------------------------------------------

  const beautyCoupon: AppearanceFeature['windows']['beautyCoupon'] = (app) => {
    beautyOf(ctx)?.askCoupon(app === APP_BEAUTY_SHOP_EX ? 'hair' : 'face');
    opened('beautyCoupon');
  };

  // ---- R20: the scroll of sex change ---------------------------------------------------------------------------------------------------

  /**
   * The preview of the other sex: the own visual state with the sex turned (`CMover::UpdateParts( !sex, 0, 0, hair, face, equipInfo, ... )`).
   * KEEP-DEVIATION: the original's model has no hair colour (its ambient stays at the default); the preview keeps the player's, which is what
   * the server keeps (`sex_change` leaves the hair alone).
   */
  const sexPreview = (): ChangeSexPreview | null => {
    const port = previewPortOf(ctx);
    if (!port) return null;
    return {
      port,
      subject: (face) => {
        const v: MoverVisualState | undefined = ctx.entities.self()?.visual;
        if (!v) return null;
        const other = v.sex === 0 ? 1 : 0;
        return {
          ...v,
          sex: other,
          moverIndex: other === 1 ? MI_FEMALE : MI_MALE,
          skinSet: 0,
          headMesh: face,
        };
      },
    };
  };

  const changeSex: AppearanceFeature['windows']['changeSex'] = (item, target) => {
    fullUi(ctx)?.appearance.openChangeSex({
      preview: ctx.entities.self()?.visual ? sexPreview() : null,
      confirm: (face) => {
        const cur = fresh(item);
        // `SendDoUseItem( m_dwItemId, m_dwObjId, m_nSelectFace )`
        if (cur) inv.itemSenders.doUseItem(cur.objId, target, face);
        stats.sent += 1;
      },
    });
    opened('changeSex');
  };

  // ---- R25: the scrolls of item transy -----------------------------------------------------------------------------------------------

  /** The table in id order (the original walks `prj.m_aPropItem`). */
  const itemsInOrder = () => {
    const rows = ctx.data?.items()?.rows() ?? [];
    return [...rows].sort((a, b) => a.id - b.id);
  };
  const setIds = (): ReadonlySet<number> =>
    setMemberIds(ctx.data?.bundle('item-ext')?.set_items ?? []);

  /** `m_pItemElemChange`: the armour with the twin's id and durability (`m_nHitPoint = dwEndurance`). */
  const twinOf = (
    view: InvItemView,
    it: InvItem,
    twin: NonNullable<ReturnType<typeof findTransyTwin>>,
  ) => ({
    ...view,
    name: twin.name,
    kind2: twin.item_kind2,
    parts: partsOf(twin),
    // the composed tooltip is cached per serial: the twin is another item
    serial: `transy:${String(it.serial)}:${String(twin.id)}`,
    itemId: twin.id,
    icon: twin.icon !== '' ? `item/${twin.icon}` : null,
    twoHanded: twin.handed === HD_TWO,
    item: { ...it, itemId: twin.id, hitPoint: twin.endurance },
  });

  const itemTransy: AppearanceFeature['windows']['itemTransy'] = (item) => {
    const scrollItem = fresh(item);
    const scroll = scrollItem ? inv.itemView(scrollItem) : null;
    const ui = fullUi(ctx);
    if (!scroll || !ui) return;
    const scrollId = item.itemId;
    ui.appearance.openItemTransy({
      scroll,
      tips: { putItem: text(TID_GAME_TRANSITEM_PUT), putScroll: text(TID_GAME_TRANSITEM_PUT1) },
      refuse: (objId) => {
        // `OnDropIcon`: a worn item is refused with the line and the forbid cursor
        if (inv.state().container.isEquip(objId)) {
          say(TID_GAME_EQUIPPUT);
          stats.refused += 1;
          return true;
        }
        return inv.item(objId) === null;
      },
      accept: (objId): TransyPair | null => {
        const it = inv.item(objId);
        const view = it ? inv.itemView(it) : null;
        const prop = it ? ctx.data?.item(it.itemId) : undefined;
        if (!it || !view || !prop) return null;
        const twin = findTransyTwin(prop, itemsInOrder(), (id) => setIds().has(id));
        if (!twin) {
          say(TID_GAME_ITEM_TRANSY);
          stats.refused += 1;
          return null;
        }
        // the scroll fits one level band: `_A` 1-60, `_B` 61-120 (an item without a limit counts as high)
        const high = prop.limit_level1 >= TRANSY_LEVEL_SPLIT;
        if (scrollId === II_CHR_SYS_SCR_ITEMTRANSY_A ? high : !high) {
          const tid =
            scrollId === II_CHR_SYS_SCR_ITEMTRANSY_A
              ? TID_GAME_ITEM_TRANSY_NOT_LEVEL_0
              : TID_GAME_ITEM_TRANSY_NOT_LEVEL_1;
          const scrollName = ctx.data?.item(scrollId)?.name ?? '';
          sysChat(fc.texts, fullUi(ctx)?.chat ?? null, tid, [scrollName]);
          stats.refused += 1;
          return null;
        }
        return { source: view, twin: twinOf(view, it, twin), twinItemId: twin.id };
      },
      reserve,
      send: (armour, scrollObj, change) => {
        senders.itemTransy(armour, scrollObj, change);
        stats.sent += 1;
      },
    });
    opened('itemTransy');
  };

  const windows: AppearanceFeature['windows'] = { beautyCoupon, changeSex, itemTransy };
  inv.registerDispatchWindows(windows);

  const feature: AppearanceFeature = {
    senders,
    windows,
    openChangeFace() {
      const ui = fullUi(ctx);
      if (!ui) return false;
      ui.appearance.openChangeFace({
        confirm: (face) => {
          // legacy-send: SendChat
          chatOf(ctx)?.submit(`/ChangeFace ${String(face)}`);
          stats.sent += 1;
        },
      });
      opened('changeFace');
      return true;
    },
  };
  features.set(ctx, feature);
  addProbeMember(ctx, 'appearance', () => ({ stats: { ...stats, opened: { ...stats.opened } } }));
  return feature;
}
