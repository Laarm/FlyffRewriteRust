// `usePolling`: runs `task` now and then every `everyMs` while polling is allowed (app/activity.ts: visible tab, recent input).
// A task that throws is ignored (the next tick tries again); the task decides what to show for an error.
import { useEffect, useRef, useState } from 'react';
import { onResume, shouldPoll } from './activity';

export function usePolling(
  task: () => Promise<void> | void,
  everyMs: number,
  enabled = true,
): void {
  const latest = useRef(task);
  useEffect(() => {
    latest.current = task;
  });
  useEffect(() => {
    if (!enabled) return;
    const state = { stopped: false, timer: undefined as ReturnType<typeof setTimeout> | undefined };
    const stopped = (): boolean => state.stopped;
    const run = async (): Promise<void> => {
      if (stopped()) return;
      if (shouldPoll()) {
        try {
          await latest.current();
        } catch {
          // the task reports its own errors
        }
      }
      if (!stopped()) state.timer = setTimeout(() => void run(), everyMs);
    };
    void run();
    const off = onResume(() => {
      clearTimeout(state.timer);
      void run();
    });
    return () => {
      state.stopped = true;
      clearTimeout(state.timer);
      off();
    };
  }, [everyMs, enabled]);
}

/** `Date.now()` re-read every `everyMs` (a render must not call the clock itself). */
export function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => {
      setNow(Date.now());
    }, everyMs);
    return () => {
      clearInterval(id);
    };
  }, [everyMs]);
  return now;
}
