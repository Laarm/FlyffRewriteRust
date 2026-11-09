// The hover card (Plan/25 section 3.2, 3.5): what the pointer rests on, as a small card beside it - name, level, job or rank, an HP gauge, a state word.
// It reads the entity the map reports under the pointer (`hover` of the map's store, set at the pointer's pace, never per frame) and asks nothing of the
// server: everything on it is in the frames. The card never takes the pointer.
import { useLayoutEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { EntityInfo, MapApi } from '../../world/api';
import { Flag, Kind, rankName } from '../../world/types';
import { jobName } from '../players/jobs';
import { useTick } from './InspectorData';
import { I } from './InspectorStrings';

/** The line under the name: job of a player, rank of a monster, the stack of an item. */
export function hoverMeta(e: EntityInfo): string {
  switch (e.kind) {
    case Kind.PLAYER:
      return `${I.hover.lv(e.level)} ${jobName(e.sub)}`;
    case Kind.MONSTER:
      return e.sub === 0 ? I.hover.lv(e.level) : `${I.hover.lv(e.level)} ${rankName(e.sub)}`;
    case Kind.ITEM:
      return `x${String(e.level)}`;
    case Kind.NPC:
      return 'NPC';
    case Kind.PET:
      return 'Pet';
    default:
      return 'Ctrl';
  }
}

/** The state words that matter at a glance. */
export function hoverState(e: EntityInfo): string[] {
  const out: string[] = [];
  if ((e.flags & Flag.DEAD) !== 0) out.push(I.hover.dead);
  if ((e.flags & Flag.ASLEEP) !== 0) out.push(I.hover.asleep);
  if ((e.flags & Flag.BOSS) !== 0) out.push(I.hover.boss);
  return out;
}

/** The card for the entity under the pointer, or nothing. */
export function HoverCard({ api }: { readonly api: MapApi }) {
  const hover = useStore(api.store, (s) => s.hover);
  useTick(hover ? 500 : 0);
  const entity = hover ? api.entity(hover.id) : null;
  const root = useRef<HTMLDivElement>(null);
  const [flip, setFlip] = useState({ x: false, y: false });

  // near the right or lower edge the card goes to the other side of the pointer (primitives as dependencies: the entity is a new object per read)
  const sx = hover?.sx;
  const sy = hover?.sy;
  const shown = entity !== null;
  useLayoutEffect(() => {
    const el = root.current;
    const parent = el?.parentElement;
    if (!el || !parent || sx === undefined || sy === undefined) return;
    const room = parent.getBoundingClientRect();
    const next = {
      x: sx + 16 + el.offsetWidth > room.width,
      y: sy + 16 + el.offsetHeight > room.height,
    };
    setFlip((cur) => (cur.x === next.x && cur.y === next.y ? cur : next));
  }, [sx, sy, shown]);

  if (!hover || !entity) return null;
  const x = flip.x ? hover.sx - 12 : hover.sx + 16;
  const y = flip.y ? hover.sy - 12 : hover.sy + 16;
  const state = hoverState(entity);
  const bars = entity.kind === Kind.PLAYER || entity.kind === Kind.MONSTER;
  return (
    <div
      ref={root}
      className="hover-card"
      data-testid="hover-card"
      role="tooltip"
      style={{
        left: x,
        top: y,
        transform: `translate(${flip.x ? '-100%' : '0'}, ${flip.y ? '-100%' : '0'})`,
      }}
    >
      <span className="hover-card__name">{entity.name}</span>
      <span className="hover-card__meta">
        {hoverMeta(entity)}
        {state.length > 0 ? ` · ${state.join(', ')}` : ''}
      </span>
      {bars ? (
        <div className="insp-bar" data-tone="hp">
          <span
            className="insp-bar__track"
            role="meter"
            aria-label="HP"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={entity.hpPct}
          >
            <span className="insp-bar__fill" style={{ width: `${String(entity.hpPct)}%` }} />
          </span>
        </div>
      ) : null}
    </div>
  );
}
