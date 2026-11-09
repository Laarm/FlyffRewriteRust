// visibilitychange / pagehide / focus as typed events (Plan/40 section 2.1, 10.5).

export type VisibilityEvent = 'visible' | 'hidden' | 'pagehide' | 'focus' | 'blur';
export type Unsubscribe = () => void;

/** `true` while the tab is hidden (always `false` outside a browser). */
export function isHidden(): boolean {
  return typeof document !== 'undefined' && document.visibilityState === 'hidden';
}

/** Subscribes to page lifecycle events; returns the unsubscribe function. */
export function onVisibility(cb: (e: VisibilityEvent) => void): Unsubscribe {
  if (typeof document === 'undefined' || typeof window === 'undefined') {
    return () => undefined;
  }
  const onChange = () => {
    cb(isHidden() ? 'hidden' : 'visible');
  };
  const onPageHide = () => {
    cb('pagehide');
  };
  const onFocus = () => {
    cb('focus');
  };
  const onBlur = () => {
    cb('blur');
  };
  document.addEventListener('visibilitychange', onChange);
  window.addEventListener('pagehide', onPageHide);
  window.addEventListener('focus', onFocus);
  window.addEventListener('blur', onBlur);
  return () => {
    document.removeEventListener('visibilitychange', onChange);
    window.removeEventListener('pagehide', onPageHide);
    window.removeEventListener('focus', onFocus);
    window.removeEventListener('blur', onBlur);
  };
}
