// What the panels of the Items tab share: the filter over the cells, the selection, and the strip that acts on the selected cell.
import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Button, Checkbox, Field, NumberInput, Select, TextInput } from '../../components';
import { useCan } from '../../stores/session';
import type { ItemElem } from '../../../gen/proto/flyff/v1/item_types_pb';
import { openItemDialog } from './ItemDialog';
import { useCharacter } from './context';
import {
  NO_FILTER,
  isFiltering,
  type CellFilter,
  type ContainerModel,
  type KindFilter,
} from './containers';
import { infoOf } from './ItemCell';
import type { GridCommon } from './Grids';
import { removeCell } from './itemActions';
import { usePlayerGame } from './gamedata';
import { T } from './strings';
import { equipmentOf, type TipViewer } from './tooltip';

export interface Selection {
  readonly key: string;
  readonly slot: number;
}

export interface ItemsUi {
  readonly filter: CellFilter;
  readonly setFilter: (f: CellFilter) => void;
  readonly selection: Selection | null;
  readonly select: (container: ContainerModel, slot: number) => void;
  readonly clear: () => void;
}

export function useItemsUi(): ItemsUi {
  const [filter, setFilter] = useState<CellFilter>(NO_FILTER);
  const [selection, setSelection] = useState<Selection | null>(null);
  const select = useCallback((container: ContainerModel, slot: number) => {
    setSelection({ key: container.key, slot });
  }, []);
  const clear = useCallback(() => {
    setSelection(null);
  }, []);
  return { filter, setFilter, selection, select, clear };
}

/** The viewer of the tooltips: this character. */
export function useViewer(): TipViewer {
  const { live } = useCharacter();
  const d = live.detail;
  return useMemo(
    () => ({ sex: d?.sex ?? 0, job: d?.job ?? 0, level: d?.level ?? 1 }),
    [d?.sex, d?.job, d?.level],
  );
}

/** The equipment container of this character. */
export function useEquipment(): ReturnType<typeof equipmentOf> {
  const { id, live } = useCharacter();
  const model = live.containers.get(`equipment:0:${String(id)}`);
  return useMemo(() => equipmentOf(model), [model]);
}

export function FilterBar({ ui }: { readonly ui: ItemsUi }) {
  const f = ui.filter;
  const set = (patch: Partial<CellFilter>): void => {
    ui.setFilter({ ...f, ...patch });
  };
  return (
    <div className="plr-filter" role="search" aria-label={T.items.filter}>
      <Field label={T.items.filter}>
        <TextInput
          type="search"
          placeholder={T.items.filterText}
          value={f.text}
          onChange={(e) => {
            set({ text: e.target.value });
          }}
        />
      </Field>
      <Field label={T.items.filterKind}>
        <Select
          value={f.kind}
          onChange={(e) => {
            set({ kind: e.target.value as KindFilter });
          }}
        >
          {(Object.keys(T.items.kinds) as KindFilter[]).map((k) => (
            <option key={k} value={k}>
              {T.items.kinds[k]}
            </option>
          ))}
        </Select>
      </Field>
      <Field label={T.items.filterUpgrade}>
        <NumberInput
          min={0}
          max={20}
          value={f.minUpgrade === 0 ? '' : f.minUpgrade}
          onChange={(e) => {
            const n = Math.trunc(Number(e.target.value));
            set({ minUpgrade: Number.isFinite(n) ? Math.max(0, n) : 0 });
          }}
        />
      </Field>
      <Checkbox
        label={T.items.filterRandom}
        checked={f.randomOptions}
        onChange={(e) => {
          set({ randomOptions: e.target.checked });
        }}
      />
      {isFiltering(f) ? (
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            ui.setFilter(NO_FILTER);
          }}
        >
          {T.items.clearFilter}
        </Button>
      ) : null}
    </div>
  );
}

/** The grid helpers for one container: selection, opening the details, the flash of changed cells. */
export function useGridCommon(
  ui: ItemsUi,
  container: ContainerModel | undefined,
  flash: ReadonlyMap<string, ReadonlyMap<number, number>>,
  pending?: ReadonlySet<number>,
): GridCommon {
  const game = usePlayerGame();
  const { id, name } = useCharacter();
  const viewer = useViewer();
  const equipment = useEquipment();
  const selected = container && ui.selection?.key === container.key ? ui.selection.slot : null;
  return {
    game: game.data,
    filter: ui.filter,
    flash: container ? flash.get(container.key) : undefined,
    selected,
    pending,
    onSelect: (slot) => {
      if (container) ui.select(container, slot);
    },
    onOpen: (slot) => {
      const elem = container?.cells.get(slot);
      if (!container || !elem) return;
      openItemDialog({ elem, slot, container, character: { id, name }, viewer, equipment });
    },
  };
}

/** The strip under the grids: the selected item and what can be done with it. */
export function SelectedStrip({
  ui,
  containers,
}: {
  readonly ui: ItemsUi;
  readonly containers: ReadonlyMap<string, ContainerModel>;
}) {
  const game = usePlayerGame();
  const { id, name } = useCharacter();
  const viewer = useViewer();
  const equipment = useEquipment();
  const canRemove = useCan('act.items');
  const container = ui.selection ? containers.get(ui.selection.key) : undefined;
  const elem: ItemElem | undefined =
    container && ui.selection ? container.cells.get(ui.selection.slot) : undefined;
  if (!container || !elem || !ui.selection) {
    return (
      <div className="plr-selected" aria-live="polite">
        <span className="adm-muted">{T.items.nothingSelected}</span>
      </div>
    );
  }
  const slot = ui.selection.slot;
  const info = infoOf(game.data, elem.itemId);
  const actions: ReactNode = (
    <>
      <Button
        size="sm"
        onClick={() => {
          openItemDialog({ elem, slot, container, character: { id, name }, viewer, equipment });
        }}
      >
        {T.items.details}
      </Button>
      {canRemove ? (
        <Button
          size="sm"
          variant="danger"
          onClick={() => {
            void removeCell({
              characterId: id,
              characterName: name,
              container,
              slot,
              elem,
              itemName: info.name,
            });
          }}
        >
          {T.items.remove}
        </Button>
      ) : null}
    </>
  );
  return (
    <div className="plr-selected" aria-live="polite" data-testid="selected-item">
      <span className="plr-selected__name">
        {info.name}
        {elem.abilityOption > 0 ? ` +${String(elem.abilityOption)}` : ''}
        {elem.num > 1 ? ` ${T.items.stack(elem.num)}` : ''}
      </span>
      <span className="adm-muted adm-mono">#{elem.serial.toString()}</span>
      {actions}
    </div>
  );
}
