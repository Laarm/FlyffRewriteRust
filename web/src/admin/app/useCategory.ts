// `useCategory`: loads the module of a category (app/views.ts) and reports loading and failure.
import { useCallback, useEffect, useState } from 'react';
import { loadCategory, type CategoryModule } from './views';
import type { CategoryId } from './nav';

export type CategoryState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly module: CategoryModule }
  | { readonly status: 'error'; readonly error: unknown };

type Settled = Exclude<CategoryState, { status: 'loading' }>;

const LOADING: CategoryState = { status: 'loading' };

export function useCategory(id: CategoryId | null): { state: CategoryState; retry: () => void } {
  const [attempt, setAttempt] = useState(0);
  const key = `${id ?? ''}:${String(attempt)}`;
  const [result, setResult] = useState<{ key: string; state: Settled } | null>(null);
  useEffect(() => {
    if (id === null) return;
    let live = true;
    loadCategory(id).then(
      (module) => {
        if (live) setResult({ key, state: { status: 'ready', module } });
      },
      (error: unknown) => {
        if (live) setResult({ key, state: { status: 'error', error } });
      },
    );
    return () => {
      live = false;
    };
  }, [id, key]);
  const retry = useCallback(() => {
    setAttempt((n) => n + 1);
  }, []);
  // a result of another category or attempt counts as "loading" (no state is set from the effect body)
  return { state: result?.key === key ? result.state : LOADING, retry };
}
