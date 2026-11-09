// The overlay of the map (the `overlay` slot): the hover card, the context menu (a right click or a long press, Plan/25 section 3.11) and the "You"
// chip that jumps to the acting GM character. On an empty point the menu offers the point actions; on an entity the actions of its inspector card.
import { useCallback, useEffect, useState } from 'react';
import { useStore } from 'zustand';
import type { MapContextEvent, MapSlotProps } from '../../world/api';
import { Kind } from '../../world/types';
import { ContextMenu } from './ContextMenu';
import { HoverCard } from './HoverCard';
import {
  monsterActions,
  otherActions,
  playerActions,
  pointActions,
  selectedPlayers,
  useActionEnv,
  type ActionEnv,
  type MapAction,
  type ObjRef,
  type PlayerRef,
} from './InspectorActions';
import { useActingCharacter } from './InspectorData';
import { openTeleportToLocation } from './InspectorLocation';
import { jumpToMe } from './InspectorSummary';
import { I } from './InspectorStrings';
import { openBookmarkDialog } from './locations/BookmarkDialog';
import { openSpawnDialog } from './ToolsSpawn';
import './Inspector.css';

interface MenuState {
  readonly worldId: number;
  readonly x: number;
  readonly y: number;
  readonly title: string;
  readonly actions: readonly MapAction[];
}

/** The menu for a right click: the actions of the entity under the pointer, or the point actions of the ground. */
export function menuFor(env: ActionEnv, e: MapContextEvent): Omit<MenuState, 'worldId'> {
  const base = { x: e.clientX, y: e.clientY };
  const ent = e.entity;
  if (ent) {
    const ref: ObjRef = {
      id: ent.id,
      kind: ent.kind,
      name: ent.name,
      world: e.world,
      layer: e.layer,
      x: ent.x,
      z: ent.z,
    };
    if (ent.kind === Kind.PLAYER) {
      const p: PlayerRef = { ...ref, characterId: ent.characterId };
      const following = env.api.store.getState().follow?.objId === ent.id;
      return {
        ...base,
        title: ent.name,
        actions: [
          {
            id: 'select',
            label: I.actions.select,
            run: () => {
              env.api.select([ent.id], 'set');
            },
          },
          ...playerActions(env, p, {
            following,
            teleportToLocation: (players) => {
              openTeleportToLocation(env, players);
            },
          }),
        ],
      };
    }
    const own = ent.kind === Kind.MONSTER ? monsterActions(env, ref) : otherActions(env, ref);
    return {
      ...base,
      title: ent.name,
      actions: [
        {
          id: 'select',
          label: I.actions.select,
          run: () => {
            env.api.select([ent.id], 'set');
          },
        },
        ...own,
      ],
    };
  }
  const at = { world: e.world, layer: e.layer, x: e.x, z: e.z };
  return {
    ...base,
    title: I.menu.point(e.x, e.z),
    actions: pointActions(env, at, selectedPlayers(env.api), {
      spawn: (p) => {
        openSpawnDialog(env, p);
      },
      bookmark: (p) => {
        openBookmarkDialog({ world: p.world, x: p.x, z: p.z });
      },
    }),
  };
}

export default function MapOverlay({ api }: MapSlotProps) {
  const env = useActionEnv(api);
  const acting = useActingCharacter();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const picking = useStore(api.store, (s) => s.picking);
  const close = useCallback(() => {
    setMenu(null);
  }, []);

  useEffect(
    () =>
      api.onContextMenu((e) => {
        // a pending "pick a point" owns the clicks: a right click does not open a menu on top of it
        if (api.store.getState().picking !== null) return true;
        setMenu({ ...menuFor(env, e), worldId: api.store.getState().worldId });
        return true;
      }),
    [api, env],
  );

  // the menu belongs to the place it was opened for: a new world or a pick closes it
  const worldId = useStore(api.store, (s) => s.worldId);
  const shown = menu !== null && menu.worldId === worldId && picking === null ? menu : null;

  return (
    <div className="map-over" data-testid="map-overlay">
      <HoverCard api={api} />
      {acting ? (
        <button
          type="button"
          className="map-you"
          data-testid="you-chip"
          title={I.you.title(acting.name)}
          aria-label={I.you.jump}
          onClick={() => {
            jumpToMe(api);
          }}
        >
          <span aria-hidden="true">⌖</span> {I.you.label}
        </button>
      ) : null}
      {shown ? (
        <ContextMenu
          x={shown.x}
          y={shown.y}
          title={shown.title}
          actions={shown.actions}
          onClose={close}
        />
      ) : null}
    </div>
  );
}
