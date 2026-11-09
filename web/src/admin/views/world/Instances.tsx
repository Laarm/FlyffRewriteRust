// Live World -> Instances & layers (Plan/25 section 3.14): every live layer other than 0 from the `LayersSubscribe` stream: world, layer id, kind
// (instance, guild house, mini-room, ticket), owner, members, monsters alive, age and time left, with "Open on map". The same stream feeds the layer
// selector of the map toolbar. The subscription is open while this tab is shown.
import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { EmptyState } from '../../components/EmptyState';
import { Table, type Column } from '../../components/Table';
import { formatAge } from './LayerPopover';
import { openOnMap } from './jump';
import { layerKey, useLayerFeed, useLayers, type LayerRow } from './layers';
import { W } from './strings';
import { useWorldNames } from './worldNames';

/** "12 min": the time a layer has left; `0` means none (a layer without a timer). */
export function formatLeft(seconds: number): string {
  return seconds <= 0 ? '-' : formatAge(seconds);
}

export function Instances() {
  useLayerFeed();
  const layers = useLayers((s) => s.layers);
  const status = useLayers((s) => s.status);
  const names = useWorldNames();
  // ages count from the moment this tab first saw a layer (the stream carries no creation clock)
  const firstSeen = useLayers((s) => s.firstSeen);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      clearInterval(t);
    };
  }, []);

  const I = W.instances;
  const columns: Column<LayerRow>[] = [
    {
      id: 'world',
      header: I.world,
      cell: (r) => names.get(r.world) ?? `World ${String(r.world)}`,
      sortable: true,
      sortValue: (r) => r.world,
    },
    {
      id: 'layer',
      header: I.layer,
      cell: (r) => r.layer,
      sortable: true,
      sortValue: (r) => r.layer,
    },
    {
      id: 'kind',
      header: I.kind,
      cell: (r) => I.kinds[r.kind] ?? r.kind,
      sortable: true,
      sortValue: (r) => r.kind,
    },
    {
      id: 'owner',
      header: I.owner,
      cell: (r) => r.owner || '-',
      sortable: true,
      sortValue: (r) => r.owner,
    },
    {
      id: 'members',
      header: I.members,
      cell: (r) => r.members,
      sortable: true,
      sortValue: (r) => r.members,
      align: 'right',
    },
    {
      id: 'monsters',
      header: I.monsters,
      cell: (r) => r.monsters,
      sortable: true,
      sortValue: (r) => r.monsters,
      align: 'right',
    },
    {
      id: 'age',
      header: I.age,
      cell: (r) => formatAge((now - (firstSeen[layerKey(r)] ?? now)) / 1000),
    },
    { id: 'left', header: I.left, cell: (r) => formatLeft(r.secondsLeft) },
    {
      id: 'open',
      header: '',
      cell: (r) => (
        <Button
          size="sm"
          onClick={() => {
            openOnMap({ world: r.world, layer: r.layer });
          }}
        >
          {I.open}
        </Button>
      ),
    },
  ];

  if (status === 'refused') return <EmptyState icon="lock" title={I.title} hint={I.refused} />;
  if (status === 'idle') return <EmptyState icon="clock" title={I.title} hint={I.waiting} />;
  if (layers.length === 0) return <EmptyState icon="map" title={I.title} hint={I.empty} />;
  return (
    <section aria-label={I.title}>
      <Table columns={columns} rows={layers} rowKey={layerKey} label={I.title} />
    </section>
  );
}
