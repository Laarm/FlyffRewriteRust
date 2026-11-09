// The grids of the Items tab (Plan/25 section 4.4): the inventory grid of 6 columns (its size is the CAPACITY of the snapshot, which
// follows `features.inventory_168`, `bigger_pocket` and `perm_extrabags`: the console never hard-codes it), the same grid for a pocket and
// a bank, and the equipment in a paper-doll arrangement.
import { useMemo } from 'react';
import {
  PARTS_BULLET,
  PARTS_CAP,
  PARTS_CAP2,
  PARTS_CLOAK,
  PARTS_CLOAK2,
  PARTS_CLOTH,
  PARTS_BOOTS,
  PARTS_EARRING1,
  PARTS_EARRING2,
  PARTS_FOOT,
  PARTS_FOOT2,
  PARTS_GLOVE,
  PARTS_HAND,
  PARTS_HAND2,
  PARTS_HAT,
  PARTS_LWEAPON,
  PARTS_MASK,
  PARTS_NECKLACE1,
  PARTS_RIDE,
  PARTS_RING1,
  PARTS_RING2,
  PARTS_RWEAPON,
  PARTS_SHIELD,
  PARTS_UPPER2,
  PARTS_UPPER_BODY,
} from '../../../gen/defs/neuz';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { matchesFilter, type CellFilter, type ContainerModel, isFiltering } from './containers';
import { infoOf, ItemCell } from './ItemCell';
import type { PlayerGame } from './gamedata';

export interface GridCommon {
  readonly game: PlayerGame['data'];
  readonly filter: CellFilter;
  /** Display slot -> flash token of the container (changed cells). */
  readonly flash?: ReadonlyMap<number, number> | undefined;
  readonly selected: number | null;
  /** Slots with a pending removal. */
  readonly pending?: ReadonlySet<number> | undefined;
  readonly onSelect: (slot: number) => void;
  readonly onOpen: (slot: number) => void;
}

function dimmed(common: GridCommon, elem: ItemElem | undefined): boolean {
  if (!elem || !isFiltering(common.filter)) return false;
  const prop = common.game?.byId.get(elem.itemId);
  return !matchesFilter(common.filter, elem, {
    name: infoOf(common.game, elem.itemId).name,
    kind1: prop?.kind1 ?? -1,
  });
}

/** A bag, a pocket or a bank: `capacity` cells in 6 columns. */
export function ItemGrid({
  model,
  common,
  scroll = false,
  label,
}: {
  readonly model: ContainerModel;
  readonly common: GridCommon;
  /** A tall grid scrolls (the bag); a pocket or a bank shows whole. */
  readonly scroll?: boolean;
  readonly label: string;
}) {
  const slots = useMemo(
    () => Array.from({ length: model.capacity }, (_, i) => i),
    [model.capacity],
  );
  return (
    <div
      className={scroll ? 'plr-grid plr-grid--bag' : 'plr-grid'}
      role="group"
      aria-label={label}
      data-capacity={model.capacity}
      style={{ ['--cols' as string]: 6 }}
    >
      {slots.map((slot) => {
        const elem = model.cells.get(slot);
        return (
          <ItemCell
            key={slot}
            elem={elem}
            slot={slot}
            game={common.game}
            selected={common.selected === slot}
            dim={dimmed(common, elem)}
            flash={common.flash?.get(slot)}
            pending={common.pending?.has(slot) ?? false}
            onSelect={common.onSelect}
            onOpen={common.onOpen}
          />
        );
      })}
    </div>
  );
}

/** The equipment slots in a paper-doll arrangement: groups of four, every part the character wears is shown. */
export interface DollGroup {
  readonly label: string;
  readonly parts: readonly (readonly [part: number, name: string])[];
}

export const DOLL_GROUPS: readonly DollGroup[] = [
  {
    label: 'Armor',
    parts: [
      [PARTS_CAP, 'Helmet'],
      [PARTS_UPPER_BODY, 'Suit'],
      [PARTS_HAND, 'Gauntlet'],
      [PARTS_FOOT, 'Boots'],
    ],
  },
  {
    label: 'Weapons and more',
    parts: [
      [PARTS_RWEAPON, 'Weapon'],
      [PARTS_LWEAPON, 'Left hand'],
      [PARTS_SHIELD, 'Shield'],
      [PARTS_BULLET, 'Ammo'],
      [PARTS_CLOAK, 'Cloak'],
      [PARTS_MASK, 'Mask'],
      [PARTS_RIDE, 'Board'],
    ],
  },
  {
    label: 'Accessories',
    parts: [
      [PARTS_NECKLACE1, 'Necklace'],
      [PARTS_RING1, 'Ring'],
      [PARTS_RING2, 'Ring'],
      [PARTS_EARRING1, 'Earring'],
      [PARTS_EARRING2, 'Earring'],
    ],
  },
  {
    label: 'Fashion',
    parts: [
      [PARTS_HAT, 'Hat'],
      [PARTS_CLOTH, 'Clothes'],
      [PARTS_GLOVE, 'Gloves'],
      [PARTS_BOOTS, 'Shoes'],
    ],
  },
  {
    label: 'Overcoat',
    parts: [
      [PARTS_CAP2, 'Helmet'],
      [PARTS_UPPER2, 'Suit'],
      [PARTS_HAND2, 'Gauntlet'],
      [PARTS_FOOT2, 'Boots'],
      [PARTS_CLOAK2, 'Cloak'],
    ],
  },
];

/** The parts the arrangement shows. */
const SHOWN = new Set(DOLL_GROUPS.flatMap((g) => g.parts.map(([p]) => p)));

/** The equipment container as a paper doll; parts outside the arrangement show in an extra group when occupied. */
export function PaperDoll({
  model,
  common,
}: {
  readonly model: ContainerModel;
  readonly common: GridCommon;
}) {
  const extra = [...model.cells.keys()].filter((p) => !SHOWN.has(p)).sort((a, b) => a - b);
  const groups: readonly DollGroup[] =
    extra.length === 0
      ? DOLL_GROUPS
      : [
          ...DOLL_GROUPS,
          { label: 'Other', parts: extra.map((p) => [p, `Part ${String(p)}`] as const) },
        ];
  return (
    <div className="plr-doll" role="group" aria-label="Equipped items">
      {groups.map((g) => (
        <DollGroupView key={g.label} group={g} model={model} common={common} />
      ))}
    </div>
  );
}

function DollGroupView({
  group,
  model,
  common,
}: {
  readonly group: DollGroup;
  readonly model: ContainerModel;
  readonly common: GridCommon;
}) {
  return (
    <>
      <div className="plr-doll__label">{group.label}</div>
      {group.parts.map(([part, name]) => {
        const elem = model.cells.get(part);
        return (
          <ItemCell
            key={part}
            elem={elem}
            slot={part}
            game={common.game}
            worn
            label={name}
            selected={common.selected === part}
            dim={dimmed(common, elem)}
            flash={common.flash?.get(part)}
            pending={common.pending?.has(part) ?? false}
            onSelect={common.onSelect}
            onOpen={common.onOpen}
          />
        );
      })}
    </>
  );
}
