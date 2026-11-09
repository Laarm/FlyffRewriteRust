// The context menu of the map (Plan/25 section 3.11): a floating list of `MapAction`s at the pointer. It stays inside the window, closes on a click
// elsewhere, on Escape, on scroll and when the window loses focus; arrows move, Enter chooses. A disabled item says why.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { MapAction } from './InspectorActions';

export interface ContextMenuProps {
  /** Client coordinates of the pointer. */
  readonly x: number;
  readonly y: number;
  readonly title?: string;
  readonly actions: readonly MapAction[];
  readonly onClose: () => void;
  readonly label?: string;
}

export function ContextMenu({
  x,
  y,
  title,
  actions,
  onClose,
  label = 'Map actions',
}: ContextMenuProps) {
  const root = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ left: x, top: y });

  // inside the window: a menu near the right or lower edge opens up or to the left
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    setAt({
      left: Math.max(4, Math.min(x, window.innerWidth - w - 4)),
      top: Math.max(4, Math.min(y, window.innerHeight - h - 4)),
    });
  }, [x, y, actions]);

  useEffect(() => {
    root.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus();
    const onDown = (e: MouseEvent): void => {
      if (root.current && !root.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onClose();
        return;
      }
      if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
      e.preventDefault();
      const nodes = Array.from(
        root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? [],
      );
      if (nodes.length === 0) return;
      const i = nodes.indexOf(document.activeElement as HTMLElement);
      const next =
        e.key === 'ArrowDown' ? (i + 1) % nodes.length : (i - 1 + nodes.length) % nodes.length;
      nodes[next]?.focus();
    };
    document.addEventListener('mousedown', onDown, true);
    // on the window, like the map's own keys: `stopPropagation` of its handler does not reach a listener of the same target
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('blur', onClose);
    window.addEventListener('wheel', onClose, { passive: true });
    window.addEventListener('resize', onClose);
    return () => {
      document.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('blur', onClose);
      window.removeEventListener('wheel', onClose);
      window.removeEventListener('resize', onClose);
    };
  }, [onClose]);

  return (
    <div
      ref={root}
      className="ctx-menu"
      role="menu"
      aria-label={label}
      data-testid="map-context-menu"
      style={{ left: at.left, top: at.top }}
      onContextMenu={(e) => {
        e.preventDefault();
      }}
    >
      {title ? <div className="ctx-menu__title">{title}</div> : null}
      {actions.map((a) => (
        <div key={a.id} role="none">
          {a.separator === true ? <div className="ctx-menu__sep" role="separator" /> : null}
          <button
            type="button"
            role="menuitem"
            className={`ctx-menu__item${a.danger === true ? ' ctx-menu__item--danger' : ''}`}
            disabled={a.disabled !== undefined}
            {...(a.disabled === undefined ? {} : { title: a.disabled })}
            data-action={a.id}
            onClick={() => {
              onClose();
              void a.run();
            }}
          >
            {a.label}
          </button>
        </div>
      ))}
    </div>
  );
}
