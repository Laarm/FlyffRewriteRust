// The inspector with nothing selected (Plan/25 section 3.2): the world summary - players in the world, the bosses in view, the live instances -
// with "Broadcast to this world" (`act.broadcast`) and "Jump to me".
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import { runConfirmed } from '../../app/mutate';
import { Button, Field, TextArea } from '../../components';
import { useCan } from '../../stores/session';
import type { EntityInfo, MapApi, MapSlotProps } from '../../world/api';
import { Flag, Kind, isBossRank } from '../../world/types';
import { broadcast } from '../comm/api';
import { useActingCharacter } from './InspectorData';
import { I } from './InspectorStrings';
import { layersOfWorld, useLayers } from './layers';
import { toast } from '../../stores/toasts';
import { useWorldNames } from './worldNames';

/** The bosses of the entities in view: mid-boss, boss and giant ranks, or the boss flag. */
export function bossesOf(list: readonly EntityInfo[]): EntityInfo[] {
  return list
    .filter((e) => e.kind === Kind.MONSTER && (isBossRank(e.sub) || (e.flags & Flag.BOSS) !== 0))
    .sort((a, b) => b.level - a.level || a.name.localeCompare(b.name));
}

function BroadcastText({ onText }: { readonly onText: (text: string) => void }) {
  const [text, setText] = useState('');
  return (
    <Field
      label="Text"
      hint="At most 511 bytes. Everybody in the world sees it as a system notice."
    >
      <TextArea
        rows={3}
        maxLength={400}
        data-autofocus
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          onText(e.target.value);
        }}
      />
    </Field>
  );
}

/** Sends a notice to the players of one world. */
export async function broadcastToWorld(world: number, name: string): Promise<boolean> {
  const draft = { text: '' };
  const out = await runConfirmed({
    title: `${I.summary.broadcast}: ${name}`,
    confirmLabel: 'Send',
    message: (
      <BroadcastText
        onText={(t) => {
          draft.text = t;
        }}
      />
    ),
    facts: { allPlayers: false, targets: 1, targetName: name },
    run: (given) => {
      if (draft.text.trim() === '') throw new Error('Write the notice first.');
      return broadcast(
        { text: draft.text.trim(), style: 'system', target: { kind: 'world', world } },
        given.reason,
        given.confirm,
      );
    },
    success: (r) => `Sent to ${String(r.delivered)} players.`,
  });
  return out.ok;
}

/** "Jump to me": the acting GM character, if it is in the world. */
export function jumpToMe(api: MapApi): void {
  if (!api.jumpToMe()) toast.info(I.you.offline);
}

export function Summary({ api }: MapSlotProps) {
  const worldId = useStore(api.store, (s) => s.worldId);
  const worlds = useStore(api.store, (s) => s.worlds);
  const counts = useStore(api.store, (s) => s.counts);
  const names = useWorldNames();
  const layers = useLayers((s) => s.layers);
  const canBroadcast = useCan('act.broadcast');
  const acting = useActingCharacter();
  const [bosses, setBosses] = useState<EntityInfo[]>(() => bossesOf(api.inView(Kind.MONSTER)));
  useEffect(() => {
    const read = (): void => {
      setBosses(bossesOf(api.inView(Kind.MONSTER)));
    };
    read();
    const t = setInterval(read, 1000);
    return () => {
      clearInterval(t);
    };
  }, [api]);
  const online = worlds?.worlds.find((w) => w.id === worldId)?.online ?? 0;
  const name = names.get(worldId) ?? `World ${String(worldId)}`;
  const S = I.summary;
  return (
    <section className="insp-card" data-testid="inspector-summary" aria-label={S.title}>
      <header className="insp-head">
        <div className="insp-head__text">
          <h3 className="insp-head__title">{name}</h3>
          <div className="insp-head__sub">
            <span>{S.title}</span>
            <span>{S.nothing}</span>
          </div>
        </div>
      </header>
      <ul className="insp-summary">
        <li data-testid="summary-players">{S.players(online)}</li>
        <li>{S.playersInView(counts.players)}</li>
        <li data-testid="summary-instances">
          {S.instances(layersOfWorld(layers, worldId).length)}
        </li>
        <li data-testid="summary-bosses">{S.bosses(bosses.length)}</li>
      </ul>
      {bosses.length > 0 ? (
        <ul className="insp-bosses">
          {bosses.slice(0, 8).map((b) => (
            <li key={b.id}>
              <button
                type="button"
                className="insp-link"
                onClick={() => {
                  api.select([b.id], 'set');
                }}
              >
                {S.bossRow(b.name, b.level)}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <div className="insp-actions">
        <div className="insp-actions__group">
          {canBroadcast ? (
            <Button
              size="sm"
              onClick={() => {
                void broadcastToWorld(worldId, name);
              }}
            >
              {S.broadcast}
            </Button>
          ) : null}
          <Button
            size="sm"
            disabled={acting === null}
            {...(acting === null ? { title: I.actions.needsActing } : {})}
            onClick={() => {
              jumpToMe(api);
            }}
          >
            {S.jumpToMe}
          </Button>
        </div>
      </div>
      <p className="insp-note">{S.hint}</p>
    </section>
  );
}
