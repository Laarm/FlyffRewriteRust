// Items -> Pets (Plan/25 section 4.4): the pet items of the bag, the pockets and the banks with kind, level, EXP, energy and life; the
// summoned one is marked active. "Return to egg" and "Revive" are the pet actions of the console (act.items, online characters).
import { useMemo } from 'react';
import { Badge, Button, Table, type Column } from '../../components';
import { useCan } from '../../stores/session';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { useCharacter } from './context';
import { FLAG_USING, type ContainerModel } from './containers';
import { formatNumber } from './format';
import { infoOf, ItemCell } from './ItemCell';
import { usePlayerGame, type PlayerGame } from './gamedata';
import { releasePet, revivePet } from './itemActions';
import { T } from './strings';

export interface PetRow {
  readonly key: string;
  readonly elem: ItemElem;
  readonly slot: number;
  readonly container: ContainerModel;
}

/** Every pet item the character (or its account) holds, the bag first. */
export function petRows(containers: ReadonlyMap<string, ContainerModel>): PetRow[] {
  const order = { inventory: 0, pocket: 1, bank: 2, equipment: 3 } as const;
  const rows: PetRow[] = [];
  for (const c of containers.values()) {
    if (c.kind === 'equipment') continue;
    for (const [slot, elem] of c.cells) {
      if (elem.pet) rows.push({ key: `${c.key}#${String(slot)}`, elem, slot, container: c });
    }
  }
  return rows.sort(
    (a, b) =>
      order[a.container.kind] - order[b.container.kind] ||
      a.container.index - b.container.index ||
      a.slot - b.slot,
  );
}

const whereOf = (r: PetRow): string => {
  const c = r.container;
  if (c.kind === 'inventory') return 'Bag';
  if (c.kind === 'pocket') return `Pocket ${String(c.index + 1)}`;
  return `Bank of ${c.ownerName || `slot ${String(c.index + 1)}`}`;
};

/** The name of the pet (its own name, else the name of its item). */
function petName(r: PetRow, game: PlayerGame['data']): string {
  const own = r.elem.pet?.name ?? '';
  return own !== '' ? own : infoOf(game, r.elem.itemId).name;
}

export function PetsPanel() {
  const { id, name, live } = useCharacter();
  const game = usePlayerGame();
  const canAct = useCan('act.items');
  const rows = useMemo(() => petRows(live.containers), [live.containers]);
  const columns: Column<PetRow>[] = [
    {
      id: 'item',
      header: 'Pet',
      cell: (r) => (
        <span className="adm-row">
          <ItemCell elem={r.elem} slot={r.slot} game={game.data} />
          <span>{petName(r, game.data)}</span>
        </span>
      ),
      text: (r) => petName(r, game.data),
    },
    { id: 'where', header: T.items.pets.where, cell: whereOf, text: whereOf },
    {
      id: 'kind',
      header: T.items.pets.kind,
      cell: (r) => r.elem.pet?.kind ?? 0,
      text: (r) => r.elem.pet?.kind ?? 0,
    },
    {
      id: 'level',
      header: T.items.pets.level,
      cell: (r) => T.items.pets.levels[r.elem.pet?.level ?? 0] ?? String(r.elem.pet?.level ?? 0),
      text: (r) => r.elem.pet?.level ?? 0,
    },
    {
      id: 'exp',
      header: T.items.pets.exp,
      cell: (r) => formatNumber(r.elem.pet?.exp ?? 0),
      text: (r) => r.elem.pet?.exp ?? 0,
      align: 'right',
    },
    {
      id: 'energy',
      header: T.items.pets.energy,
      cell: (r) => formatNumber(r.elem.pet?.energy ?? 0),
      text: (r) => r.elem.pet?.energy ?? 0,
      align: 'right',
    },
    {
      id: 'life',
      header: T.items.pets.life,
      cell: (r) =>
        (r.elem.pet?.life ?? 0) === 0 ? (
          <Badge tone="danger">{T.items.pets.dead}</Badge>
        ) : (
          formatNumber(r.elem.pet?.life ?? 0)
        ),
      text: (r) => r.elem.pet?.life ?? 0,
      align: 'right',
    },
    {
      id: 'state',
      header: '',
      cell: (r) =>
        (r.elem.flag & FLAG_USING) !== 0 ? (
          <Badge tone="accent">{T.items.pets.active}</Badge>
        ) : null,
      text: (r) => ((r.elem.flag & FLAG_USING) !== 0 ? T.items.pets.active : ''),
    },
    {
      id: 'actions',
      header: '',
      cell: (r) => {
        if (!canAct || !live.online) return null;
        const active = (r.elem.flag & FLAG_USING) !== 0;
        const dead = (r.elem.pet?.life ?? 1) === 0;
        return (
          <span className="adm-row">
            {active ? (
              <Button
                size="sm"
                onClick={() => {
                  void releasePet(id, name);
                }}
              >
                {T.items.pets.release}
              </Button>
            ) : null}
            {dead && r.container.kind === 'inventory' ? (
              <Button
                size="sm"
                onClick={() => {
                  void revivePet(id, name, r.slot, r.elem);
                }}
              >
                {T.items.pets.revive}
              </Button>
            ) : null}
          </span>
        );
      },
    },
  ];
  return (
    <Table
      label="Pets"
      columns={columns}
      rows={rows}
      rowKey={(r) => r.key}
      loading={live.status === 'loading'}
      emptyTitle={T.items.pets.none}
    />
  );
}
