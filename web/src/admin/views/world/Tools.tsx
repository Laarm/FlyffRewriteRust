// The Tools tab of the map's sidebar (Plan/25 section 3.2): the pointer (pan, box select, ruler) and the actions that need a click on the map - clear an
// area, respawn a region, spawn monsters at a point, drop an item at a point. Each action is offered only with its capability; the ruler shows the
// distance in metres and the walking time at 5 m/s.
import { useStore } from 'zustand';
import { Button } from '../../components';
import type { MapSlotProps } from '../../world/api';
import { useActionEnv } from './InspectorActions';
import { I } from './InspectorStrings';
import { clearArea, dropAtPoint, respawnAtPoint, spawnAtPoint } from './ToolsActions';
import './Inspector.css';

/** Walking speed of the ruler, metres per second (Plan/25 section 3.9). */
export const WALK_MPS = 5;

/** Distance and walking time of the ruler's two points, or `null` while the second is missing. */
export function measure(
  ruler: {
    readonly a: readonly [number, number];
    readonly b: readonly [number, number] | null;
  } | null,
): { metres: number; seconds: number } | null {
  if (!ruler?.b) return null;
  const metres = Math.hypot(ruler.b[0] - ruler.a[0], ruler.b[1] - ruler.a[1]);
  return { metres, seconds: metres / WALK_MPS };
}

export default function Tools({ api }: MapSlotProps) {
  const env = useActionEnv(api);
  const tool = useStore(api.store, (s) => s.tool);
  const ruler = useStore(api.store, (s) => s.ruler);
  const selected = useStore(api.store, (s) => s.selection.length);
  const picking = useStore(api.store, (s) => s.picking);
  const T = I.tools;
  const reading = measure(ruler);
  const busy = picking !== null;
  const can = (cap: string): boolean => env.caps.has(cap);
  const mode = (id: 'pan' | 'select' | 'ruler', label: string) => (
    <Button
      size="sm"
      variant={tool === id ? 'primary' : 'secondary'}
      aria-pressed={tool === id}
      onClick={() => {
        api.setTool(id);
      }}
    >
      {label}
    </Button>
  );
  return (
    <div className="tools-panel" data-testid="tools-panel">
      <h3>{T.pointer}</h3>
      <div className="tools-row">
        {mode('pan', T.pan)}
        {mode('select', T.boxSelect)}
        {mode('ruler', T.ruler)}
      </div>
      {tool === 'ruler' ? (
        <p className="insp-note" data-testid="ruler-reading">
          {reading ? T.rulerResult(reading.metres, reading.seconds) : T.rulerHint}
        </p>
      ) : null}
      {tool === 'select' ? <p className="insp-note">{T.useBox}</p> : null}
      {selected > 0 ? <p className="insp-note">{T.selected(selected)}</p> : null}
      <h3>{T.actions}</h3>
      <div className="tools-row">
        {can('act.combat') ? (
          <Button
            size="sm"
            disabled={busy}
            onClick={() => {
              void clearArea(env);
            }}
          >
            {T.clearArea}
          </Button>
        ) : null}
        {can('act.world') ? (
          <Button
            size="sm"
            disabled={busy}
            onClick={() => {
              void respawnAtPoint(env);
            }}
          >
            {T.respawn}
          </Button>
        ) : null}
        {can('act.combat') ? (
          <Button
            size="sm"
            disabled={busy}
            onClick={() => {
              void spawnAtPoint(env);
            }}
          >
            {T.spawn}
          </Button>
        ) : null}
        {can('act.items') ? (
          <Button
            size="sm"
            disabled={busy}
            onClick={() => {
              void dropAtPoint(env);
            }}
          >
            {T.drop}
          </Button>
        ) : null}
      </div>
      {!can('act.combat') && !can('act.world') && !can('act.items') ? (
        <p className="insp-note">{T.needsCombat}</p>
      ) : null}
    </div>
  );
}
