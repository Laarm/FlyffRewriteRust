// The item drawer (`#/items/...?i=2154`, Plan/25 section 2.1): the picture, the exact game tooltip of the plain item, and the ways on:
// grant it, build a variant, find who has it.
import { useEffect } from 'react';
import type { DrawerBodyProps } from '../../app/views';
import { Button, Skeleton } from '../../components';
import { navigate } from '../../stores/route';
import { useCan } from '../../stores/session';
import { ensureGrantData, useGrantData } from '../../grant/data';
import { ItemIcon } from '../../grant/ItemIcon';
import { Preview } from '../../grant/ItemPreview';
import { blankSpec } from '../../grant/spec';
import { openGrantDialog } from '../../grant';
import { T } from './strings';
import type { StoredItemDto } from '../../gen/dto';

/** The plain item as the tooltip composer wants it. */
export function plainItem(id: number): StoredItemDto {
  return {
    item_id: id,
    count: 1,
    serial: 0,
    upgrade_level: 0,
    element: 0,
    element_level: 0,
    element_stone_id: 0,
    guild_id: 0,
    charged: false,
    flags: 0,
    keep_until: 0,
    random_option: '0',
    text: '',
    endurance: -1,
    piercing_cards: [],
    ultimate_gems: [],
    vis_keep_until: [],
    pet: null,
  };
}

export function ItemDrawer({ record, controls }: DrawerBodyProps) {
  const id = Number(record.id);
  const data = useGrantData((s) => s.data);
  const canGrant = useCan('act.items');
  useEffect(() => {
    void ensureGrantData();
  }, []);
  const prop = data?.byId.get(id);
  useEffect(() => {
    controls.setHeader(prop?.name ?? `#${record.id}`, `#${record.id}`);
  }, [controls, prop, record.id]);
  if (!data) return <Skeleton />;
  if (!prop) return <p className="grant-hint">{T.drawer.unknown}</p>;
  const go = (tab: string, params: Record<string, string>): void => {
    controls.close();
    navigate({ category: 'items', tab, params });
  };
  return (
    <div className="adm-stack">
      <div className="grant-inline">
        <ItemIcon icon={prop.icon} size={48} label={prop.name} />
        <strong>{prop.name}</strong>
      </div>
      <Preview item={plainItem(id)} />
      <div className="grant-actions">
        {canGrant && (
          <Button
            variant="primary"
            onClick={() => {
              openGrantDialog({ spec: blankSpec(id) });
            }}
          >
            {T.drawer.grant}
          </Button>
        )}
        <Button
          onClick={() => {
            go('builder', { item: String(id) });
          }}
        >
          {T.drawer.build}
        </Button>
        <Button
          onClick={() => {
            go('search', { item: String(id) });
          }}
        >
          {T.drawer.find}
        </Button>
      </div>
    </div>
  );
}
