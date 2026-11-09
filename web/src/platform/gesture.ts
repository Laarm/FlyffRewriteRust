// First-user-gesture latch (audio unlock, Plan/01 6.8 step 8; Plan/40 section 2.1).

const waiting = new Set<() => void>();
let latched = false;
let installed = false;

const EVENTS = ['pointerdown', 'keydown', 'touchstart'] as const;

function fire(): void {
  latched = true;
  for (const e of EVENTS) {
    document.removeEventListener(e, fire, true);
  }
  installed = false;
  const callbacks = [...waiting];
  waiting.clear();
  for (const cb of callbacks) {
    cb();
  }
}

function install(): void {
  if (installed || latched || typeof document === 'undefined') return;
  installed = true;
  for (const e of EVENTS) {
    document.addEventListener(e, fire, { capture: true });
  }
}

/** `true` once the user has interacted with the page. */
export function hasGesture(): boolean {
  return latched;
}

/** Calls `cb` on the first user gesture (immediately when one already happened). */
export function onFirstGesture(cb: () => void): void {
  if (latched) {
    cb();
    return;
  }
  waiting.add(cb);
  install();
}
