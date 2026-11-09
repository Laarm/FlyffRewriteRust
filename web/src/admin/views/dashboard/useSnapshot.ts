// `useSnapshot`: loads a document now and every `everyMs` while polling is allowed (app/activity.ts), restarts when `key` changes
// (a new time span) and when `reload()` is called. The answer of an older key is never shown for a newer one.
import { useCallback, useEffect, useRef, useState } from 'react';
import { onResume, shouldPoll } from '../../app/activity';

export interface Snapshot<T> {
  /** The latest answer for the current key (`null` until it arrives). */
  readonly data: T | null;
  /** The last failure (kept until the next success). */
  readonly error: unknown;
  readonly loading: boolean;
  readonly reload: () => void;
}

export function useSnapshot<T>(
  load: (signal: AbortSignal) => Promise<T>,
  key: string,
  everyMs: number,
): Snapshot<T> {
  const [epoch, setEpoch] = useState(0);
  const [state, setState] = useState<{ key: string; data: T | null; error: unknown }>({
    key: '',
    data: null,
    error: null,
  });
  const latest = useRef(load);
  useEffect(() => {
    latest.current = load;
  });
  const full = `${key}|${String(epoch)}`;

  useEffect(() => {
    const controller = new AbortController();
    const stopped = (): boolean => controller.signal.aborted;
    const run = async (): Promise<void> => {
      if (stopped()) return;
      try {
        const data = await latest.current(controller.signal);
        if (!controller.signal.aborted) setState({ key: full, data, error: null });
      } catch (error) {
        if (controller.signal.aborted) return;
        setState((s) => ({ key: full, data: s.key === full ? s.data : null, error }));
      }
    };
    let timer: ReturnType<typeof setTimeout> | undefined;
    // a hidden or forgotten tab skips the load but keeps the clock, so the data is fresh again when it is shown
    const schedule = (): void => {
      timer = setTimeout(() => {
        if (stopped()) return;
        if (shouldPoll()) void loop();
        else schedule();
      }, everyMs);
    };
    const loop = async (): Promise<void> => {
      await run();
      if (!stopped()) schedule();
    };
    void loop();
    const off = onResume(() => {
      clearTimeout(timer);
      void loop();
    });
    return () => {
      controller.abort();
      clearTimeout(timer);
      off();
    };
  }, [full, everyMs]);

  const reload = useCallback(() => {
    setEpoch((n) => n + 1);
  }, []);
  const current = state.key === full;
  return {
    data: current ? state.data : null,
    error: state.error,
    loading: !current || (state.data === null && state.error === null),
    reload,
  };
}
