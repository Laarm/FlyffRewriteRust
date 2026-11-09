// "Teleport to location...": a chooser over the quick locations (Plan/25 section 3.10, 3.13). A click on a location closes the chooser and opens the usual
// confirmation of a teleport; a location of a layered world (`pick_live`) lists its live layers and sends the players into the one the operator clicks
// (none live: layer 0, the empty template layer).
import { useMemo, useState } from 'react';
import { Button, Dialog, Skeleton, TextInput } from '../../components';
import { openDialog } from '../../stores/dialogs';
import { teleportPlayers, type ActionEnv, type PlayerRef } from './InspectorActions';
import { I } from './InspectorStrings';
import { layersOfWorld, useLayers } from './layers';
import type { LocationRow } from './locations/api';
import { useLocations, useLocationsFeed } from './locations/store';

function TeleportLocationDialog({
  env,
  players,
  onClose,
}: {
  readonly env: ActionEnv;
  readonly players: readonly PlayerRef[];
  readonly onClose: () => void;
}) {
  useLocationsFeed();
  const doc = useLocations((s) => s.doc);
  const status = useLocations((s) => s.status);
  const layers = useLayers((s) => s.layers);
  const [text, setText] = useState('');
  const who = players.length === 1 ? (players[0]?.name ?? '') : `${String(players.length)} players`;
  const rows = useMemo(() => {
    const needle = text.trim().toLowerCase();
    return (doc?.locations ?? []).filter(
      (l) =>
        needle === '' ||
        l.name.toLowerCase().includes(needle) ||
        l.group.toLowerCase().includes(needle),
    );
  }, [doc, text]);

  const go = (row: LocationRow, layer: number): void => {
    onClose();
    void teleportPlayers(
      env,
      players,
      { world: row.world, layer, x: row.x, z: row.z },
      { label: 'location' },
    );
  };

  return (
    <Dialog title={I.dialogs.locationTitle(who)} onClose={onClose} size="md">
      <div className="adm-stack">
        <TextInput
          aria-label={I.dialogs.locationPick}
          placeholder={I.dialogs.locationPick}
          value={text}
          data-autofocus
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
        {status === 'loading' && doc === null ? (
          <Skeleton lines={4} />
        ) : rows.length === 0 ? (
          <p className="adm-muted">
            {doc === null ? I.dialogs.locationLoading : I.dialogs.locationNone}
          </p>
        ) : (
          <ul className="insp-places" aria-label={I.dialogs.locationPick}>
            {rows.map((l) => {
              const live = l.layer_policy === 'pick_live' ? layersOfWorld(layers, l.world) : [];
              return (
                <li key={l.id}>
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => {
                      go(l, live.length === 1 ? (live[0]?.layer ?? 0) : 0);
                    }}
                    disabled={live.length > 1}
                  >
                    {`${l.group}: ${l.name}`}
                  </Button>
                  {live.length > 1
                    ? live.map((lay) => (
                        <Button
                          key={lay.layer}
                          size="sm"
                          variant="ghost"
                          onClick={() => {
                            go(l, lay.layer);
                          }}
                        >
                          {`${I.dialogs.locationLayer} ${String(lay.layer)}${lay.owner === '' ? '' : ` · ${lay.owner}`} · ${String(lay.members)} in`}
                        </Button>
                      ))
                    : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </Dialog>
  );
}

/** Opens the chooser for these players. */
export function openTeleportToLocation(env: ActionEnv, players: readonly PlayerRef[]): void {
  openDialog((close) => <TeleportLocationDialog env={env} players={players} onClose={close} />);
}
