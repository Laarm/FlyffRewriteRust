// Dialog: a modal window (Plan/25 section 2.3 "every action reachable by keyboard"). Focus moves into the dialog when it opens,
// Tab stays inside, Escape closes it (unless `busy`), and focus returns to where it was. `role="dialog"` with `aria-modal`.
import {
  useEffect,
  useId,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react';
import { Icon } from './Icon';

export interface DialogProps {
  readonly title: string;
  readonly children: ReactNode;
  /** The buttons row. */
  readonly footer?: ReactNode;
  readonly onClose: () => void;
  readonly size?: 'sm' | 'md' | 'lg';
  /** Ignore Escape and the close button while something runs. */
  readonly busy?: boolean;
  /** A click on the dark area closes (information dialogs; not forms and confirmations). */
  readonly dismissOnOverlay?: boolean;
}

const FOCUSABLE =
  'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

export function Dialog({
  title,
  children,
  footer,
  onClose,
  size = 'md',
  busy = false,
  dismissOnOverlay = false,
}: DialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const node = ref.current;
    // the first field wins over the close button
    const first = node?.querySelector<HTMLElement>(
      '[data-autofocus],input:not([disabled]),select:not([disabled]),textarea:not([disabled])',
    );
    (first ?? node?.querySelector<HTMLElement>(FOCUSABLE) ?? node)?.focus();
    return () => {
      previous?.focus();
    };
  }, []);

  const onKeyDown = (event: ReactKeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      if (!busy) onClose();
      return;
    }
    if (event.key !== 'Tab') return;
    const nodes = Array.from(ref.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
    const firstNode = nodes[0];
    const lastNode = nodes[nodes.length - 1];
    if (!firstNode || !lastNode) return;
    if (event.shiftKey && document.activeElement === firstNode) {
      event.preventDefault();
      lastNode.focus();
    } else if (!event.shiftKey && document.activeElement === lastNode) {
      event.preventDefault();
      firstNode.focus();
    }
  };

  return (
    <div
      className="adm-overlay"
      onMouseDown={(e) => {
        if (dismissOnOverlay && !busy && e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        className={`adm-dialog adm-dialog--${size}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <header className="adm-dialog__head">
          <h2 id={titleId}>{title}</h2>
          <button
            type="button"
            className="adm-iconbtn"
            aria-label="Close"
            disabled={busy}
            onClick={onClose}
          >
            <Icon name="x" />
          </button>
        </header>
        <div className="adm-dialog__body">{children}</div>
        {footer ? <footer className="adm-dialog__foot">{footer}</footer> : null}
      </div>
    </div>
  );
}
