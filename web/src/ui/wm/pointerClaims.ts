// The UI's claims on browser pointer events (BFIX1 #004). `UiRoot` listens on the document in the capture phase, which runs
// before the listeners of the 3-D canvas, and asks the window manager whether a window (or an icon of a pass-through
// window such as the buff windows) wants the event. It marks the DOM event it consumed here; the world input
// (`input/pointer.ts`, wired in `app/inputHost.ts`) skips a marked press, move and wheel, so a click on a buff icon never
// walks the character, selects a monster or zooms the camera. Releases and cancels are never marked: a world press must
// always see its release. Nothing is stopped or hidden from other listeners: the mark is a side table keyed by event
// identity (a `WeakSet`, so the events are free to be collected).

const claimed = new WeakSet<object>();

/** The UI consumed this DOM event: the world must not act on it. */
export function markUiClaimed(e: object): void {
  claimed.add(e);
}

/** `true` when {@link markUiClaimed} was called for this very event object. */
export function isUiClaimed(e: object): boolean {
  return claimed.has(e);
}
