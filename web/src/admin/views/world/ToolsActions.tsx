// The tools that need a click on the map first (Plan/25 section 3.2 Tools, 3.12): clear an area (two corners, a typed confirmation), refill a spawn region
// (a click inside it), spawn monsters at a point and drop an item at a point. Each asks the map for the points with `pickPoint`, so Esc cancels it.
import { useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { Button, Checkbox, Dialog } from '../../components';
import type { AreaKind, SpawnRowDto } from '../../gen/dto';
import { openGrantDialog } from '../../grant';
import { openDialog } from '../../stores/dialogs';
import { toast } from '../../stores/toasts';
import { summarize, type ActionEnv } from './InspectorActions';
import { despawnArea, respawnRegion } from './InspectorApi';
import { spawnLines } from './InspectorData';
import { I } from './InspectorStrings';
import { openSpawnDialog } from './ToolsSpawn';

const AREA_KINDS: readonly AreaKind[] = ['monster', 'pet', 'npc', 'item', 'ctrl'];

function KindChoice({ onChange }: { readonly onChange: (kinds: readonly AreaKind[]) => void }) {
  const [on, setOn] = useState<ReadonlySet<AreaKind>>(new Set(['monster']));
  return (
    <div className="adm-stack">
      {AREA_KINDS.map((k) => (
        <Checkbox
          key={k}
          label={I.tools.kinds[k]}
          checked={on.has(k)}
          onChange={(e) => {
            const next = new Set(on);
            if (e.target.checked) next.add(k);
            else next.delete(k);
            setOn(next);
            onChange(AREA_KINDS.filter((x) => next.has(x)));
          }}
        />
      ))}
    </div>
  );
}

/** Two clicks make the rectangle, the operator ticks what goes, and types `CONFIRM`; at most 500 objects (the server counts and removes nothing above). */
export async function clearArea(env: ActionEnv): Promise<boolean> {
  const api = env.api;
  const a = await api.pickPoint(I.prompts.areaA);
  if (!a) return false;
  api.setHighlights('tools-area', [
    { id: 'area-a', kind: 'point', rect: [a.x, a.z, a.x, a.z], label: 'A' },
  ]);
  const b = await api.pickPoint(I.prompts.areaB);
  if (b?.world !== a.world || b.layer !== a.layer) {
    api.setHighlights('tools-area', []);
    return false;
  }
  const rect: [number, number, number, number] = [
    Math.min(a.x, b.x),
    Math.min(a.z, b.z),
    Math.max(a.x, b.x),
    Math.max(a.z, b.z),
  ];
  api.setHighlights('tools-area', [{ id: 'area', kind: 'rect', rect, label: I.tools.clearArea }]);
  const choice: { kinds: readonly AreaKind[] } = { kinds: ['monster'] };
  try {
    const out = await runConfirmed({
      title: I.dialogs.areaTitle,
      message: (
        <div className="adm-stack">
          <p>{I.dialogs.areaMessage(AREA_KINDS.length, rect[0], rect[1], rect[2], rect[3])}</p>
          <KindChoice
            onChange={(k) => {
              choice.kinds = k;
            }}
          />
        </div>
      ),
      confirmLabel: I.dialogs.areaConfirm,
      danger: true,
      facts: { irreversible: true },
      run: (given) => {
        if (choice.kinds.length === 0) throw new Error('Tick at least one kind.');
        return despawnArea({ world: a.world, layer: a.layer }, rect, choice.kinds, given);
      },
      success: (r) => summarize(r, I.dialogs.areaDone),
    });
    return out.ok;
  } finally {
    api.setHighlights('tools-area', []);
  }
}

function RegionChoice({
  lines,
  onPick,
  onClose,
}: {
  readonly lines: readonly SpawnRowDto[];
  readonly onPick: (line: SpawnRowDto) => void;
  readonly onClose: () => void;
}) {
  return (
    <Dialog title={I.dialogs.respawnTitle} onClose={onClose} size="sm" dismissOnOverlay>
      <p>{I.tools.regionPick}</p>
      <ul className="insp-places">
        {lines.map((l) => (
          <li key={l.region}>
            <Button
              size="sm"
              onClick={() => {
                onPick(l);
              }}
            >
              {I.tools.regionRow(l.name, l.alive, l.target)}
            </Button>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}

/** The respawn lines whose rectangle holds the point, the smallest first. */
export function linesAt(lines: readonly SpawnRowDto[], x: number, z: number): SpawnRowDto[] {
  return lines
    .filter((l) => x >= l.rect[0] && x < l.rect[2] && z >= l.rect[1] && z < l.rect[3])
    .sort(
      (a, b) =>
        (a.rect[2] - a.rect[0]) * (a.rect[3] - a.rect[1]) -
          (b.rect[2] - b.rect[0]) * (b.rect[3] - b.rect[1]) || a.region - b.region,
    );
}

async function refill(line: SpawnRowDto, at: { world: number; layer: number }): Promise<boolean> {
  const out = await runConfirmed({
    title: I.dialogs.respawnTitle,
    message: I.tools.regionRow(line.name, line.alive, line.target),
    confirmLabel: 'Respawn',
    run: (given) => respawnRegion(at, line.region, given),
    success: (r) => summarize(r, () => I.dialogs.respawnDone(r.affected ?? 0)),
  });
  return out.ok;
}

/** "Respawn region...": a click inside the region; when several lines cover the point the operator chooses one. */
export async function respawnAtPoint(env: ActionEnv): Promise<boolean> {
  const p = await env.api.pickPoint(I.prompts.respawn);
  if (!p) return false;
  let lines: readonly SpawnRowDto[];
  try {
    lines = await spawnLines(p.world);
  } catch (error) {
    toast.error(error, I.tools.regionLoading);
    return false;
  }
  const here = linesAt(lines, p.x, p.z);
  const first = here[0];
  if (!first) {
    toast.info(I.tools.noRegion);
    return false;
  }
  if (here.length === 1) return await refill(first, p);
  return await new Promise<boolean>((resolve) => {
    openDialog((close) => (
      <RegionChoice
        lines={here}
        onClose={() => {
          close();
          resolve(false);
        }}
        onPick={(line) => {
          close();
          void refill(line, p).then(resolve);
        }}
      />
    ));
  });
}

/** "Spawn monsters at a point...": a click, then the picker. */
export async function spawnAtPoint(env: ActionEnv): Promise<void> {
  const p = await env.api.pickPoint(I.prompts.spawn);
  if (p) openSpawnDialog(env, p);
}

/** "Drop item at a point...": a click, then the grant dialog in ground-drop mode. */
export async function dropAtPoint(env: ActionEnv): Promise<void> {
  const p = await env.api.pickPoint(I.prompts.drop);
  if (p) openGrantDialog({ ground: { world: p.world, layer: p.layer, x: p.x, y: 0, z: p.z } });
}
