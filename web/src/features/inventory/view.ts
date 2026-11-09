// The read model of the inventory window (`InventoryView` of `ui/windows/char/ports.ts`, Plan/45 §9.1; WP-CFEAT-10): the
// store's container turned into the 168 bag cells and 31 equipment cells the shell draws. Item views keep their identity
// while their item, their cooldown and the item bundle do not change (the shell caches its icon cells per object).

import { HD_TWO } from '../../gen/defs/attribute';
import { IK2_FOOD, IK2_SKILL, IK3_PILL } from '../../gen/defs/itemkind';
import { MAX_HUMAN_PARTS } from '../../gen/defs/neuz';
import type { ClientDataIndex } from '../../data';
import type { InventoryItem, InventoryView } from '../../ui/windows/char/ports';
import { itemIconPath } from '../common/itemIcon';
import { isExpired, NULL_ID, partsOf } from './info';
import { ITEM_FLAG, type InvItem } from './items';
import { cooldownGroup, type GroupCooldown, type InventoryState } from './state';

/** `MODE_OPTION_DONT_RENDER_MASK` of the mode word: the own mask item is hidden from others. */
export const MODE_OPTION_DONT_RENDER_MASK = 0x0100_0000;

/** An inventory item as the shell and the tooltip composers see it: the display fields plus the full record. */
export interface InvItemView extends InventoryItem {
  readonly item: InvItem;
}

/** What the view needs besides the container. */
export interface InventoryViewEnv {
  data(): Pick<ClientDataIndex, 'item' | 'version'> | null | undefined;
  /** `features.inventory_sorting`, `hide_fashion`, `new_font_color`. */
  features(): { sorting: boolean; hideFashion: boolean; newFontColor: boolean };
  /** The purse (`GetGold()`). */
  gold(): number;
  /** The mode word (`MODE_OPTION_DONT_RENDER_MASK` is the inverse of `g_Option.m_bViewMask`). */
  modeFlags(): number;
  /** The paper doll's subject (the own `MoverVisualState`). */
  subject?(): unknown;
}

interface Cached {
  readonly cd: GroupCooldown | null;
  readonly view: InvItemView;
}

/** Builds `InventoryView`s from the store state; remembers the item views of the last builds. */
export class InventoryViewBuilder {
  #cache = new WeakMap<InvItem, Cached>();
  #dataVersion = -1;
  #last: InventoryView | null = null;
  #lastKey: readonly unknown[] = [];

  constructor(private readonly env: InventoryViewEnv) {}

  #itemView(it: InvItem, state: InventoryState): InvItemView {
    const data = this.env.data();
    const prop = data?.item(it.itemId);
    const group = prop
      ? cooldownGroup(
          prop.item_kind2,
          prop.item_kind3,
          prop.skill_ready,
          IK2_FOOD,
          IK2_SKILL,
          IK3_PILL,
        )
      : 0;
    const cd = group > 0 ? (state.cooldowns[group - 1] ?? null) : null;
    const hit = this.#cache.get(it);
    if (hit) {
      if (hit.cd === cd) return hit.view;
    }
    const view: InvItemView = {
      name: prop?.name ?? `Item ${String(it.itemId)}`,
      kind2: prop?.item_kind2 ?? 0,
      parts: partsOf(prop),
      serial: it.serial,
      objId: it.objId,
      itemId: it.itemId,
      icon: itemIconPath(prop, it),
      count: it.num,
      stackable: (prop?.pack_max ?? 1) > 1,
      extra: it.extra,
      using: (it.flag & ITEM_FLAG.ISUSING) !== 0,
      expired: isExpired(it),
      cooldown: cd,
      twoHanded: prop?.handed === HD_TWO,
      wearable: prop !== undefined && prop.parts !== NULL_ID,
      item: it,
    };
    this.#cache.set(it, { cd, view });
    return view;
  }

  /** The view for `state`; the same object while nothing the view shows changed. */
  build(state: InventoryState): InventoryView {
    const data = this.env.data();
    const f = this.env.features();
    const gold = this.env.gold();
    const mask = (this.env.modeFlags() & MODE_OPTION_DONT_RENDER_MASK) === 0;
    const key = [
      state.version,
      data?.version ?? -1,
      f.sorting,
      f.hideFashion,
      f.newFontColor,
      gold,
      mask,
      state.hideCoat,
      this.env.subject?.(),
    ];
    if (
      this.#last &&
      key.length === this.#lastKey.length &&
      key.every((v, i) => Object.is(v, this.#lastKey[i]))
    ) {
      return this.#last;
    }
    if ((data?.version ?? -1) !== this.#dataVersion) {
      this.#dataVersion = data?.version ?? -1;
      this.#cache = new WeakMap();
    }
    const c = state.container;
    const bag: (InventoryItem | null)[] = [];
    for (let slot = 0; slot < c.indexNum; slot += 1) {
      const it = c.getAtSlot(slot);
      bag.push(it ? this.#itemView(it, state) : null);
    }
    const equip: (InventoryItem | null)[] = [];
    for (let part = 0; part < MAX_HUMAN_PARTS; part += 1) {
      const it = c.getEquip(part);
      equip.push(it ? this.#itemView(it, state) : null);
    }
    const subject = this.env.subject?.();
    const view: InventoryView = {
      bag,
      equip,
      gold,
      hideCoat: state.hideCoat,
      viewMask: mask,
      sorting: f.sorting,
      hideFashion: f.hideFashion,
      newFontColor: f.newFontColor,
      version: state.equipVersion,
      upgradeMode: false,
      ...(subject === undefined ? {} : { subject }),
    };
    this.#last = view;
    this.#lastKey = key;
    return view;
  }
}
