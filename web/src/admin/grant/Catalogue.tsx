// The catalogue column (Plan/25 section 5.1): search by name or `#id`, the kind tree, the facets, an icon grid of 48 px cells or a list.
// Clicking an item starts building it; the "+" of a row adds the plain item to the basket at once (the fast path of a grant).
import { useMemo, useState } from 'react';
import * as JOBS from '../../gen/defs/job';
import { Button, Select, TextInput } from '../components';
import { filterCatalog, kindPath, NULL_ID, type CatalogFilter, type CatalogItem } from './catalog';
import { useGrantData, useMeta } from './data';
import { ItemIcon } from './ItemIcon';
import { blankSpec } from './spec';
import { useGrant } from './store';
import { T } from './strings';
import type { KindNode } from '../gen/dto';

const PAGE = 120;

/** The kind tree as a flat list with indentation: `[key, label]`. */
export function flattenKinds(
  tree: readonly KindNode[],
  depth = 0,
): { key: string; label: string }[] {
  const out: { key: string; label: string }[] = [];
  for (const n of tree) {
    out.push({ key: n.key, label: `${'  '.repeat(depth)}${n.name} (${String(n.count)})` });
    out.push(...flattenKinds(n.children, depth + 1));
  }
  return out;
}

/** The jobs a facet can pick, from the generated `JOB_*` definitions (the classes up to the hero). */
export const JOB_CHOICES: readonly { id: number; name: string }[] = Object.entries(JOBS)
  .filter(([k, v]) => k.startsWith('JOB_') && v < 40)
  .map(([k, v]) => ({
    id: v,
    name: k
      .slice(4)
      .toLowerCase()
      .replace(/^\w/, (c) => c.toUpperCase())
      .replace(/_/g, ' '),
  }))
  .sort((a, b) => a.id - b.id);

function Tri({
  label,
  value,
  onChange,
}: {
  readonly label: string;
  readonly value: boolean | null;
  readonly onChange: (v: boolean | null) => void;
}) {
  return (
    <label className="grant-facet">
      <span>{label}</span>
      <Select
        value={value === null ? '' : value ? 'yes' : 'no'}
        onChange={(e) => {
          onChange(e.target.value === '' ? null : e.target.value === 'yes');
        }}
      >
        <option value="">{T.catalogue.any}</option>
        <option value="yes">{T.catalogue.yes}</option>
        <option value="no">{T.catalogue.no}</option>
      </Select>
    </label>
  );
}

function intOrNull(text: string): number | null {
  const n = Math.trunc(Number(text));
  return text.trim() === '' || !Number.isFinite(n) ? null : n;
}

function Facets({
  filter,
  set,
}: {
  readonly filter: CatalogFilter;
  readonly set: (p: Partial<CatalogFilter>) => void;
}) {
  return (
    <div className="grant-facets">
      <label className="grant-facet">
        <span>{T.catalogue.levelFrom}</span>
        <TextInput
          inputMode="numeric"
          value={filter.minLevel ?? ''}
          onChange={(e) => {
            set({ minLevel: intOrNull(e.target.value) });
          }}
        />
      </label>
      <label className="grant-facet">
        <span>{T.catalogue.levelTo}</span>
        <TextInput
          inputMode="numeric"
          value={filter.maxLevel ?? ''}
          onChange={(e) => {
            set({ maxLevel: intOrNull(e.target.value) });
          }}
        />
      </label>
      <label className="grant-facet">
        <span>{T.catalogue.job}</span>
        <Select
          value={filter.job ?? ''}
          onChange={(e) => {
            set({ job: e.target.value === '' ? null : Number(e.target.value) });
          }}
        >
          <option value="">{T.catalogue.any}</option>
          {JOB_CHOICES.map((j) => (
            <option key={j.id} value={j.id}>
              {j.name}
            </option>
          ))}
        </Select>
      </label>
      <label className="grant-facet">
        <span>{T.catalogue.sex}</span>
        <Select
          value={filter.sex ?? ''}
          onChange={(e) => {
            set({ sex: e.target.value === '' ? null : Number(e.target.value) });
          }}
        >
          <option value="">{T.catalogue.any}</option>
          <option value="0">{T.catalogue.male}</option>
          <option value="1">{T.catalogue.female}</option>
        </Select>
      </label>
      <Tri
        label={T.catalogue.stackable}
        value={filter.stackable}
        onChange={(v) => {
          set({ stackable: v });
        }}
      />
      <Tri
        label={T.catalogue.bound}
        value={filter.bound}
        onChange={(v) => {
          set({ bound: v });
        }}
      />
      <Tri
        label={T.catalogue.set}
        value={filter.inSet}
        onChange={(v) => {
          set({ inSet: v });
        }}
      />
      <Tri
        label={T.catalogue.event}
        value={filter.event}
        onChange={(v) => {
          set({ event: v });
        }}
      />
      <label className="grant-facet">
        <span>{T.catalogue.tier}</span>
        <Select
          value={filter.tier}
          onChange={(e) => {
            set({ tier: e.target.value === '2' ? 2 : e.target.value === '1' ? 1 : 0 });
          }}
        >
          <option value={0}>{T.catalogue.any}</option>
          <option value={1}>{T.catalogue.unique}</option>
          <option value={2}>{T.catalogue.ultimate}</option>
        </Select>
      </label>
    </div>
  );
}

export function Catalogue() {
  const data = useGrantData((s) => s.data);
  const phase = useGrantData((s) => s.phase);
  const error = useGrantData((s) => s.error);
  const meta = useMeta((s) => s.meta);
  const filter = useGrant((s) => s.filter);
  const view = useGrant((s) => s.view);
  const selected = useGrant((s) => s.spec?.item_id ?? null);
  const setFilter = useGrant((s) => s.setFilter);
  const setView = useGrant((s) => s.setView);
  const pick = useGrant((s) => s.pick);
  const addSpec = useGrant((s) => s.addSpec);
  const [shown, setShown] = useState(PAGE);
  const [facets, setFacets] = useState(false);

  const kinds = useMemo(() => flattenKinds(meta?.kinds ?? []), [meta]);
  const rows = useMemo(() => (data ? filterCatalog(data.catalog, filter) : []), [data, filter]);
  const visible = rows.slice(0, shown);
  const change = (p: Partial<CatalogFilter>): void => {
    setShown(PAGE);
    setFilter(p);
  };

  if (phase === 'error') {
    return (
      <section className="grant-col grant-catalogue" aria-label={T.catalogue.title}>
        <div className="grant-note grant-note--warn" role="alert">
          {T.catalogue.loadFailed(error ?? '')}
        </div>
      </section>
    );
  }

  const quick = (item: CatalogItem): void => {
    addSpec(blankSpec(item.id));
  };

  return (
    <section className="grant-col grant-catalogue" aria-label={T.catalogue.title}>
      <div className="grant-search">
        <TextInput
          type="search"
          aria-label={T.catalogue.search}
          placeholder={T.catalogue.placeholder}
          data-autofocus
          value={filter.text}
          onChange={(e) => {
            change({ text: e.target.value });
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              const first = rows[0];
              if (first) pick(first.id);
            }
          }}
        />
      </div>
      <div className="grant-inline">
        <Select
          aria-label={T.catalogue.kind}
          value={filter.kind}
          onChange={(e) => {
            change({ kind: e.target.value });
          }}
        >
          <option value="">{T.catalogue.allKinds}</option>
          {kinds.map((k) => (
            <option key={k.key} value={k.key}>
              {k.label}
            </option>
          ))}
        </Select>
        <Button
          size="sm"
          variant={facets ? 'primary' : 'secondary'}
          aria-expanded={facets}
          onClick={() => {
            setFacets(!facets);
          }}
        >
          {T.catalogue.filters}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label={view === 'grid' ? T.catalogue.listView : T.catalogue.gridView}
          onClick={() => {
            setView(view === 'grid' ? 'list' : 'grid');
          }}
        >
          {view === 'grid' ? T.catalogue.list : T.catalogue.grid}
        </Button>
      </div>
      {facets && <Facets filter={filter} set={change} />}
      <div className="grant-count" aria-live="polite">
        {phase === 'loading' || phase === 'idle'
          ? T.catalogue.loading
          : T.catalogue.count(rows.length)}
        {filter.kind !== '' && meta ? ` · ${kindPath(meta.kinds, filter.kind)}` : ''}
      </div>
      <div className={view === 'grid' ? 'grant-grid' : 'grant-list'} role="list">
        {visible.map((item) => (
          <div
            key={item.id}
            role="listitem"
            className={`grant-cell ${selected === item.id ? 'is-selected' : ''}`}
          >
            <button
              type="button"
              className="grant-cell__pick"
              aria-label={`${item.name} #${String(item.id)}`}
              title={`${item.name} (#${String(item.id)})`}
              onClick={() => {
                pick(item.id);
              }}
            >
              <ItemIcon icon={item.icon} size={view === 'grid' ? 48 : 32} />
              {view === 'list' && (
                <span className="grant-cell__text">
                  <span className="grant-cell__name">{item.name}</span>
                  <span className="grant-hint">
                    #{item.id} · {item.level > 0 ? T.builder.level(item.level) : '-'}
                    {item.job !== NULL_ID
                      ? ` · ${JOB_CHOICES.find((j) => j.id === item.job)?.name ?? `job ${String(item.job)}`}`
                      : ''}
                    {item.sex !== NULL_ID
                      ? ` · ${item.sex === 0 ? T.catalogue.male : T.catalogue.female}`
                      : ''}
                  </span>
                </span>
              )}
            </button>
            <button
              type="button"
              className="grant-cell__add"
              aria-label={T.catalogue.quickAdd(item.name)}
              title={T.catalogue.quickAdd(item.name)}
              onClick={() => {
                quick(item);
              }}
            >
              +
            </button>
          </div>
        ))}
      </div>
      {rows.length > shown && (
        <Button
          size="sm"
          onClick={() => {
            setShown(shown + PAGE);
          }}
        >
          {T.catalogue.more(rows.length - shown)}
        </Button>
      )}
    </section>
  );
}
