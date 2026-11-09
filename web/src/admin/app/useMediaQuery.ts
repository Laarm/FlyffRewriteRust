// `useMediaQuery('(max-width: 1279px)')`: true while the query matches (the navigation rail below 1280 px).
import { useEffect, useState } from 'react';

export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() =>
    typeof matchMedia === 'function' ? matchMedia(query).matches : false,
  );
  useEffect(() => {
    if (typeof matchMedia !== 'function') return;
    const list = matchMedia(query);
    const on = (): void => {
      setMatches(list.matches);
    };
    on();
    list.addEventListener('change', on);
    return () => {
      list.removeEventListener('change', on);
    };
  }, [query]);
  return matches;
}
