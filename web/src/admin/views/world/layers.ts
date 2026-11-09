// The live layers of every world (Plan/25 section 3.14, 3.16 `LayersSubscribe`): a store fed by ONE subscription on the tab's socket, shared by the
// Instances & layers tab and the layer selector of the map toolbar. The subscription opens with the first user and closes with the last; the stream
// sends the layers other than 0 on change, at most once a second.
import { useEffect } from 'react';
import { create } from 'zustand';
import type { LayerInfo } from '../../../gen/proto/admin';
import type { Subscription } from '../../api/ws';
import { adminSocket, useLive } from '../../stores/live';

export interface LayerRow {
  readonly world: number;
  readonly layer: number;
  /** `normal`, `ticket`, `instance`, `guild_house` or `mini_room`. */
  readonly kind: string;
  readonly owner: string;
  readonly members: number;
  readonly monsters: number;
  /** Seconds until the layer closes, `0` for none. */
  readonly secondsLeft: number;
  readonly createdTick: number;
}

interface LayersState {
  readonly layers: readonly LayerRow[];
  /** `idle` until the first list arrives. */
  readonly status: 'idle' | 'live' | 'refused';
  /** The client time of the last list (ms). */
  readonly at: number;
  /** When each layer (`world/layer`) was first seen by this tab (ms): the stream carries no creation clock, ages count from here. */
  readonly firstSeen: Readonly<Record<string, number>>;
}

export const useLayers = create<LayersState>(() => ({
  layers: [],
  status: 'idle',
  at: 0,
  firstSeen: {},
}));

export const layerKey = (l: { world: number; layer: number }): string =>
  `${String(l.world)}/${String(l.layer)}`;

/** The rows of a `LayerList` message. */
export function rowsOf(list: readonly LayerInfo[]): LayerRow[] {
  return list.map((l) => ({
    world: l.world,
    layer: l.layer,
    kind: l.kind,
    owner: l.owner,
    members: l.members,
    monsters: l.monsters,
    secondsLeft: l.secondsLeft,
    createdTick: Number(l.createdTick),
  }));
}

/** The live layers of one world, most members first (the popover and the selector sort like this). */
export function layersOfWorld(layers: readonly LayerRow[], world: number): LayerRow[] {
  return layers
    .filter((l) => l.world === world)
    .sort((a, b) => b.members - a.members || a.layer - b.layer);
}

let users = 0;
let sub: Subscription | null = null;

function open(): void {
  if (sub) return;
  sub = adminSocket().subscribeLayers(
    null,
    (list) => {
      const now = Date.now();
      const layers = rowsOf(list.layers);
      const prev = useLayers.getState().firstSeen;
      const firstSeen: Record<string, number> = {};
      for (const l of layers) firstSeen[layerKey(l)] = prev[layerKey(l)] ?? now;
      useLayers.setState({ layers, status: 'live', at: now, firstSeen });
    },
    {
      onError: () => {
        useLayers.setState({ status: 'refused' });
      },
    },
  );
}

function close(): void {
  const s = sub;
  sub = null;
  if (s) s.close();
  useLayers.setState({ layers: [], status: 'idle', at: 0, firstSeen: {} });
}

/** Holds the layer subscription while the component is mounted (and re-opens it on a new socket). */
export function useLayerFeed(): void {
  const epoch = useLive((s) => s.epoch);
  useEffect(() => {
    users += 1;
    // a new socket (epoch) forgets the old subscription
    if (sub && epoch >= 0) {
      /* the subscription object belongs to the socket it was made on */
    }
    open();
    return () => {
      users -= 1;
      if (users <= 0) {
        users = 0;
        close();
      }
    };
  }, [epoch]);
}

/** Tests: drops the module state. */
export function resetLayersForTests(): void {
  users = 0;
  sub = null;
  useLayers.setState({ layers: [], status: 'idle', at: 0, firstSeen: {} });
}
