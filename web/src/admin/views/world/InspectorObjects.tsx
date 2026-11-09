// The cards of everything on the map that is not a player (Plan/25 section 3.10): the monster card (rank, AI state, target and the top of the hate list, the
// spawn region and its respawn time, the owner of a summon), the NPC card (key, dialog script, menus, shop or teleporter), the ground item card (icon and
// game tooltip, owner, owner-only) and a plain card for pets and ctrls.
import { useEffect, useMemo, useState } from 'react';
import { useStore } from 'zustand';
import { runConfirmed } from '../../app/mutate';
import type { ObjectDetail, StoredItemDto } from '../../gen/dto';
import { ensureGrantData, ItemIcon, Preview, useGrantData } from '../../grant';
import type { MapApi, MapSlotProps } from '../../world/api';
import { Kind, rankName } from '../../world/types';
import { useBundle } from '../players/gamedata';
import {
  monsterActions,
  otherActions,
  summarize,
  useActionEnv,
  type MapAction,
  type ObjRef,
} from './InspectorActions';
import { respawnRegion } from './InspectorApi';
import { useEntity, useObjectDetail, useSpawnLine } from './InspectorData';
import { ActionButtons, Bar, CardHead, Facts } from './InspectorParts';
import { I } from './InspectorStrings';
import { stateText } from './InspectorPlayer';
import { openNpcScript } from './InspectorScript';

function refOf(
  id: number,
  kind: number,
  name: string,
  d: ObjectDetail | null,
  api: MapApi,
): ObjRef {
  const s = api.store.getState();
  return {
    id,
    kind,
    name,
    world: d?.world ?? s.worldId,
    layer: d?.layer ?? s.layer,
    x: d?.x ?? api.entity(id)?.x ?? 0,
    z: d?.z ?? api.entity(id)?.z ?? 0,
  };
}

/** A name that selects the object it names. */
function ObjLink({
  api,
  id,
  name,
}: {
  readonly api: MapApi;
  readonly id: number;
  readonly name: string;
}) {
  return (
    <button
      type="button"
      className="insp-link"
      onClick={() => {
        api.select([id], 'set');
      }}
    >
      {name}
    </button>
  );
}

// ---- monsters --------------------------------------------------------------------------------------------------------------------

export function MonsterCard({ api, id }: MapSlotProps & { readonly id: number }) {
  const env = useActionEnv(api);
  const entity = useEntity(api, id);
  const { detail, gone } = useObjectDetail(id);
  const world = detail?.world ?? api.store.getState().worldId;
  const line = useSpawnLine(world, detail?.spawn_region ?? 0);
  const [region, setRegion] = useState(false);
  const name = detail?.name ?? entity?.name ?? `#${String(id)}`;
  const ref = refOf(id, Kind.MONSTER, name, detail, api);

  // the spawn region is drawn on the map while it is asked for; it goes with the card
  useEffect(() => {
    if (!region || !line) {
      api.setHighlights('inspector-region', []);
      return;
    }
    api.setHighlights('inspector-region', [
      { id: `region-${String(line.region)}`, kind: 'rect', rect: line.rect, label: line.name },
    ]);
    return () => {
      api.setHighlights('inspector-region', []);
    };
  }, [api, region, line]);

  const actions: MapAction[] = monsterActions(env, ref);
  if (line) {
    const at = actions.findIndex((a) => a.id === 'follow');
    const extra: MapAction[] = [
      {
        id: 'show-region',
        label: region ? I.actions.hideRegion : I.actions.showRegion,
        separator: true,
        run: () => {
          setRegion((r) => !r);
        },
      },
    ];
    if (env.caps.has('act.world')) {
      extra.push({
        id: 'respawn-region',
        label: I.actions.respawnRegion,
        run: async () => {
          await runConfirmed({
            title: I.dialogs.respawnTitle,
            message: line.name,
            confirmLabel: 'Respawn',
            run: (given) =>
              respawnRegion({ world: ref.world, layer: ref.layer }, line.region, given),
            success: (r) => summarize(r, () => I.dialogs.respawnDone(r.affected ?? 0)),
          });
        },
      });
    }
    actions.splice(at < 0 ? actions.length : at, 0, ...extra);
  }
  const target = detail && detail.target > 0 ? api.entity(detail.target) : null;
  const owner = detail && detail.owner > 0 ? api.entity(detail.owner) : null;
  return (
    <section
      className="insp-card"
      data-testid="inspector-card"
      data-kind="monster"
      aria-label={name}
    >
      <CardHead
        title={name}
        badge={I.kinds[2]}
        subtitle={
          <>
            <span>{I.hover.lv(detail?.level ?? entity?.level ?? 0)}</span>
            <span>{rankName(entity?.sub ?? 0)}</span>
            <span>{`${I.facts.prop} ${String(detail?.prop ?? entity?.prop ?? 0)}`}</span>
          </>
        }
      />
      {gone ? <p className="insp-note insp-note--warn">{I.gone}</p> : null}
      {detail ? (
        <div className="insp-bars">
          <Bar label={I.facts.hp} value={detail.hp} max={detail.hp_max} tone="hp" />
        </div>
      ) : null}
      <Facts
        rows={[
          detail && {
            label: I.facts.ai,
            value:
              I.ai[detail.ai_state] ?? (detail.ai_state === '' ? I.facts.none : detail.ai_state),
            id: 'insp-ai',
          },
          detail && { label: I.facts.state, value: stateText(detail.flag_names) },
          detail && {
            label: I.facts.position,
            value: `${detail.x.toFixed(1)}, ${detail.y.toFixed(1)}, ${detail.z.toFixed(1)}`,
          },
          detail &&
            detail.target > 0 && {
              label: I.facts.target,
              value: (
                <ObjLink
                  api={api}
                  id={detail.target}
                  name={target?.name ?? `#${String(detail.target)}`}
                />
              ),
            },
          detail &&
            detail.owner > 0 && {
              label: I.facts.owner,
              value: (
                <ObjLink
                  api={api}
                  id={detail.owner}
                  name={owner?.name ?? `#${String(detail.owner)}`}
                />
              ),
            },
          line && {
            label: I.facts.region,
            value: `${line.name} · ${String(line.alive)}/${String(line.target)}`,
            id: 'insp-region',
          },
          line && {
            label: I.facts.respawn,
            value: `${String(line.interval_s)} s${line.countdown_s > 0 ? `, next in ${String(line.countdown_s)} s` : ''}`,
          },
        ]}
      />
      {detail && detail.hate.length > 0 ? (
        <>
          <h4 className="insp-sub">{I.facts.hate}</h4>
          <ol className="insp-hate" data-testid="insp-hate">
            {detail.hate.slice(0, 5).map((h) => (
              <li key={h.obj_id}>
                <ObjLink api={api} id={h.obj_id} name={h.name} />
                <span className="insp-hate__n">{h.hate.toLocaleString('en-US')}</span>
              </li>
            ))}
          </ol>
        </>
      ) : null}
      <ActionButtons actions={actions} />
    </section>
  );
}

// ---- NPCs ------------------------------------------------------------------------------------------------------------------------

/** The `character.inc` key of an NPC on the map: the placement of the same prop that lies within a few metres. */
export function npcKeyAt(
  placements: readonly { key: string; prop: number; x: number; z: number }[],
  prop: number,
  x: number,
  z: number,
): string | null {
  let best: { key: string; d: number } | null = null;
  for (const n of placements) {
    if (n.prop !== prop) continue;
    const d = Math.hypot(n.x - x, n.z - z);
    if (d <= 8 && (best === null || d < best.d)) best = { key: n.key, d };
  }
  return best?.key ?? null;
}

export function NpcCard({ api, id }: MapSlotProps & { readonly id: number }) {
  const env = useActionEnv(api);
  const entity = useEntity(api, id);
  const { detail, gone } = useObjectDetail(id);
  const meta = useStore(api.store, (s) => s.meta);
  const npcs = useBundle('npcs');
  const name = detail?.name ?? entity?.name ?? `#${String(id)}`;
  const prop = detail?.prop ?? entity?.prop ?? 0;
  const x = detail?.x ?? entity?.x ?? 0;
  const z = detail?.z ?? entity?.z ?? 0;
  const key = useMemo(() => npcKeyAt(meta?.npcs ?? [], prop, x, z), [meta, prop, x, z]);
  const row = useMemo(
    () =>
      key === null
        ? undefined
        : npcs.data?.npcs.find((n) => n.key.toLowerCase() === key.toLowerCase()),
    [npcs.data, key],
  );
  const ref = refOf(id, Kind.NPC, name, detail, api);
  const actions: MapAction[] = [
    ...otherActions(env, ref),
    {
      id: 'open-script',
      label: I.actions.openScript,
      ...(row?.dialog_key ? {} : { disabled: I.actions.noScript }),
      run: () => {
        if (row?.dialog_key) openNpcScript(row.dialog_key);
      },
    },
  ];
  const shop = row?.tabs.some((t) => t.stock.length > 0)
    ? row.vendor_type === 1
      ? 'chip shop'
      : 'shop'
    : null;
  return (
    <section className="insp-card" data-testid="inspector-card" data-kind="npc" aria-label={name}>
      <CardHead
        title={name}
        badge={I.kinds[3]}
        subtitle={<span>{`${I.facts.prop} ${String(prop)}`}</span>}
      />
      {gone ? <p className="insp-note insp-note--warn">{I.gone}</p> : null}
      <Facts
        rows={[
          { label: I.facts.key, value: key ?? I.facts.unknown, id: 'insp-key' },
          row
            ? {
                label: I.facts.script,
                value: row.dialog_key ? `scripts/npc/${row.dialog_key}.lua` : I.facts.none,
              }
            : null,
          row
            ? {
                label: 'Menus',
                value: row.menus.length === 0 ? I.facts.none : String(row.menus.length),
              }
            : null,
          row
            ? {
                label: I.facts.kind,
                value:
                  [shop, row.teleports.length > 0 ? 'teleporter' : null]
                    .filter(Boolean)
                    .join(', ') || I.facts.none,
              }
            : null,
          { label: I.facts.position, value: `${x.toFixed(1)}, ${z.toFixed(1)}` },
        ]}
      />
      <ActionButtons actions={actions} />
    </section>
  );
}

// ---- ground items ----------------------------------------------------------------------------------------------------------------

/** The item of a ground drop as the tooltip composer takes it: what the frame knows (the id and the stack), everything else plain. */
export function plainItem(itemId: number, count: number): StoredItemDto {
  return {
    item_id: itemId,
    count: Math.max(1, count),
    serial: 0,
    upgrade_level: 0,
    element: 0,
    element_level: 0,
    element_stone_id: 0,
    guild_id: 0,
    charged: false,
    flags: 0,
    keep_until: 0,
    random_option: '0',
    text: '',
    endurance: -1,
    piercing_cards: [],
    ultimate_gems: [],
    vis_keep_until: [],
    pet: null,
  };
}

export function ItemCard({ api, id }: MapSlotProps & { readonly id: number }) {
  const env = useActionEnv(api);
  const entity = useEntity(api, id);
  const { detail, gone } = useObjectDetail(id);
  const data = useGrantData((s) => s.data);
  useEffect(() => {
    void ensureGrantData();
  }, []);
  const itemId = detail?.prop ?? entity?.prop ?? 0;
  const count = entity?.level ?? 1;
  const catalog = data?.byId.get(itemId);
  const name =
    [catalog?.name, detail?.name, entity?.name].find((n) => n !== undefined && n !== '') ??
    `#${String(id)}`;
  const ref = refOf(id, Kind.ITEM, name, detail, api);
  const owner = detail && detail.owner > 0 ? api.entity(detail.owner) : null;
  const ownerOnly = detail ? detail.flag_names.includes('owner_only') : false;
  const item = useMemo(() => (itemId > 0 ? plainItem(itemId, count) : null), [itemId, count]);
  return (
    <section className="insp-card" data-testid="inspector-card" data-kind="item" aria-label={name}>
      <CardHead
        icon={catalog ? <ItemIcon icon={catalog.icon} size={36} label={name} /> : undefined}
        title={name}
        badge={I.kinds[5]}
        subtitle={<span>{`${I.facts.prop} ${String(itemId)}`}</span>}
      />
      {gone ? <p className="insp-note insp-note--warn">{I.gone}</p> : null}
      <Facts
        rows={[
          { label: I.facts.count, value: String(count) },
          detail &&
            detail.owner > 0 && {
              label: I.facts.owner,
              value: owner?.name ?? `#${String(detail.owner)}`,
            },
          detail && { label: I.facts.ownerOnly, value: ownerOnly ? I.facts.yes : I.facts.no },
          detail && {
            label: I.facts.position,
            value: `${detail.x.toFixed(1)}, ${detail.z.toFixed(1)}`,
          },
        ]}
      />
      <div className="insp-tip" data-testid="insp-item-tip">
        <Preview item={item} />
      </div>
      <ActionButtons actions={otherActions(env, ref)} />
    </section>
  );
}

// ---- pets and ctrls ---------------------------------------------------------------------------------------------------------------

export function OtherCard({
  api,
  id,
  kind,
}: MapSlotProps & { readonly id: number; readonly kind: number }) {
  const env = useActionEnv(api);
  const entity = useEntity(api, id);
  const { detail, gone } = useObjectDetail(id);
  const name = detail?.name ?? entity?.name ?? `#${String(id)}`;
  const ref = refOf(id, kind, name, detail, api);
  const owner = detail && detail.owner > 0 ? api.entity(detail.owner) : null;
  return (
    <section className="insp-card" data-testid="inspector-card" data-kind="other" aria-label={name}>
      <CardHead
        title={name}
        badge={I.kinds[kind]}
        subtitle={<span>{`${I.facts.prop} ${String(detail?.prop ?? entity?.prop ?? 0)}`}</span>}
      />
      {gone ? <p className="insp-note insp-note--warn">{I.gone}</p> : null}
      {detail && detail.hp_max > 0 ? (
        <div className="insp-bars">
          <Bar label={I.facts.hp} value={detail.hp} max={detail.hp_max} tone="hp" />
        </div>
      ) : null}
      <Facts
        rows={[
          detail && {
            label: I.facts.position,
            value: `${detail.x.toFixed(1)}, ${detail.z.toFixed(1)}`,
          },
          detail &&
            detail.owner > 0 && {
              label: I.facts.owner,
              value: owner?.name ?? `#${String(detail.owner)}`,
            },
        ]}
      />
      <ActionButtons actions={otherActions(env, ref)} />
    </section>
  );
}
