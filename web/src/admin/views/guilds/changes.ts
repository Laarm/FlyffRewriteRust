// "A guild changed": the list of guilds is a page-by-page read and does not poll, so every mutation of this category tells the list to read again
// (a logo changed in the drawer shows in the list at once). A plain DOM event: no store, nothing to clean up but the listener.
import { useEffect } from 'react';

const EVENT = 'admin:guilds-changed';

/** Tells the open views that a guild was created, changed or disbanded. */
export function notifyGuildsChanged(): void {
  window.dispatchEvent(new Event(EVENT));
}

/** Calls `onChange` whenever [`notifyGuildsChanged`] runs while the component is mounted. */
export function useGuildsChanged(onChange: () => void): void {
  useEffect(() => {
    window.addEventListener(EVENT, onChange);
    return () => {
      window.removeEventListener(EVENT, onChange);
    };
  }, [onChange]);
}
