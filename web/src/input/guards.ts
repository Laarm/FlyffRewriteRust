// Browser guards of the game surfaces (Plan/43 §3.8): the pointer defaults that would act on the page instead of the game.
// The key guards live in the router (`shouldPreventDefault`); these are the document-wide pointer ones, so that they cover the
// DOM windows as well as the `#world` canvas:
//
//   contextmenu                    cancelled (a right drag ends on whatever is under the pointer), except on native text inputs
//   mousedown, middle button       cancelled (Windows auto-scroll); the camera drag of the flying camera uses it
//   mousedown / mouseup / pointerup, buttons 3 and 4 (back / forward)  cancelled (history navigation)
//   auxclick                       cancelled
//   dragstart                      cancelled (images and links of the page are not draggable game objects)

/** A native editable element: its context menu (copy and paste) and its drags stay the browser's. */
export function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as { tagName?: unknown; isContentEditable?: unknown } | null;
  if (el === null || typeof el.tagName !== 'string') return false;
  const tag = el.tagName.toUpperCase();
  return tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable === true;
}

interface PointerLike {
  readonly button: number;
  readonly target: EventTarget | null;
  preventDefault(): void;
}

/**
 * Installs the guards on `doc` (capture phase); `active()` says whether the input layer owns the page right now (the boot and
 * fatal screens keep the browser's own context menu). Returns the remover.
 */
export function installBrowserGuards(doc: EventTarget, active: () => boolean): () => void {
  const options: AddEventListenerOptions = { capture: true };
  const on = (type: string, fn: (e: PointerLike) => void): (() => void) => {
    const listener = (e: Event): void => {
      fn(e as unknown as PointerLike);
    };
    doc.addEventListener(type, listener, options);
    return () => {
      doc.removeEventListener(type, listener, options);
    };
  };
  const cancelUnlessEditable = (e: PointerLike): void => {
    if (active() && !isEditableTarget(e.target)) e.preventDefault();
  };
  const offs = [
    on('contextmenu', cancelUnlessEditable),
    on('auxclick', cancelUnlessEditable),
    on('dragstart', cancelUnlessEditable),
    on('mousedown', (e) => {
      if (!active()) return;
      if (e.button >= 3) e.preventDefault();
      else if (e.button === 1 && !isEditableTarget(e.target)) e.preventDefault();
    }),
    on('mouseup', (e) => {
      if (active() && e.button >= 3) e.preventDefault();
    }),
    on('pointerup', (e) => {
      if (active() && e.button >= 3) e.preventDefault();
    }),
  ];
  return () => {
    for (const off of offs) off();
  };
}
