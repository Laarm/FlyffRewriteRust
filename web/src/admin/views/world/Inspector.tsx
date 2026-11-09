// The Inspector tab of the map's sidebar (Plan/25 section 3.2, 3.10): nothing selected shows the world summary; one object its card (player, monster,
// NPC, ground item, pet or ctrl); several the multi-selection with the group actions. The map fills the slot (`slots.tsx`); the card reads the
// selection from the map's store and the numbers from `GET /admin/api/objects/{obj}`.
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import type { MapSlotProps } from '../../world/api';
import { Kind } from '../../world/types';
import { fetchObject } from './InspectorApi';
import { useEntity } from './InspectorData';
import { MultiCard } from './InspectorMulti';
import { ItemCard, MonsterCard, NpcCard, OtherCard } from './InspectorObjects';
import { PlayerCard } from './InspectorPlayer';
import { Summary } from './InspectorSummary';
import './Inspector.css';

const KIND_OF_NAME: Readonly<Record<string, number>> = {
  player: Kind.PLAYER,
  monster: Kind.MONSTER,
  npc: Kind.NPC,
  pet: Kind.PET,
  item: Kind.ITEM,
  ctrl: Kind.CTRL,
};

/**
 * One selected object. Its kind comes from the frames; an object that left the view keeps the kind it had, and one that was never in view (a link
 * with `sel=`, a find hit in another part of the world) is asked from the server once.
 */
function OneCard({ api, id }: MapSlotProps & { readonly id: number }) {
  const entity = useEntity(api, id);
  const [asked, setAsked] = useState<{ id: number; kind: number } | null>(null);
  const inView = entity !== null;
  useEffect(() => {
    if (inView || asked?.id === id) return;
    let live = true;
    fetchObject(id).then(
      (d) => {
        if (live) setAsked({ id, kind: KIND_OF_NAME[d.kind] ?? Kind.MONSTER });
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [inView, id, asked]);
  const k = entity?.kind ?? (asked?.id === id ? asked.kind : null);
  if (k === null) return null;
  switch (k) {
    case Kind.PLAYER:
      return <PlayerCard api={api} id={id} />;
    case Kind.MONSTER:
      return <MonsterCard api={api} id={id} />;
    case Kind.NPC:
      return <NpcCard api={api} id={id} />;
    case Kind.ITEM:
      return <ItemCard api={api} id={id} />;
    default:
      return <OtherCard api={api} id={id} kind={k} />;
  }
}

export default function Inspector({ api }: MapSlotProps) {
  const selection = useStore(api.store, (s) => s.selection);
  const first = selection[0];
  if (first === undefined) return <Summary api={api} />;
  if (selection.length === 1) return <OneCard key={first} api={api} id={first} />;
  return <MultiCard api={api} ids={selection} />;
}
