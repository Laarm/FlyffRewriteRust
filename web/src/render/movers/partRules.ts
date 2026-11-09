// Player (and mover) part assembly (Plan/41 section 12.3, WP-REND-05; `CMover::UpdateParts`, MoverEquip.cpp:279-456;
// `CMover::OverCoatItemRenderCheck`, Mover.cpp:9294-9420; the cap/hat block of `CMover::Render`,
// MoverRender.cpp:413-635; ref/18 section 11). A PURE function of the visual state and the item registry: which
// elements a mover shows, which model file each one loads, where weapons hang and which elements are hidden.
//
// Transcription notes (all KEEP; the order of the branches is the source's):
//  * hair `Part_{sex}Hair%02d` and head `Part_{sex}Head%02d` first, then the slots 0..30 except HEAD/HAIR twice: the
//    first pass applies the `dwExclusive` of every worn (non-expired) item (the named part is taken off), the second
//    loads the item models and the default bodies (`Part_{sex}Upper/Lower/Hand/Foot`) of EMPTY, non-exclusive body
//    slots. An item in an exclusive-removed slot is still loaded by the second pass (the C++ loads before it checks
//    `abExclusiveParts`), it just is not parented.
//  * an expired item (`flag & 1`) counts as empty for the load, but the cap/hat render rules look at the equipped item
//    itself (an expired cap still hides its base part: MoverRender.cpp:431-433).
//  * `OverCoatItemRenderCheck` runs first (fashion pairs CAP/HAT, UPPER/CLOTH, HAND/GLOVE, FOOT/BOOTS,
//    CLOAK/CLOAK2), then the cap/hat block, then the mask rule: the later `SetEffect`/`SetEffectOff` wins. Because
//    every operation is absolute (set or clear), the result of one pass from freshly loaded elements equals the
//    steady state of the original, which kept the flags from frame to frame (FIX: no stale flag survives an
//    equipment change, the assembly is a function of the state).
//  * the own (active) player and the other movers use different branches of the cap/hat block (inventory vs the
//    `m_aEquipInfo` table): `activeMover` selects them.

import {
  MAX_HUMAN_PARTS,
  PARTS_BOOTS,
  PARTS_CAP,
  PARTS_CLOAK,
  PARTS_CLOAK2,
  PARTS_CLOTH,
  PARTS_FOOT,
  PARTS_GLOVE,
  PARTS_HAIR,
  PARTS_HAND,
  PARTS_HAT,
  PARTS_HEAD,
  PARTS_LOWER_BODY,
  PARTS_LWEAPON,
  PARTS_MASK,
  PARTS_RWEAPON,
  PARTS_SHIELD,
  PARTS_UPPER_BODY,
} from '../../gen/defs/neuz';
import {
  LEFT_WEAPON_MOUNT,
  rightWeaponMounts,
  SHIELD_MOUNT,
  type AttachPoint,
} from './attachments';
import {
  defaultBodyFile,
  hairFileName,
  headFileName,
  isNullId,
  NULL_ID,
  type ItemVisualProp,
} from './registry';

/** `CItemElem::expired` (bit 0 of `byFlag`). */
export const ITEM_FLAG_EXPIRED = 0x01;
/** `MODE_OPTION_DONT_RENDER_MASK`: the mover does not show its mask (the "show mask" option is off). */
export const MODE_OPTION_DONT_RENDER_MASK = 0x0100_0000;
/** `TRANSPARENT_MODE` (GM invisibility). */
export const MODE_TRANSPARENT = 0x2;

/** One equipment slot of a mover (`EQUIP_INFO`). */
export interface EquipEntry {
  readonly itemId: number;
  /** `byFlag` (bit 0 = expired). */
  readonly flag: number;
  /** `nOption` (the element's attribute option; not used by the part rules). */
  readonly option: number;
}

/** The fashion pairs of `OverCoatItemRenderCheck`: armour part, fashion (over-coat) part. */
export const OVERCOAT_PAIRS: readonly (readonly [number, number])[] = [
  [PARTS_CAP, PARTS_HAT],
  [PARTS_UPPER_BODY, PARTS_CLOTH],
  [PARTS_HAND, PARTS_GLOVE],
  [PARTS_FOOT, PARTS_BOOTS],
  [PARTS_CLOAK, PARTS_CLOAK2],
];

/** What the rules need of the item registry (`VisualRegistry` implements it; tests use a fake). */
export interface PartRegistry {
  item(id: number): ItemVisualProp | undefined;
  /** The `.o3d` an equipped item loads for `sex` (`null` = none known). */
  itemFile(id: number, sex: 0 | 1): string | null;
}

/** The inputs of one assembly (the part of `MoverVisualState` the rules read). */
export interface AssemblyInput {
  readonly sex: 0 | 1;
  /** `MoverProp.bIfParts`: humanoid movers are assembled from parts, the others only load weapons and the shield. */
  readonly ifParts: boolean;
  readonly hairMesh: number;
  readonly headMesh: number;
  /** Equipment by `PARTS_*` slot. */
  readonly equip: ReadonlyMap<number, EquipEntry>;
  /** `[HAT, CLOTH, GLOVE, BOOTS, CLOAK]`: show the armour instead of the fashion item (`features.hide_fashion`). */
  readonly hideCoat: readonly boolean[];
  /** `m_dwMode`. */
  readonly mode: number;
  /** Disguised players show no weapons (the model itself is the disguise mover's). */
  readonly disguised: boolean;
  /** The own player: its cap/hat block reads the inventory (differs from the others', see the file comment). */
  readonly activeMover: boolean;
}

export type ElementRole = 'hair' | 'head' | 'body' | 'item' | 'weapon';

/** One element (`O3D_ELEMENT`) of the assembled model. */
export interface PartElement {
  /** The `PARTS_*` slot the element occupies. */
  readonly slot: number;
  readonly role: ElementRole;
  /** The `.o3d` it loads (`null` when the registry has no file: shows nothing). */
  readonly file: string | null;
  /** The equipped item this element shows (`null` for hair, head and the default bodies). */
  readonly itemId: number | null;
  /** The attachment point of a weapon / shield (`null` = not parented, drawn at the mover's world matrix). */
  readonly mount: AttachPoint | null;
  /** The element shows the item of ANOTHER slot (a yo-yo's second copy in the LWEAPON slot). */
  readonly copyOfSlot: number | null;
  /** The body-part texture that follows the skin set (`part_{sex}upper/lower`). */
  readonly skinTexture: boolean;
  /** `XE_REFLECT` of the element (a shield with `nReflect > 0`). */
  readonly reflect: boolean;
  /** `XE_HIDE`: the element is not drawn. */
  hidden: boolean;
}

/** The result of an assembly. */
export interface Assembly {
  /** Loaded elements in load order (one per slot). */
  readonly elements: readonly PartElement[];
  /** The elements that are drawn (not hidden). */
  visible(): PartElement[];
  bySlot(slot: number): PartElement | undefined;
  /** The slots of the visible elements, ascending. */
  visibleSlots(): number[];
}

const present = (e: EquipEntry | undefined): e is EquipEntry =>
  e !== undefined && !isNullId(e.itemId);
const expired = (e: EquipEntry): boolean => (e.flag & ITEM_FLAG_EXPIRED) !== 0;

/** Runs `UpdateParts` + the render-time visibility rules for one mover. */
export function assembleParts(input: AssemblyInput, reg: PartRegistry): Assembly {
  const { sex, ifParts } = input;
  const loaded = new Map<number, PartElement>();
  const order: number[] = [];
  const load = (el: PartElement): void => {
    if (!loaded.has(el.slot)) order.push(el.slot);
    loaded.set(el.slot, el);
  };
  const takeOff = (slot: number): void => {
    loaded.delete(slot);
    const at = order.indexOf(slot);
    if (at >= 0) order.splice(at, 1);
  };
  const setMount = (slot: number, mount: AttachPoint | null): void => {
    const e = loaded.get(slot);
    if (e) loaded.set(slot, { ...e, mount });
  };

  /** The worn, NON-expired item of a slot (`null` when empty, expired or unknown to the registry). */
  const effective = (slot: number): { entry: EquipEntry; prop: ItemVisualProp } | null => {
    const entry = input.equip.get(slot);
    if (!present(entry) || expired(entry)) return null;
    const prop = reg.item(entry.itemId);
    return prop ? { entry, prop } : null;
  };

  if (ifParts) {
    load({
      slot: PARTS_HAIR,
      role: 'hair',
      file: hairFileName(input.hairMesh, sex),
      itemId: null,
      mount: null,
      copyOfSlot: null,
      skinTexture: false,
      reflect: false,
      hidden: false,
    });
    load({
      slot: PARTS_HEAD,
      role: 'head',
      file: headFileName(input.headMesh, sex),
      itemId: null,
      mount: null,
      copyOfSlot: null,
      skinTexture: false,
      reflect: false,
      hidden: false,
    });
  }

  // pass 1: the exclusive parts of every worn item are taken off (and remembered)
  const exclusive = new Set<number>();
  for (let i = 0; i < MAX_HUMAN_PARTS; i++) {
    if (i === PARTS_HAIR || i === PARTS_HEAD) continue;
    const it = effective(i);
    if (it && !isNullId(it.prop.exclusive)) {
      takeOff(it.prop.exclusive);
      exclusive.add(it.prop.exclusive);
    }
  }

  // pass 2: the item models and the default bodies
  const isWeaponSlot = (i: number): boolean =>
    i === PARTS_RWEAPON || i === PARTS_LWEAPON || i === PARTS_SHIELD;
  for (let i = 0; i < MAX_HUMAN_PARTS; i++) {
    if (i === PARTS_HAIR || i === PARTS_HEAD) continue;
    const it = effective(i);
    if (it && (ifParts || isWeaponSlot(i))) {
      load({
        slot: i,
        role: isWeaponSlot(i) ? 'weapon' : 'item',
        file: reg.itemFile(it.entry.itemId, sex),
        itemId: it.entry.itemId,
        mount: null,
        copyOfSlot: null,
        skinTexture: false,
        reflect: false,
        hidden: false,
      });
    }
    if (exclusive.has(i)) continue;
    switch (i) {
      case PARTS_RWEAPON:
        if (it && !input.disguised) {
          for (const m of rightWeaponMounts(it.prop.kind3)) {
            if (m.slot === PARTS_RWEAPON) {
              setMount(PARTS_RWEAPON, m.point);
            } else {
              // a yo-yo's second copy: the same model in the LWEAPON slot on the left hand
              load({
                slot: m.slot,
                role: 'weapon',
                file: reg.itemFile(it.entry.itemId, sex),
                itemId: it.entry.itemId,
                mount: m.point,
                copyOfSlot: m.copyOf ?? null,
                skinTexture: false,
                reflect: false,
                hidden: false,
              });
            }
          }
        } else {
          takeOff(i);
        }
        break;
      case PARTS_LWEAPON:
        if (it && !input.disguised) setMount(i, LEFT_WEAPON_MOUNT.point);
        else takeOff(i);
        break;
      case PARTS_SHIELD:
        if (it && !input.disguised) {
          setMount(i, SHIELD_MOUNT.point);
          if (it.prop.reflect > 0) {
            // `SetEffect(PARTS_SHIELD, XE_REFLECT)` of CMover::Render
            const e = loaded.get(i);
            if (e) loaded.set(i, { ...e, reflect: true });
          }
        } else {
          takeOff(i);
        }
        break;
      case PARTS_UPPER_BODY:
      case PARTS_LOWER_BODY:
      case PARTS_HAND:
      case PARTS_FOOT:
        if (ifParts && !it) {
          const kind =
            i === PARTS_UPPER_BODY
              ? 'upper'
              : i === PARTS_LOWER_BODY
                ? 'lower'
                : i === PARTS_HAND
                  ? 'hand'
                  : 'foot';
          load({
            slot: i,
            role: 'body',
            file: defaultBodyFile(kind, sex),
            itemId: null,
            mount: null,
            copyOfSlot: null,
            skinTexture: kind === 'upper' || kind === 'lower',
            reflect: false,
            hidden: false,
          });
        }
        break;
      default:
        if (ifParts && !it) takeOff(i);
    }
  }

  // render-time visibility
  const hide = new Set<number>();
  applyOverCoat(input, reg, loaded, hide);
  applyCapRules(input, reg, hide);
  if ((input.mode & MODE_OPTION_DONT_RENDER_MASK) !== 0) hide.add(PARTS_MASK);
  else hide.delete(PARTS_MASK);

  const elements = order.map((slot) => {
    const e = loaded.get(slot);
    return e ? { ...e, hidden: hide.has(slot) } : null;
  });
  const list = elements.filter((e): e is PartElement => e !== null);
  return {
    elements: list,
    visible: () => list.filter((e) => !e.hidden),
    bySlot: (slot) => list.find((e) => e.slot === slot),
    visibleSlots: () =>
      list
        .filter((e) => !e.hidden)
        .map((e) => e.slot)
        .sort((a, b) => a - b),
  };
}

/** `OverCoatItemRenderCheck`: fashion items hide the armour part of their pair unless `hideCoat[pair]`. */
function applyOverCoat(
  input: AssemblyInput,
  reg: PartRegistry,
  loaded: ReadonlyMap<number, PartElement>,
  hide: Set<number>,
): void {
  const setHide = (flag: boolean, slot: number): void => {
    if (flag) hide.add(slot);
    else hide.delete(slot);
  };
  for (let j = 0; j < OVERCOAT_PAIRS.length; j++) {
    const [armour, fashion] = OVERCOAT_PAIRS[j] ?? [0, 0];
    if (!loaded.has(armour)) continue; // `GetParts` is NULL
    if (j === 4) setHide(input.hideCoat[4] === true, armour); // __HIDE_FASHION: the cloak follows its option
    if (!loaded.has(fashion)) continue; // no fashion element: the pair is left alone
    // (an expired fashion item is not loaded at all, so the `expired` branches of the source cannot fire here)
    if (j < 4 && input.hideCoat[j] === true) {
      const worn = present(input.equip.get(armour));
      if (worn) {
        hide.delete(armour);
        hide.add(fashion);
        if (j === 0) hideCapBase(input, reg, hide);
      } else {
        hide.add(armour);
        hide.delete(fashion);
      }
    } else {
      hide.add(armour);
      hide.delete(fashion);
      if (j === 0) hideCapBase(input, reg, hide);
    }
  }
}

/** `SetEffect(cap.dwBasePartsIgnore, XE_HIDE)` of the over-coat check (active: inventory cap, others: equip info). */
function hideCapBase(input: AssemblyInput, reg: PartRegistry, hide: Set<number>): void {
  const cap = input.equip.get(PARTS_CAP);
  if (!present(cap)) return;
  const prop = reg.item(cap.itemId);
  if (prop && !isNullId(prop.basePartsIgnore)) hide.add(prop.basePartsIgnore);
}

/** The cap/hat block of `CMover::Render`: does the helmet (or the fashion hat) hide the head, the hair, ... */
function applyCapRules(input: AssemblyInput, reg: PartRegistry, hide: Set<number>): void {
  const cap = input.equip.get(PARTS_CAP);
  const hatEntry = input.equip.get(PARTS_HAT);
  const hideHat = input.hideCoat[0] === true;
  const capProp = present(cap) ? reg.item(cap.itemId) : undefined;
  const hatProp = present(hatEntry) ? reg.item(hatEntry.itemId) : undefined;
  const hatBase = hatProp && !isNullId(hatProp.basePartsIgnore) ? hatProp.basePartsIgnore : null;
  const capBase = capProp && !isNullId(capProp.basePartsIgnore) ? capProp.basePartsIgnore : null;
  const hideHatBase = (base: number): void => {
    if (base === PARTS_HEAD) hide.add(PARTS_HAIR);
    hide.add(base);
  };

  if (input.activeMover) {
    // the inventory branch
    if (present(cap)) {
      if (capBase !== null) hide.add(capBase);
      if (present(hatEntry) && !hideHat && !expired(hatEntry)) {
        if (hatBase !== null) hideHatBase(hatBase);
        else hide.delete(PARTS_HAIR);
      }
    } else if (present(hatEntry) && !expired(hatEntry)) {
      // no cap: the over-coat hat decides (the hide-coat option is not consulted here)
      if (hatBase !== null) hideHatBase(hatBase);
    }
    return;
  }

  // the `m_aEquipInfo` branch (every other mover)
  if (present(cap)) {
    if (capBase !== null) hide.add(capBase);
    if (present(hatEntry) && !expired(hatEntry)) {
      if (!hideHat) {
        if (hatBase !== null) hideHatBase(hatBase);
        else if (hatProp) hide.delete(PARTS_HAIR);
      } else if (capBase !== null) {
        hideHatBase(capBase);
      }
    }
  } else if (present(hatEntry) && !hideHat && !expired(hatEntry)) {
    if (hatBase !== null) hideHatBase(hatBase);
    else if (hatProp) hide.add(PARTS_HAIR);
  }
}

/** Convenience: the equipment table of a list of `(slot, itemId)` pairs (tests, dev pages). */
export function equipOf(
  entries: readonly (readonly [slot: number, itemId: number, flag?: number])[],
): Map<number, EquipEntry> {
  const m = new Map<number, EquipEntry>();
  for (const [slot, itemId, flag] of entries) m.set(slot, { itemId, flag: flag ?? 0, option: 0 });
  return m;
}

/** An empty equipment table entry (`dwId == NULL_ID`). */
export const NO_ITEM: EquipEntry = { itemId: NULL_ID, flag: 0, option: 0 };
