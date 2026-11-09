// Small hooks of the players views.
import { useEffect, useState } from 'react';

/** `value` after it stayed the same for `ms` (a search box that asks the server once the operator paused). */
export function useDebounced<T>(value: T, ms = 300): T {
  const [out, setOut] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => {
      setOut(value);
    }, ms);
    return () => {
      clearTimeout(id);
    };
  }, [value, ms]);
  return out;
}
