// A cursor-paged list from the admin API (`?cursor=&limit=` -> `{items, next_cursor}`, Plan/25 section 11.3): loads the first page,
// appends more on `loadMore`, restarts when the path or the query change and aborts what is in flight. Views feed `Table` with it.
import { useCallback, useEffect, useState } from 'react';
import { fetchPage, type RequestOptions } from '../api/rest';

export interface PagedList<T> {
  readonly rows: readonly T[];
  readonly loading: boolean;
  readonly loadingMore: boolean;
  readonly error: unknown;
  readonly hasMore: boolean;
  readonly loadMore: () => void;
  readonly reload: () => void;
}

export interface PagedListOptions {
  readonly path: string;
  readonly query?: RequestOptions['query'];
  /** `false` keeps the list idle (a tab that is not open, a missing capability). */
  readonly enabled?: boolean;
  readonly limit?: number;
}

/** What was loaded for one request key (path, query, limit, reload count); a result of an older key is ignored. */
interface Loaded<T> {
  readonly key: string;
  readonly rows: readonly T[];
  readonly next: string | null;
  readonly error: unknown;
  readonly more: boolean;
}

export function usePagedList<T>({
  path,
  query,
  enabled = true,
  limit = 100,
}: PagedListOptions): PagedList<T> {
  const [epoch, setEpoch] = useState(0);
  const queryKey = JSON.stringify(query ?? {});
  const key = `${path}|${queryKey}|${String(limit)}|${String(epoch)}`;
  const [loaded, setLoaded] = useState<Loaded<T> | null>(null);

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    fetchPage<T>(
      path,
      { ...(JSON.parse(queryKey) as Record<string, string>), limit },
      null,
      controller.signal,
    ).then(
      (page) => {
        if (!controller.signal.aborted)
          setLoaded({ key, rows: page.items, next: page.next_cursor, error: null, more: false });
      },
      (error: unknown) => {
        if (!controller.signal.aborted)
          setLoaded({ key, rows: [], next: null, error, more: false });
      },
    );
    return () => {
      controller.abort();
    };
  }, [path, queryKey, limit, key, enabled]);

  const current = loaded?.key === key ? loaded : null;

  const loadMore = useCallback(() => {
    const cursor = current?.next;
    if (current === null || cursor == null || current.more) return;
    setLoaded({ ...current, more: true });
    fetchPage<T>(path, { ...(JSON.parse(queryKey) as Record<string, string>), limit }, cursor).then(
      (page) => {
        setLoaded((c) =>
          c?.key === key
            ? {
                key,
                rows: [...c.rows, ...page.items],
                next: page.next_cursor,
                error: null,
                more: false,
              }
            : c,
        );
      },
      (error: unknown) => {
        setLoaded((c) => (c?.key === key ? { ...c, error, more: false } : c));
      },
    );
  }, [current, path, queryKey, limit, key]);

  const reload = useCallback(() => {
    setEpoch((n) => n + 1);
  }, []);

  return {
    rows: current?.rows ?? [],
    loading: enabled && current === null,
    loadingMore: current?.more ?? false,
    error: current?.error ?? null,
    hasMore: current?.next != null,
    loadMore,
    reload,
  };
}
