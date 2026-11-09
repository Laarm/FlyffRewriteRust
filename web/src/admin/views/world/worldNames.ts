// The names of the worlds by id for the lists of this category (from `GET /admin/api/worlds`, cached by `components/mapMeta`).
import { useEffect, useState } from 'react';
import { loadWorlds } from '../../components/mapMeta';

/** `id -> display name`; empty until the world list arrived (a failure leaves it empty: the lists then show "World <id>"). */
export function useWorldNames(): ReadonlyMap<number, string> {
  const [names, setNames] = useState<ReadonlyMap<number, string>>(new Map());
  useEffect(() => {
    let live = true;
    loadWorlds().then(
      (doc) => {
        if (live) setNames(new Map(doc.worlds.map((w) => [w.id, w.name])));
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);
  return names;
}
