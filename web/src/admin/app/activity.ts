// User activity and tab visibility. The server restarts a session's idle timer on EVERY request (`admin.session_idle_min`), so a
// console tab that polls forever would keep a forgotten session alive for ever. The pollers (status pills, alerts) therefore pause
// while the page is hidden or the person has not touched it for `IDLE_PAUSE_MS`; the session then ends by its own idle timeout.
// Any key, pointer or wheel event (or showing the tab again) resumes them at once.

/** No input for this long pauses the pollers. */
export const IDLE_PAUSE_MS = 5 * 60 * 1000;

let lastInput = Date.now();
let installed = false;
const resumeListeners = new Set<() => void>();

function now(): number {
  return Date.now();
}

function onInput(): void {
  const wasIdle = now() - lastInput >= IDLE_PAUSE_MS;
  lastInput = now();
  if (wasIdle) for (const cb of resumeListeners) cb();
}

function onVisibility(): void {
  if (!document.hidden) {
    lastInput = now();
    for (const cb of resumeListeners) cb();
  }
}

function install(): void {
  if (installed || typeof window === 'undefined') return;
  installed = true;
  for (const type of ['pointerdown', 'keydown', 'wheel', 'touchstart']) {
    window.addEventListener(type, onInput, { passive: true, capture: true });
  }
  document.addEventListener('visibilitychange', onVisibility);
}

/** Should a poller do its work now? (the page is visible and someone used it lately) */
export function shouldPoll(at: number = now()): boolean {
  if (typeof document !== 'undefined' && document.hidden) return false;
  return at - lastInput < IDLE_PAUSE_MS;
}

/** Calls `cb` when polling may resume (input after an idle spell, the tab shown again). Returns the remover. */
export function onResume(cb: () => void): () => void {
  install();
  resumeListeners.add(cb);
  return () => {
    resumeListeners.delete(cb);
  };
}

/** Tests: pretend the last input was `msAgo` milliseconds ago. */
export function setLastInputAgoForTests(msAgo: number): void {
  lastInput = now() - msAgo;
}
