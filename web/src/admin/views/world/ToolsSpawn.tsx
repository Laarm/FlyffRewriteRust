// The spawn picker (Plan/25 section 3.11, 3.12): choose a monster by name, level and rank, how many (1 to 100), how far apart (0 to 30 m), aggressive or
// not, and for how long they stay. The list is the `movers` bundle filtered to the AI kinds `/createnpc` accepts (the server checks the same rule);
// nothing is hard-coded here but those six AI numbers of the original (`AII_*`).
import { useMemo, useState } from 'react';
import {
  Button,
  Checkbox,
  Dialog,
  Field,
  NumberInput,
  Skeleton,
  TextInput,
} from '../../components';
import { decodeRows } from '../../../data/rowTable';
import { openDialog } from '../../stores/dialogs';
import { toast } from '../../stores/toasts';
import type { PickedPoint } from '../../world/api';
import { useBundle } from '../players/gamedata';
import { spawn } from './InspectorApi';
import { summarize, type ActionEnv } from './InspectorActions';
import { I } from './InspectorStrings';

/** `AII_MONSTER`, `AII_CLOCKWORKS`, `AII_BIGMUSCLE`, `AII_KRRR`, `AII_BEAR`, `AII_METEONYKER`: the fighting AIs that a spawn accepts. */
export const SPAWNABLE_AI: ReadonlySet<number> = new Set([2, 4, 6, 7, 8, 10]);

export interface MonsterChoice {
  readonly id: number;
  readonly name: string;
  readonly level: number;
  readonly rank: number;
}

/** The monsters of a `movers` bundle that can be spawned. */
export function spawnableMonsters(
  rows: readonly { id: number; name: string; level: number; class: number; ai: number }[],
): MonsterChoice[] {
  return rows
    .filter((r) => r.id > 0 && r.name.trim() !== '' && SPAWNABLE_AI.has(r.ai))
    .map((r) => ({ id: r.id, name: r.name.trim(), level: r.level, rank: r.class }));
}

/** The best matches of a query: a prop id, or the words in the name; the exact name first, then by level and name. */
export function searchMonsters(
  list: readonly MonsterChoice[],
  query: string,
  limit = 60,
): MonsterChoice[] {
  const q = query.trim().toLowerCase();
  if (q === '') return [];
  const byId = /^\d+$/.test(q) ? Number(q) : null;
  const out = list.filter(
    (m) => (byId !== null && m.id === byId) || m.name.toLowerCase().includes(q),
  );
  out.sort(
    (a, b) =>
      Number(b.name.toLowerCase() === q) - Number(a.name.toLowerCase() === q) ||
      Number(b.name.toLowerCase().startsWith(q)) - Number(a.name.toLowerCase().startsWith(q)) ||
      a.level - b.level ||
      a.name.localeCompare(b.name),
  );
  return out.slice(0, limit);
}

export interface SpawnForm {
  readonly count: string;
  readonly spread: string;
  readonly minutes: string;
}

/** The numbers of the form, or `null` when one is out of range. */
export function readSpawnForm(
  f: SpawnForm,
): { count: number; spread: number; lifetimeS: number | null } | null {
  const count = Number(f.count);
  const spread = f.spread.trim() === '' ? 0 : Number(f.spread);
  const minutes = f.minutes.trim() === '' ? null : Number(f.minutes);
  if (!Number.isInteger(count) || count < 1 || count > 100) return null;
  if (!Number.isFinite(spread) || spread < 0 || spread > 30) return null;
  if (minutes !== null && (!Number.isFinite(minutes) || minutes <= 0 || minutes > 100_000))
    return null;
  return { count, spread, lifetimeS: minutes === null ? null : Math.round(minutes * 60) };
}

function SpawnDialog({
  env,
  at,
  onClose,
}: {
  readonly env: ActionEnv;
  readonly at: PickedPoint;
  readonly onClose: () => void;
}) {
  const movers = useBundle('movers');
  const [query, setQuery] = useState('');
  const [choice, setChoice] = useState<MonsterChoice | null>(null);
  const [form, setForm] = useState<SpawnForm>({ count: '1', spread: '0', minutes: '' });
  const [aggressive, setAggressive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const all = useMemo(
    () => (movers.data ? spawnableMonsters(decodeRows(movers.data.movers)) : []),
    [movers.data],
  );
  const found = useMemo(() => searchMonsters(all, query), [all, query]);
  const S = I.spawn;
  const parsed = readSpawnForm(form);

  const submit = async (): Promise<void> => {
    if (!choice) {
      setError(S.pick);
      return;
    }
    if (!parsed) {
      setError(S.invalid);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const r = await spawn({
        world: at.world,
        layer: at.layer,
        x: at.x,
        z: at.z,
        prop: choice.id,
        count: parsed.count,
        spread: parsed.spread,
        aggressive,
        lifetimeS: parsed.lifetimeS,
      });
      toast.success(summarize(r, () => S.done(r.spawned.length || r.ok)));
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={S.title}
      onClose={onClose}
      busy={busy}
      size="md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={!choice || !parsed}
            onClick={() => void submit()}
          >
            {S.confirm}
          </Button>
        </>
      }
    >
      <div className="tools-spawn" data-testid="spawn-dialog">
        <p className="adm-muted">{S.at(env.worldName(at.world), at.x, at.z)}</p>
        <Field label={S.search} hint={S.searchHint}>
          <TextInput
            data-autofocus
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
            }}
          />
        </Field>
        {movers.data === null && movers.error === null ? (
          <Skeleton lines={3} />
        ) : movers.error !== null ? (
          <p className="adm-field__error">{S.failed}</p>
        ) : query.trim() === '' ? null : found.length === 0 ? (
          <p className="adm-muted">{S.none}</p>
        ) : (
          <ul className="tools-spawn__list" role="listbox" aria-label={S.search}>
            {found.map((m) => (
              <li key={m.id} role="presentation">
                <button
                  type="button"
                  role="option"
                  aria-selected={choice?.id === m.id}
                  className="tools-spawn__item"
                  onClick={() => {
                    setChoice(m);
                  }}
                >
                  <span>{m.name}</span>
                  <span>
                    {S.columns.level} {m.level}
                  </span>
                  <span>{S.ranks[m.rank] ?? ''}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        {choice ? (
          <p data-testid="spawn-choice">
            <strong>{choice.name}</strong> ({S.columns.level} {choice.level}, {S.columns.id}{' '}
            {choice.id})
          </p>
        ) : null}
        <div className="tools-spawn__row">
          <Field label={S.count}>
            <NumberInput
              min={1}
              max={100}
              value={form.count}
              onChange={(e) => {
                setForm({ ...form, count: e.target.value });
              }}
            />
          </Field>
          <Field label={S.spread}>
            <NumberInput
              min={0}
              max={30}
              value={form.spread}
              onChange={(e) => {
                setForm({ ...form, spread: e.target.value });
              }}
            />
          </Field>
        </div>
        <Field label={S.lifetime}>
          <NumberInput
            min={1}
            value={form.minutes}
            onChange={(e) => {
              setForm({ ...form, minutes: e.target.value });
            }}
          />
        </Field>
        <Checkbox
          label={S.aggressive}
          checked={aggressive}
          onChange={(e) => {
            setAggressive(e.target.checked);
          }}
        />
        {error ? (
          <p className="adm-field__error" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </Dialog>
  );
}

/** Opens the spawn picker for a point. */
export function openSpawnDialog(env: ActionEnv, at: PickedPoint): void {
  openDialog((close) => <SpawnDialog env={env} at={at} onClose={close} />);
}
