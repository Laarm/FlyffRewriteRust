// Menu: a button that opens a list of actions (the identity menu, row action menus). Arrow keys move, Enter chooses, Escape and a
// click outside close. Items are buttons with `role="menuitem"`; a `radio` item shows a check for the chosen option.
import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { Icon, type IconName } from './Icon';

export interface MenuItem {
  readonly id: string;
  readonly label: ReactNode;
  readonly icon?: IconName;
  readonly onSelect?: () => void;
  readonly disabled?: boolean;
  /** Shown under the label (why an item is disabled). */
  readonly hint?: string;
  /** A checkmark for the current choice of a group. */
  readonly checked?: boolean;
  /** A thin line above the item. */
  readonly separator?: boolean;
  /** Not clickable: a small heading or a line of information. */
  readonly heading?: boolean;
}

export interface MenuProps {
  /** The button's content. */
  readonly trigger: ReactNode;
  readonly label: string;
  readonly items: readonly MenuItem[];
  readonly align?: 'start' | 'end';
  readonly className?: string;
}

export function Menu({ trigger, label, items, align = 'end', className }: MenuProps) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const id = useId();

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent): void => {
      if (wrap.current && !wrap.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => {
      document.removeEventListener('mousedown', onDown);
    };
  }, [open]);

  useEffect(() => {
    if (open)
      wrap.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus();
  }, [open]);

  const onKeyDown = (e: ReactKeyboardEvent): void => {
    if (e.key === 'Escape' && open) {
      e.stopPropagation();
      setOpen(false);
      wrap.current?.querySelector<HTMLElement>('button[aria-haspopup]')?.focus();
      return;
    }
    if (!open || (e.key !== 'ArrowDown' && e.key !== 'ArrowUp')) return;
    e.preventDefault();
    const nodes = Array.from(
      wrap.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? [],
    );
    const i = nodes.indexOf(document.activeElement as HTMLElement);
    const next =
      e.key === 'ArrowDown'
        ? nodes[(i + 1) % nodes.length]
        : nodes[(i - 1 + nodes.length) % nodes.length];
    next?.focus();
  };

  return (
    <div className={`adm-menu ${className ?? ''}`} ref={wrap} onKeyDown={onKeyDown}>
      <button
        type="button"
        className="adm-menu__trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={label}
        onClick={() => {
          setOpen((o) => !o);
        }}
      >
        {trigger}
      </button>
      {open ? (
        <div
          id={id}
          role="menu"
          aria-label={label}
          className={`adm-menu__list adm-menu__list--${align}`}
        >
          {items.map((it) =>
            it.heading === true ? (
              <div key={it.id} className="adm-menu__heading" role="presentation">
                {it.label}
              </div>
            ) : (
              <button
                key={it.id}
                type="button"
                role="menuitem"
                className={`adm-menu__item ${it.separator === true ? 'has-separator' : ''}`}
                disabled={it.disabled === true}
                onClick={() => {
                  setOpen(false);
                  it.onSelect?.();
                }}
              >
                <span className="adm-menu__icon">{it.icon ? <Icon name={it.icon} /> : null}</span>
                <span className="adm-grow">
                  {it.label}
                  {it.hint ? <span className="adm-menu__hint">{it.hint}</span> : null}
                </span>
                {it.checked === true ? <Icon name="check" /> : null}
              </button>
            ),
          )}
        </div>
      ) : null}
    </div>
  );
}
