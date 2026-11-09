// The multi-selection card (Plan/25 section 3.10): the counts per kind, the list (a click narrows the selection to that object) and the group actions
// - kill all, despawn all, heal all players, message all players, teleport all players to a point (typed confirmation). The server takes at most
// `admin.max_objects_per_action` (500) targets and says so; the card shows a note above that.
import { useMemo } from 'react';
import { useStore } from 'zustand';
import { Button } from '../../components';
import type { EntityInfo, MapSlotProps } from '../../world/api';
import { Kind } from '../../world/types';
import { groupActions, useActionEnv, type ObjRef, type PlayerRef } from './InspectorActions';
import { useEntitiesOf } from './InspectorData';
import { ActionButtons, CardHead } from './InspectorParts';
import { I } from './InspectorStrings';

/** The most targets one action takes (`admin.max_objects_per_action`). */
export const MAX_TARGETS = 500;

const KIND_NAMES: Readonly<Record<number, string>> = {
  [Kind.PLAYER]: 'players',
  [Kind.MONSTER]: 'monsters',
  [Kind.NPC]: 'NPCs',
  [Kind.PET]: 'pets',
  [Kind.ITEM]: 'items',
  [Kind.CTRL]: 'ctrls',
};

/** "3 players, 12 monsters". */
export function countsText(list: readonly EntityInfo[]): string {
  const by = new Map<number, number>();
  for (const e of list) by.set(e.kind, (by.get(e.kind) ?? 0) + 1);
  return [...by.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([k, n]) => `${String(n)} ${KIND_NAMES[k] ?? 'objects'}`)
    .join(', ');
}

export function MultiCard({ api, ids }: MapSlotProps & { readonly ids: readonly number[] }) {
  const env = useActionEnv(api);
  const world = useStore(api.store, (s) => s.worldId);
  const layer = useStore(api.store, (s) => s.layer);
  const entities = useEntitiesOf(api, ids);
  const refs = useMemo<ObjRef[]>(
    () =>
      entities.map((e) =>
        e.kind === Kind.PLAYER
          ? ({
              id: e.id,
              kind: e.kind,
              name: e.name,
              world,
              layer,
              x: e.x,
              z: e.z,
              characterId: e.characterId,
            } satisfies PlayerRef)
          : { id: e.id, kind: e.kind, name: e.name, world, layer, x: e.x, z: e.z },
      ),
    [entities, world, layer],
  );
  const actions = groupActions(env, refs);
  return (
    <section
      className="insp-card"
      data-testid="inspector-card"
      data-kind="multi"
      aria-label={I.title}
    >
      <CardHead
        title={`${String(ids.length)} selected`}
        subtitle={<span data-testid="selection-counts">{countsText(entities)}</span>}
      />
      {ids.length > MAX_TARGETS ? (
        <p className="insp-note insp-note--warn">{`At most ${String(MAX_TARGETS)} objects per action: the server refuses more.`}</p>
      ) : null}
      {entities.length < ids.length ? (
        <p className="insp-note">{`${String(ids.length - entities.length)} of them are out of view.`}</p>
      ) : null}
      <ActionButtons actions={actions} />
      <table className="insp-list" data-testid="selection-list">
        <tbody>
          {entities.slice(0, 100).map((e) => (
            <tr key={e.id}>
              <td>
                <button
                  type="button"
                  className="insp-link"
                  onClick={() => {
                    api.select([e.id], 'set');
                  }}
                >
                  {e.name}
                </button>
              </td>
              <td className="insp-list__kind">{KIND_NAMES[e.kind] ?? ''}</td>
              <td className="insp-list__num">
                {e.kind === Kind.PLAYER || e.kind === Kind.MONSTER ? e.level : ''}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <Button
        size="sm"
        variant="ghost"
        onClick={() => {
          api.clearSelection();
        }}
      >
        {I.actions.clear}
      </Button>
    </section>
  );
}
