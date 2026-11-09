// Tabs (the second navigation level, Plan/25 section 2.1): ARIA tablist with roving tabindex; Left/Right/Home/End move and
// activate. The caller renders the panel with `role="tabpanel"`, `id={tabPanelId(prefix)}` and `aria-labelledby={tabButtonId(prefix, value)}`.
import { useRef, type KeyboardEvent, type ReactNode } from 'react';

export interface TabItem {
  readonly id: string;
  readonly label: string;
  /** A count or short status shown after the label. */
  readonly badge?: ReactNode;
  readonly disabled?: boolean;
}

export interface TabsProps {
  readonly tabs: readonly TabItem[];
  readonly value: string;
  readonly onChange: (id: string) => void;
  /** Unique per tab bar on the page; names the ids of buttons and panel. */
  readonly idPrefix: string;
  readonly label: string;
}

export const tabButtonId = (prefix: string, id: string): string => `${prefix}-tab-${id}`;
export const tabPanelId = (prefix: string): string => `${prefix}-panel`;

export function Tabs({ tabs, value, onChange, idPrefix, label }: TabsProps) {
  const refs = useRef(new Map<string, HTMLButtonElement>());
  const enabled = tabs.filter((t) => t.disabled !== true);

  const move = (event: KeyboardEvent, from: string): void => {
    const i = enabled.findIndex((t) => t.id === from);
    let next: TabItem | undefined;
    if (event.key === 'ArrowRight') next = enabled[(i + 1) % enabled.length];
    else if (event.key === 'ArrowLeft') next = enabled[(i - 1 + enabled.length) % enabled.length];
    else if (event.key === 'Home') next = enabled[0];
    else if (event.key === 'End') next = enabled[enabled.length - 1];
    if (!next) return;
    event.preventDefault();
    onChange(next.id);
    refs.current.get(next.id)?.focus();
  };

  return (
    <div className="adm-tabs" role="tablist" aria-label={label}>
      {tabs.map((t) => {
        const selected = t.id === value;
        return (
          <button
            key={t.id}
            ref={(el) => {
              if (el) refs.current.set(t.id, el);
              else refs.current.delete(t.id);
            }}
            type="button"
            role="tab"
            id={tabButtonId(idPrefix, t.id)}
            aria-selected={selected}
            aria-controls={tabPanelId(idPrefix)}
            tabIndex={selected ? 0 : -1}
            disabled={t.disabled === true}
            className={`adm-tab ${selected ? 'is-selected' : ''}`}
            onClick={() => {
              onChange(t.id);
            }}
            onKeyDown={(e) => {
              move(e, t.id);
            }}
          >
            {t.label}
            {t.badge !== undefined && <span className="adm-tab__badge">{t.badge}</span>}
          </button>
        );
      })}
    </div>
  );
}
