// Drawer (Plan/25 section 2.1): record details open in a right-hand panel over any view, so the admin never loses the list or the
// map they came from. 520 px wide, 720 px for the Items tab of a character; the pin button turns it into a full page; "Copy link" copies
// the deep link. It is not modal: the page behind stays usable. Escape closes it while focus is inside.
import { useEffect, useRef, type KeyboardEvent, type ReactNode } from 'react';
import { Button } from './Button';
import { Icon } from './Icon';

export interface DrawerProps {
  readonly title: string;
  readonly subtitle?: ReactNode;
  readonly children: ReactNode;
  readonly onClose: () => void;
  /** 720 px instead of 520 px. */
  readonly wide?: boolean;
  /** Shown as a full page instead of a panel. */
  readonly pinned?: boolean;
  readonly onTogglePin?: () => void;
  readonly onCopyLink?: () => void;
  /** Extra header buttons (an "open in a new tab" link, an actions menu). */
  readonly actions?: ReactNode;
}

export function Drawer({
  title,
  subtitle,
  children,
  onClose,
  wide = false,
  pinned = false,
  onTogglePin,
  onCopyLink,
  actions,
}: DrawerProps) {
  const ref = useRef<HTMLElement>(null);

  // focus enters the drawer when it opens so that the keyboard follows the click that opened it
  useEffect(() => {
    ref.current?.focus();
  }, []);

  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === 'Escape') {
      event.stopPropagation();
      onClose();
    }
  };

  return (
    <aside
      ref={ref}
      className={`adm-drawer ${wide ? 'adm-drawer--wide' : ''} ${pinned ? 'adm-drawer--pinned' : ''}`}
      aria-label={title}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      data-testid="drawer"
    >
      <header className="adm-drawer__head">
        <div className="adm-grow">
          <h2 className="adm-ellipsis">{title}</h2>
          {subtitle ? <div className="adm-muted adm-ellipsis">{subtitle}</div> : null}
        </div>
        {actions}
        {onCopyLink ? (
          <Button
            variant="ghost"
            size="sm"
            icon="link"
            onClick={onCopyLink}
            aria-label="Copy link"
            title="Copy link"
          />
        ) : null}
        {onTogglePin ? (
          <Button
            variant="ghost"
            size="sm"
            icon={pinned ? 'x' : 'pin'}
            onClick={onTogglePin}
            aria-pressed={pinned}
            aria-label={pinned ? 'Unpin: show as a panel' : 'Pin: show as a full page'}
            title={pinned ? 'Show as a panel' : 'Show as a full page'}
          />
        ) : null}
        <button type="button" className="adm-iconbtn" aria-label="Close details" onClick={onClose}>
          <Icon name="x" />
        </button>
      </header>
      <div className="adm-drawer__body">{children}</div>
    </aside>
  );
}
