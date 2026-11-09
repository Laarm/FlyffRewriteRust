// One cell of an item grid (Plan/25 section 4.4): the game's look of an inventory cell (`ui/icons/IconCell.tsx`: the 32 x 32 icon, the
// stack count, the red multiply of an expired item, the dim of an item in use) plus the admin's badges (the +N upgrade, the element, a
// dot for random options). IconCell itself needs a window manager (`WndEnv`); the cell here uses its exported pieces over the plain
// PNG URL of the icon, so the console does not boot a window system.
import { memo, type MouseEvent } from 'react';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { DIM_ALPHA, iconImageStyle } from '../../../ui/icons/IconCell';
import { FLAG_EXPIRED, FLAG_USING, hasRandomOptions } from './containers';
import type { PlayerGame } from './gamedata';
import { useTip } from './Tip';
import { T } from './strings';

export interface CellInfoView {
  readonly name: string;
  readonly iconUrl: string | null;
  readonly packMax: number;
}

/** What the game data knows about an item id (names and icons need the bundles; a cell works without them). */
export function infoOf(game: PlayerGame['data'], itemId: number): CellInfoView {
  const prop = game?.byId.get(itemId);
  return {
    name: prop?.name ?? T.items.unknownItem(itemId),
    iconUrl: prop && game ? game.iconUrl(prop.icon) : null,
    packMax: prop?.packMax ?? 1,
  };
}

export interface ItemCellProps {
  readonly elem: ItemElem | undefined;
  /** The display slot (the cell's address for the removal routes). */
  readonly slot: number;
  readonly game: PlayerGame['data'];
  readonly selected?: boolean;
  /** The filter hides this cell (it is dimmed, not removed, so the grid keeps its shape). */
  readonly dim?: boolean;
  /** The change counter: its parity restarts the one-second flash. */
  readonly flash?: number | undefined;
  /** A pending removal waits for this cell (dashed border). */
  readonly pending?: boolean;
  /** The cell is a worn piece (the tooltip adds the set bonus). */
  readonly worn?: boolean;
  /** What an empty cell is called (the equipment part, "Slot 12"). */
  readonly label?: string;
  readonly onSelect?: (slot: number) => void;
  readonly onOpen?: (slot: number) => void;
}

function Icon({ elem, info }: { readonly elem: ItemElem; readonly info: CellInfoView }) {
  const expired = (elem.flag & FLAG_EXPIRED) !== 0;
  const using = (elem.flag & FLAG_USING) !== 0;
  if (info.iconUrl === null) {
    return (
      <span className="plr-cell__q" aria-hidden="true">
        ?
      </span>
    );
  }
  return (
    <span
      className="plr-cell__icon"
      aria-hidden="true"
      style={iconImageStyle({
        url: info.iconUrl,
        size: 32,
        left: 0,
        top: 0,
        alpha: using ? DIM_ALPHA : 1,
        tint: expired,
      })}
    />
  );
}

export const ItemCell = memo(function ItemCell({
  elem,
  slot,
  game,
  selected = false,
  dim = false,
  flash,
  pending = false,
  worn = false,
  label,
  onSelect,
  onOpen,
}: ItemCellProps) {
  const tip = useTip();
  const parity = flash === undefined ? undefined : flash % 2 === 0 ? 'a' : 'b';
  if (!elem) {
    // an item that left the cell (a removal, a sale) is a change too: the empty cell flashes
    return (
      <div
        className="plr-cell plr-cell--empty"
        role="img"
        aria-label={label ?? T.items.slot(slot)}
        title={label}
        data-slot={slot}
        data-flash={parity}
      />
    );
  }
  const info = infoOf(game, elem.itemId);
  const show = (e: { currentTarget: HTMLElement }): void => {
    tip?.show({ elem, worn, slot, anchor: e.currentTarget.getBoundingClientRect() });
  };
  const classes = ['plr-cell', 'plr-cell--item', selected ? 'is-selected' : '', dim ? 'is-dim' : '']
    .filter(Boolean)
    .join(' ');
  return (
    <button
      type="button"
      className={classes}
      data-slot={slot}
      data-item={elem.itemId}
      data-flash={parity}
      aria-label={`${info.name}${elem.num > 1 ? ` ${T.items.stack(elem.num)}` : ''}${elem.abilityOption > 0 ? ` +${String(elem.abilityOption)}` : ''}`}
      aria-pressed={selected}
      onMouseEnter={show}
      onFocus={show}
      onMouseLeave={() => tip?.hide()}
      onBlur={() => tip?.hide()}
      onClick={(e: MouseEvent) => {
        e.stopPropagation();
        onSelect?.(slot);
      }}
      onDoubleClick={() => {
        onOpen?.(slot);
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onOpen?.(slot);
      }}
    >
      <Icon elem={elem} info={info} />
      {elem.num > 1 || info.packMax > 1 ? (
        <span className="plr-cell__count">{elem.num}</span>
      ) : null}
      {elem.abilityOption > 0 ? (
        <span className="plr-cell__plus">+{elem.abilityOption}</span>
      ) : null}
      {elem.itemResist > 0 ? (
        <span className="plr-cell__elem" data-element={elem.itemResist} aria-hidden="true" />
      ) : null}
      {hasRandomOptions(elem) ? <span className="plr-cell__opt" aria-hidden="true" /> : null}
      {pending ? <span className="plr-cell__pend" aria-hidden="true" /> : null}
    </button>
  );
});
