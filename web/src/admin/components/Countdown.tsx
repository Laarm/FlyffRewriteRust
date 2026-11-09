// Countdown: the time left until a unix timestamp, as `mm:ss` (or `h:mm:ss`), re-rendered every second. Used for the shutdown
// countdown, session expiry and scheduled notices.
import { useEffect, useState } from 'react';

/** `75` -> `1:15`, `3725` -> `1:02:05`; negative values show `0:00`. */
export function formatCountdown(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const two = (n: number): string => String(n).padStart(2, '0');
  return h > 0 ? `${String(h)}:${two(m)}:${two(sec)}` : `${String(m)}:${two(sec)}`;
}

export interface CountdownProps {
  /** Unix seconds of the end. */
  readonly until: number;
  /** The clock (tests inject one); default `Date.now`. */
  readonly now?: () => number;
  readonly onDone?: () => void;
}

export function Countdown({ until, now = Date.now, onDone }: CountdownProps) {
  const [left, setLeft] = useState(() => until - now() / 1000);
  useEffect(() => {
    let done = false;
    const tick = (): void => {
      const remaining = until - now() / 1000;
      setLeft(remaining);
      if (remaining <= 0 && !done) {
        done = true;
        onDone?.();
      }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      clearInterval(id);
    };
  }, [until, now, onDone]);
  return (
    <time className="adm-countdown" dateTime={`PT${String(Math.max(0, Math.floor(left)))}S`}>
      {formatCountdown(left)}
    </time>
  );
}
