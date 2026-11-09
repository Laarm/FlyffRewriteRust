// Items & Economy -> Item search (Plan/25 section 5.5): where an item is, over every bag, bank, guild bank, mail, waiting grant and the
// ground. The server streams the hits as it scans (NDJSON) with progress; the operator can cancel, and the duplicate-serial report
// runs the same scan for serials that exist twice.
import { useEffect, useRef, useState } from 'react';
import type { TabProps } from '../../app/views';
import {
  Badge,
  Button,
  Field,
  NumberInput,
  Select,
  Table,
  TextInput,
  type Column,
} from '../../components';
import { toast } from '../../stores/toasts';
import { withRecord } from '../../app/route';
import type {
  DuplicateDto,
  DuplicatesDto,
  ItemHitDto,
  ItemSearchBody,
  SearchEvent,
} from '../../gen/dto';
import { fetchDuplicates, searchItems } from '../../grant/api';
import { ensureGrantData, useGrantData } from '../../grant/data';
import { formatNumber } from '../../grant/format';
import { ItemPicker } from '../../grant/ItemPicker';
import { ELEMENTS } from '../../grant/ranges';
import { getRoute, navigate } from '../../stores/route';
import { T } from './strings';

/** The hits shown at most (a result list of 10,000 rows is not a list anybody reads). */
export const SHOWN_MAX = 2000;

interface RunState {
  readonly running: boolean;
  readonly hits: readonly ItemHitDto[];
  readonly live: number;
  readonly skipped: number;
  readonly scanned: number;
  readonly total: number | null;
  readonly done: {
    readonly hits: number;
    readonly scanned: number;
    readonly truncated: boolean;
  } | null;
  readonly error: string | null;
  readonly cancelled: boolean;
}

const IDLE: RunState = {
  running: false,
  hits: [],
  live: 0,
  skipped: 0,
  scanned: 0,
  total: null,
  done: null,
  error: null,
  cancelled: false,
};

/** Where a hit lies, in words. */
export function placeOf(h: ItemHitDto): string {
  const site = T.search.sites[h.site] ?? h.site;
  switch (h.site) {
    case 'ground':
      return h.ground
        ? `${site}, ${T.search.ground(h.ground.world, h.ground.x, h.ground.z)}`
        : site;
    case 'mail':
      return `${site}, ${T.search.mail(h.mail_id ?? 0)}`;
    case 'pending_grant':
      return `${site}, ${T.search.grant(h.grant_id ?? 0)}`;
    case 'guild_bank':
      return `${site}, ${T.search.guild(h.guild_name, h.guild_id)}`;
    case 'pocket':
      return `${site} ${T.search.pocket(h.pocket)}`;
    default:
      return site;
  }
}

function ownerOf(h: ItemHitDto): string {
  if (h.character_name !== null)
    return h.account_name ? `${h.character_name} (${h.account_name})` : h.character_name;
  return '';
}

/** One step of the stream applied to the state. */
export function applyEvent(s: RunState, ev: SearchEvent): RunState {
  switch (ev.type) {
    case 'start':
      return { ...s, live: ev.live, skipped: ev.skipped_accounts };
    case 'hit': {
      if (s.hits.length >= SHOWN_MAX) return s;
      return { ...s, hits: [...s.hits, ev] };
    }
    case 'progress':
      return { ...s, scanned: ev.scanned, total: ev.total };
    case 'done':
      return {
        ...s,
        running: false,
        scanned: ev.scanned,
        done: { hits: ev.hits, scanned: ev.scanned, truncated: ev.truncated },
      };
    case 'error':
      return { ...s, running: false, error: ev.message };
  }
}

function DuplicatesPanel() {
  const [state, setState] = useState<{
    running: boolean;
    result: DuplicatesDto | null;
    error: string | null;
  }>({
    running: false,
    result: null,
    error: null,
  });
  const run = async (): Promise<void> => {
    setState({ running: true, result: null, error: null });
    try {
      setState({ running: false, result: await fetchDuplicates(), error: null });
    } catch (e) {
      setState({ running: false, result: null, error: e instanceof Error ? e.message : String(e) });
    }
  };
  const columns: Column<DuplicateDto>[] = [
    {
      id: 'serial',
      header: T.search.serial,
      cell: (d) => d.serial,
      text: (d) => d.serial,
      align: 'right',
    },
    {
      id: 'places',
      header: T.search.site,
      cell: (d) => (
        <ul className="grant-lines">
          {d.places.map((p, i) => (
            <li key={i}>
              {T.search.sites[p.site] ?? p.site}
              {p.character_name ? ` · ${p.character_name}` : ''} · #{p.item_id} x{p.count}
            </li>
          ))}
        </ul>
      ),
      text: (d) => d.places.map((p) => `${p.site} ${p.character_name ?? ''}`).join(' '),
    },
  ];
  return (
    <section className="items-tab" aria-label={T.search.dupTitle}>
      <div className="items-toolbar">
        <Button loading={state.running} onClick={() => void run()}>
          {T.search.duplicates}
        </Button>
        {state.running && <span className="grant-hint">{T.search.dupRunning}</span>}
        {state.result && (
          <span className="grant-hint">
            {state.result.duplicates.length === 0
              ? T.search.dupNone
              : T.search.dupFound(state.result.duplicates.length, state.result.scanned)}
            {state.result.truncated ? ` ${T.search.dupTruncated}` : ''}
          </span>
        )}
        {state.error && <span className="adm-field__error">{state.error}</span>}
      </div>
      {state.result && state.result.duplicates.length > 0 && (
        <Table
          columns={columns}
          rows={state.result.duplicates}
          rowKey={(d) => String(d.serial)}
          label={T.search.dupTitle}
          exportName="duplicate-serials"
        />
      )}
    </section>
  );
}

export function SearchTab({ route }: TabProps) {
  const data = useGrantData((s) => s.data);
  // `#/items/search?item=2154` (the item drawer) starts with that item; the operator's own choice wins from then on
  const wantedParam = route.params['item'];
  const wantedId = wantedParam === undefined ? null : Number(wantedParam);
  const [picked, setItem] = useState<number | null | undefined>(undefined);
  const item =
    picked === undefined
      ? wantedId !== null && Number.isInteger(wantedId) && wantedId > 0
        ? wantedId
        : null
      : picked;
  const [serial, setSerial] = useState('');
  const [minUpgrade, setMinUpgrade] = useState('');
  const [element, setElement] = useState('');
  const [maxHits, setMaxHits] = useState(2000);
  const [run, setRun] = useState<RunState>(IDLE);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => {
    void ensureGrantData();
    return () => {
      controller.current?.abort();
    };
  }, []);

  const body = (): ItemSearchBody | null => {
    const serialNum = serial.trim() === '' ? null : Number(serial);
    if (item === null && (serialNum === null || !Number.isFinite(serialNum))) return null;
    return {
      ...(item === null ? {} : { item_id: item }),
      ...(serialNum !== null && Number.isFinite(serialNum) ? { serial: serialNum } : {}),
      ...(minUpgrade.trim() === '' ? {} : { min_upgrade: Math.trunc(Number(minUpgrade)) }),
      ...(element === '' ? {} : { element: Number(element) }),
      max_hits: maxHits,
    };
  };
  const ready = body() !== null;

  const start = async (): Promise<void> => {
    const b = body();
    if (!b) {
      toast.info(T.search.needsFilter);
      return;
    }
    controller.current?.abort();
    const ctl = new AbortController();
    controller.current = ctl;
    let state: RunState = { ...IDLE, running: true };
    setRun(state);
    // the hits arrive faster than a render: apply them as they come, show them every 150 ms
    let dirty = false;
    const flush = setInterval(() => {
      if (dirty) {
        dirty = false;
        setRun(state);
      }
    }, 150);
    try {
      await searchItems(
        b,
        (ev) => {
          state = applyEvent(state, ev);
          dirty = true;
        },
        ctl.signal,
      );
      state = { ...state, running: false };
    } catch (e) {
      state = ctl.signal.aborted
        ? { ...state, running: false, cancelled: true }
        : { ...state, running: false, error: e instanceof Error ? e.message : String(e) };
    } finally {
      clearInterval(flush);
      setRun(state);
    }
  };
  const cancel = (): void => {
    controller.current?.abort();
  };

  const columns: Column<ItemHitDto>[] = [
    {
      id: 'where',
      header: T.search.site,
      cell: (h) => (
        <>
          {placeOf(h)}{' '}
          {h.site === 'guild_bank' && h.guild_id !== null ? (
            <a
              href={`#/guilds?g=${String(h.guild_id)}`}
              onClick={(e) => {
                e.stopPropagation();
              }}
            >
              {T.search.openGuild}
            </a>
          ) : null}{' '}
          <Badge tone={h.source === 'live' ? 'ok' : 'neutral'}>
            {h.source === 'live' ? T.search.live : T.search.db}
          </Badge>
        </>
      ),
      text: placeOf,
      sortable: true,
      filterable: true,
    },
    {
      id: 'owner',
      header: T.search.owner,
      cell: (h) =>
        h.character_id !== null ? (
          <button
            type="button"
            className="grant-link"
            aria-label={T.search.openCharacter(h.character_name ?? String(h.character_id))}
            onClick={() => {
              const here = getRoute();
              if (here)
                navigate(withRecord(here, { kind: 'character', id: String(h.character_id) }));
            }}
          >
            {ownerOf(h)}
          </button>
        ) : (
          ownerOf(h)
        ),
      text: ownerOf,
      sortable: true,
      filterable: true,
    },
    {
      id: 'slot',
      header: T.search.slot,
      cell: (h) => h.slot ?? '',
      text: (h) => h.slot ?? '',
      align: 'right',
    },
    {
      id: 'item',
      header: T.search.itemCol,
      cell: (h) =>
        `${data?.itemName(h.item.item_id) ?? ''} #${String(h.item.item_id)}${h.item.count > 1 ? ` x${formatNumber(h.item.count)}` : ''}${h.item.upgrade_level > 0 ? ` +${String(h.item.upgrade_level)}` : ''}`,
      text: (h) => `${data?.itemName(h.item.item_id) ?? ''} ${String(h.item.item_id)}`,
      filterable: true,
    },
    {
      id: 'serial',
      header: T.search.serial,
      cell: (h) => h.item.serial,
      text: (h) => h.item.serial,
      align: 'right',
      sortable: true,
    },
    {
      id: 'saved',
      header: T.search.saved,
      cell: (h) =>
        h.saved_at ? new Date(h.saved_at * 1000).toISOString().slice(0, 16).replace('T', ' ') : '',
      text: (h) => h.saved_at ?? 0,
    },
  ];

  const percent =
    run.total !== null && run.total > 0
      ? Math.min(100, Math.round((run.scanned / run.total) * 100))
      : 0;
  return (
    <div className="items-tab">
      <p className="grant-hint">{T.search.hint}</p>
      <form
        className="items-toolbar"
        onSubmit={(e) => {
          e.preventDefault();
          void start();
        }}
      >
        <Field label={T.search.item}>
          <ItemPicker
            value={item}
            onChange={setItem}
            label={T.search.item}
            placeholder="Name or #id"
          />
        </Field>
        <Field label={T.search.serial}>
          <TextInput
            inputMode="numeric"
            value={serial}
            onChange={(e) => {
              setSerial(e.target.value);
            }}
          />
        </Field>
        <Field label={T.search.minUpgrade}>
          <NumberInput
            min={0}
            max={20}
            value={minUpgrade}
            onChange={(e) => {
              setMinUpgrade(e.target.value);
            }}
          />
        </Field>
        <Field label={T.search.element}>
          <Select
            value={element}
            onChange={(e) => {
              setElement(e.target.value);
            }}
          >
            <option value="">{T.search.any}</option>
            {ELEMENTS.map((el) => (
              <option key={el.id} value={el.id}>
                {el.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={T.search.maxHits}>
          <NumberInput
            min={1}
            max={10_000}
            value={maxHits}
            onChange={(e) => {
              setMaxHits(Math.min(10_000, Math.max(1, Math.trunc(Number(e.target.value)) || 2000)));
            }}
          />
        </Field>
        <Button type="submit" variant="primary" loading={run.running} disabled={!ready}>
          {T.search.run}
        </Button>
        {run.running && (
          <Button variant="danger" onClick={cancel}>
            {T.search.cancel}
          </Button>
        )}
      </form>
      {(run.running || run.done) && (
        <div className="adm-stack adm-stack--tight" aria-live="polite">
          {run.running && (
            <div
              className="items-progress"
              role="progressbar"
              aria-valuenow={percent}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <div style={{ width: `${String(percent)}%` }} />
            </div>
          )}
          <span className="grant-hint">
            {run.running
              ? T.search.scanning(run.scanned, run.total)
              : run.done
                ? T.search.finished(run.done.hits, run.done.scanned)
                : ''}
            {run.live > 0 || run.skipped > 0 ? ` ${T.search.started(run.live, run.skipped)}` : ''}
          </span>
        </div>
      )}
      {run.done?.truncated === true && (
        <div className="grant-note grant-note--warn">{T.search.truncated}</div>
      )}
      {run.cancelled && <div className="grant-note">{T.search.cancelled}</div>}
      {run.error && (
        <div className="grant-note grant-note--warn" role="alert">
          {run.error}
        </div>
      )}
      {(run.hits.length > 0 || run.done) && (
        <Table
          columns={columns}
          rows={run.hits}
          rowKey={(h) =>
            `${h.site}:${String(h.character_id ?? h.guild_id ?? h.mail_id ?? h.grant_id ?? 0)}:${String(h.slot ?? 0)}:${String(h.item.serial)}:${String(h.item.item_id)}:${h.source}:${String(h.pocket ?? 0)}`
          }
          label={T.search.title}
          emptyTitle={T.search.none}
          exportName="item-search"
        />
      )}
      <DuplicatesPanel />
    </div>
  );
}
