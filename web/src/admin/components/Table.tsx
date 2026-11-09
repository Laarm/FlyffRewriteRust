// Table of the admin design system (Plan/25 section 2.3): server-side cursor paging (a "Load more" row), sort, column filters,
// CSV export (PII columns only with `view.pii`), sticky header and keyboard navigation (Up/Down/Home/End/PageUp/PageDown move the
// active row, Enter or a click activates it). Sorting and filtering run on the server when the caller gives `onSortChange` /
// `onFiltersChange` (the lists are paged there), else on the rows in hand.
import { useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { useCan } from '../stores/session';
import { Button } from './Button';
import { EmptyState, ErrorState } from './EmptyState';
import { Icon } from './Icon';
import { Skeleton } from './Spinner';

export interface Column<T> {
  readonly id: string;
  readonly header: string;
  readonly cell: (row: T) => ReactNode;
  /** Plain text of the cell: the CSV value, and what a client-side filter matches. */
  readonly text?: (row: T) => string | number;
  readonly sortable?: boolean;
  /** Client-side sort key (without it a client-side sort uses `text`). */
  readonly sortValue?: (row: T) => string | number;
  readonly filterable?: boolean;
  /** Personal data (IPs, e-mails): hidden and left out of the CSV without `view.pii`. */
  readonly pii?: boolean;
  readonly width?: string;
  readonly align?: 'left' | 'right' | 'center';
}

export interface SortState {
  readonly column: string;
  readonly dir: 'asc' | 'desc';
}

export interface TableProps<T> {
  readonly columns: readonly Column<T>[];
  readonly rows: readonly T[];
  readonly rowKey: (row: T) => string;
  /** Accessible name of the table. */
  readonly label: string;
  readonly loading?: boolean;
  readonly error?: unknown;
  readonly onRetry?: () => void;
  readonly emptyTitle?: string;
  readonly emptyHint?: ReactNode;
  readonly sort?: SortState | null;
  readonly onSortChange?: (sort: SortState | null) => void;
  readonly filters?: Readonly<Record<string, string>>;
  readonly onFiltersChange?: (filters: Record<string, string>) => void;
  readonly hasMore?: boolean;
  readonly loadingMore?: boolean;
  readonly onLoadMore?: () => void;
  readonly onRowActivate?: (row: T) => void;
  readonly selectedKey?: string | null;
  /** Shows the CSV button; the value is the file name without extension. */
  readonly exportName?: string;
  /** Overrides the session's `view.pii` (tests, views that already know). */
  readonly canPii?: boolean;
  readonly toolbar?: ReactNode;
}

// ---- pure helpers (tested) ----------------------------------------------------------------------------------------------

/** Columns the viewer may see. */
export function visibleColumns<T>(
  columns: readonly Column<T>[],
  canPii: boolean,
): readonly Column<T>[] {
  return columns.filter((c) => canPii || c.pii !== true);
}

function textOf<T>(c: Column<T>, row: T): string {
  return c.text ? String(c.text(row)) : '';
}

/** Client-side filter: every non-empty column filter must be a case-insensitive substring of the column's text. */
export function filterRows<T>(
  rows: readonly T[],
  columns: readonly Column<T>[],
  filters: Readonly<Record<string, string>>,
): readonly T[] {
  const active = Object.entries(filters).filter(([, v]) => v.trim() !== '');
  if (active.length === 0) return rows;
  return rows.filter((row) =>
    active.every(([id, needle]) => {
      const col = columns.find((c) => c.id === id);
      return col ? textOf(col, row).toLowerCase().includes(needle.trim().toLowerCase()) : true;
    }),
  );
}

/** Client-side sort (stable; numbers compare as numbers, text with the locale's numeric collation). */
export function sortRows<T>(
  rows: readonly T[],
  columns: readonly Column<T>[],
  sort: SortState | null,
): readonly T[] {
  if (!sort) return rows;
  const col = columns.find((c) => c.id === sort.column);
  if (!col) return rows;
  const key = (row: T): string | number =>
    col.sortValue ? col.sortValue(row) : col.text ? col.text(row) : '';
  const sign = sort.dir === 'asc' ? 1 : -1;
  return rows
    .map((row, i) => ({ row, i, k: key(row) }))
    .sort((a, b) => {
      const c =
        typeof a.k === 'number' && typeof b.k === 'number'
          ? a.k - b.k
          : String(a.k).localeCompare(String(b.k), undefined, {
              numeric: true,
              sensitivity: 'base',
            });
      return c !== 0 ? sign * c : a.i - b.i;
    })
    .map((x) => x.row);
}

/** A spreadsheet reads a cell that starts with one of these as a formula: such text gets a leading apostrophe (CSV injection). */
const FORMULA_START = /^[=+\-@\t\r]/;
const PLAIN_NUMBER = /^-?\d+(\.\d+)?$/;

function csvCell(value: string | number): string {
  let text = String(value);
  if (typeof value === 'string' && FORMULA_START.test(text) && !PLAIN_NUMBER.test(text))
    text = `'${text}`;
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** RFC 4180 CSV of the rows: a header line, `\r\n` line ends, PII columns only when `canPii`. */
export function toCsv<T>(
  columns: readonly Column<T>[],
  rows: readonly T[],
  canPii: boolean,
): string {
  const cols = visibleColumns(columns, canPii);
  const lines = [cols.map((c) => csvCell(c.header)).join(',')];
  for (const row of rows) {
    lines.push(cols.map((c) => csvCell(c.text ? c.text(row) : '')).join(','));
  }
  return `${lines.join('\r\n')}\r\n`;
}

function download(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/csv;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = `${name}.csv`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => {
    URL.revokeObjectURL(url);
  }, 1000);
}

const NO_FILTERS: Readonly<Record<string, string>> = {};

/** Where the keyboard moves the active row (`null`: not a movement key). */
export function nextActiveRow(key: string, from: number, last: number): number | null {
  switch (key) {
    case 'ArrowDown':
      return Math.min(last, from + 1);
    case 'ArrowUp':
      return Math.max(0, from - 1);
    case 'Home':
      return 0;
    case 'End':
      return Math.max(0, last);
    case 'PageDown':
      return Math.min(last, from + 10);
    case 'PageUp':
      return Math.max(0, from - 10);
    default:
      return null;
  }
}

// ---- the component --------------------------------------------------------------------------------------------------------

export function Table<T>(props: TableProps<T>) {
  const {
    columns,
    rows,
    rowKey,
    label,
    loading = false,
    error,
    onRetry,
    emptyTitle = 'Nothing here',
    emptyHint,
    sort = null,
    onSortChange,
    filters,
    onFiltersChange,
    hasMore = false,
    loadingMore = false,
    onLoadMore,
    onRowActivate,
    selectedKey = null,
    exportName,
    toolbar,
  } = props;
  const sessionPii = useCan('view.pii');
  const canPii = props.canPii ?? sessionPii;
  const baseId = useId();
  const wrapRef = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(0);
  const [localSort, setLocalSort] = useState<SortState | null>(null);
  const [localFilters, setLocalFilters] = useState<Record<string, string>>({});

  const cols = useMemo(() => visibleColumns(columns, canPii), [columns, canPii]);
  const serverSort = onSortChange !== undefined;
  const serverFilter = onFiltersChange !== undefined;
  const effSort = serverSort ? sort : localSort;
  const effFilters = serverFilter ? (filters ?? NO_FILTERS) : localFilters;

  const shown = useMemo(() => {
    const filtered = serverFilter ? rows : filterRows(rows, cols, effFilters);
    return serverSort ? filtered : sortRows(filtered, cols, effSort);
  }, [rows, cols, effFilters, effSort, serverFilter, serverSort]);

  const rowId = (i: number): string => `${baseId}-r${String(i)}`;
  const activeIndex = Math.min(active, Math.max(0, shown.length - 1));
  const filterable = cols.some((c) => c.filterable === true);

  const changeSort = (c: Column<T>): void => {
    if (c.sortable !== true) return;
    const next: SortState | null =
      effSort?.column !== c.id
        ? { column: c.id, dir: 'asc' }
        : effSort.dir === 'asc'
          ? { column: c.id, dir: 'desc' }
          : null;
    if (onSortChange) onSortChange(next);
    else setLocalSort(next);
  };

  const changeFilter = (id: string, value: string): void => {
    const next = { ...effFilters, [id]: value };
    if (onFiltersChange) onFiltersChange(next);
    else setLocalFilters(next);
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    if ((event.target as HTMLElement).tagName === 'INPUT') return;
    if (event.key === 'Enter' || event.key === ' ') {
      const row = shown[activeIndex];
      if (row !== undefined && onRowActivate) {
        event.preventDefault();
        onRowActivate(row);
      }
      return;
    }
    const next = nextActiveRow(event.key, activeIndex, shown.length - 1);
    if (next === null) return;
    event.preventDefault();
    setActive(next);
    document.getElementById(rowId(next))?.scrollIntoView({ block: 'nearest' });
  };

  const body = ((): ReactNode => {
    if (error !== undefined && error !== null && rows.length === 0) {
      return <ErrorState error={error} {...(onRetry ? { onRetry } : {})} />;
    }
    if (loading && rows.length === 0) return <Skeleton lines={6} height={18} />;
    if (shown.length === 0) {
      return <EmptyState title={emptyTitle} {...(emptyHint ? { hint: emptyHint } : {})} />;
    }
    return null;
  })();

  return (
    <div className="adm-table">
      {(toolbar !== undefined || exportName !== undefined) && (
        <div className="adm-table__bar">
          <div className="adm-row adm-grow">{toolbar}</div>
          {exportName ? (
            <Button
              size="sm"
              icon="download"
              disabled={shown.length === 0}
              onClick={() => {
                download(exportName, toCsv(cols, shown, canPii));
              }}
            >
              Export CSV
            </Button>
          ) : null}
        </div>
      )}
      <div
        ref={wrapRef}
        className="adm-table__scroll"
        role="grid"
        aria-label={label}
        aria-rowcount={shown.length + 1}
        aria-busy={loading || undefined}
        aria-activedescendant={shown.length > 0 ? rowId(activeIndex) : undefined}
        tabIndex={0}
        onKeyDown={onKeyDown}
      >
        <table>
          <thead>
            <tr role="row">
              {cols.map((c) => {
                const dir = effSort?.column === c.id ? effSort.dir : null;
                return (
                  <th
                    key={c.id}
                    role="columnheader"
                    scope="col"
                    aria-sort={
                      dir === 'asc'
                        ? 'ascending'
                        : dir === 'desc'
                          ? 'descending'
                          : c.sortable
                            ? 'none'
                            : undefined
                    }
                    style={{ width: c.width, textAlign: c.align }}
                  >
                    {c.sortable === true ? (
                      <button
                        type="button"
                        className="adm-th-btn"
                        onClick={() => {
                          changeSort(c);
                        }}
                      >
                        {c.header}
                        <Icon
                          name={dir === 'desc' ? 'sortDesc' : 'sortAsc'}
                          size={12}
                          style={{ opacity: dir ? 1 : 0.35 }}
                        />
                      </button>
                    ) : (
                      c.header
                    )}
                  </th>
                );
              })}
            </tr>
            {filterable && (
              <tr role="row" className="adm-table__filters">
                {cols.map((c) => (
                  <th key={c.id} role="gridcell">
                    {c.filterable === true ? (
                      <input
                        className="adm-input adm-input--sm"
                        type="search"
                        aria-label={`Filter ${c.header}`}
                        placeholder="Filter"
                        value={effFilters[c.id] ?? ''}
                        onChange={(e) => {
                          changeFilter(c.id, e.target.value);
                        }}
                      />
                    ) : null}
                  </th>
                ))}
              </tr>
            )}
          </thead>
          <tbody>
            {shown.map((row, i) => {
              const key = rowKey(row);
              return (
                <tr
                  key={key}
                  id={rowId(i)}
                  role="row"
                  aria-rowindex={i + 2}
                  aria-selected={selectedKey === key ? true : undefined}
                  className={`${i === activeIndex ? 'is-active' : ''} ${selectedKey === key ? 'is-selected' : ''} ${onRowActivate ? 'is-clickable' : ''}`}
                  onClick={() => {
                    setActive(i);
                    onRowActivate?.(row);
                  }}
                >
                  {cols.map((c) => (
                    <td key={c.id} role="gridcell" style={{ textAlign: c.align }}>
                      {c.cell(row)}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
        {body}
        {hasMore && onLoadMore ? (
          <div className="adm-table__more">
            <Button loading={loadingMore} onClick={onLoadMore}>
              Load more
            </Button>
          </div>
        ) : null}
      </div>
    </div>
  );
}
