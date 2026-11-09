// The layer popover of a `pick_live` location (Plan/25 section 3.13, 3.14): the live layers of the world with owner, members, state and age, most
// members first; a click opens one on the map. With no live layer the banner says so and layer 0 (the empty template layer) is offered.
import { Button } from '../../components/Button';
import { Dialog } from '../../components/Dialog';
import { W } from './strings';
import { layersOfWorld, useLayers } from './layers';

export interface LayerPopoverProps {
  readonly world: number;
  readonly onChoose: (layer: number) => void;
  readonly onClose: () => void;
}

/** "3 min", "1 h 5 min": how long a layer has lived (the stream gives the creation tick; unknown ages read "-"). */
export function formatAge(seconds: number | null): string {
  if (seconds === null || seconds < 0) return '-';
  if (seconds < 60) return `${String(Math.round(seconds))} s`;
  const m = Math.floor(seconds / 60);
  return m < 60 ? `${String(m)} min` : `${String(Math.floor(m / 60))} h ${String(m % 60)} min`;
}

function ownerLabel(l: { owner: string; kind: string }): string {
  if (l.owner !== '') return l.owner;
  return W.instances.kinds[l.kind] ?? l.kind;
}

export function LayerPopover({ world, onChoose, onClose }: LayerPopoverProps) {
  const layers = layersOfWorld(
    useLayers((s) => s.layers),
    world,
  );
  return (
    <Dialog title={W.popover.title} onClose={onClose} size="sm" dismissOnOverlay>
      {layers.length === 0 ? (
        <p>{W.popover.none}</p>
      ) : (
        <ul className="map-popover">
          {layers.map((l) => (
            <li key={l.layer}>
              <Button
                variant="secondary"
                onClick={() => {
                  onChoose(l.layer);
                }}
              >
                {`${String(l.layer)} · ${ownerLabel(l)} · ${W.popover.members(l.members)}`}
              </Button>
            </li>
          ))}
        </ul>
      )}
      {layers.length === 0 ? (
        <Button
          variant="primary"
          onClick={() => {
            onChoose(0);
          }}
        >
          {W.popover.open}
        </Button>
      ) : null}
    </Dialog>
  );
}
