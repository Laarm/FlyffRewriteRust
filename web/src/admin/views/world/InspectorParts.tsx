// The pieces the cards of the inspector share: a gauge for HP, MP and FP, a list of facts, and the action buttons grouped as the plan lists them.
import type { ReactNode } from 'react';
import { Button } from '../../components';
import type { MapAction } from './InspectorActions';

export interface BarProps {
  readonly label: string;
  readonly value: number;
  readonly max: number;
  readonly tone: 'hp' | 'mp' | 'fp';
}

/** A gauge: `value / max` with the numbers beside it (the game's red, blue and green). */
export function Bar({ label, value, max, tone }: BarProps) {
  const pct = max <= 0 ? 0 : Math.max(0, Math.min(100, Math.round((value / max) * 100)));
  return (
    <div className="insp-bar" data-tone={tone}>
      <span className="insp-bar__label">{label}</span>
      <span
        className="insp-bar__track"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={Math.max(1, max)}
        aria-valuenow={Math.max(0, Math.min(max, value))}
      >
        <span className="insp-bar__fill" style={{ width: `${String(pct)}%` }} />
      </span>
      <span className="insp-bar__text">{`${value.toLocaleString('en-US')} / ${max.toLocaleString('en-US')}`}</span>
    </div>
  );
}

export interface Fact {
  readonly label: string;
  readonly value: ReactNode;
  /** A test id for the value. */
  readonly id?: string;
}

/** `label: value` rows; a row without a value is left out. */
export function Facts({ rows }: { readonly rows: readonly (Fact | null | false)[] }) {
  return (
    <dl className="insp-facts">
      {rows.map((r) =>
        r === null || r === false ? null : (
          <div key={r.label} className="insp-facts__row">
            <dt>{r.label}</dt>
            <dd data-testid={r.id}>{r.value}</dd>
          </div>
        ),
      )}
    </dl>
  );
}

/** The actions as buttons; a thin gap sets off each group (`separator`), a disabled one says why in its title. */
export function ActionButtons({ actions }: { readonly actions: readonly MapAction[] }) {
  if (actions.length === 0) return null;
  const groups: MapAction[][] = [];
  for (const a of actions) {
    if (a.separator === true || groups.length === 0) groups.push([a]);
    else groups[groups.length - 1]?.push(a);
  }
  return (
    <div className="insp-actions" role="group" aria-label="Actions">
      {groups.map((g) => (
        <div key={g[0]?.id} className="insp-actions__group">
          {g.map((a) => (
            <Button
              key={a.id}
              size="sm"
              variant={a.danger === true ? 'danger' : 'secondary'}
              disabled={a.disabled !== undefined}
              {...(a.disabled === undefined ? {} : { title: a.disabled })}
              data-action={a.id}
              onClick={() => {
                void a.run();
              }}
            >
              {a.label}
            </Button>
          ))}
        </div>
      ))}
    </div>
  );
}

/** The name of the card with a line of context under it. */
export function CardHead({
  icon,
  title,
  subtitle,
  badge,
}: {
  readonly icon?: ReactNode;
  readonly title: string;
  readonly subtitle?: ReactNode;
  readonly badge?: ReactNode;
}) {
  return (
    <header className="insp-head">
      {icon ? <span className="insp-head__icon">{icon}</span> : null}
      <div className="insp-head__text">
        <h3 className="insp-head__title" data-testid="insp-name">
          {title}
        </h3>
        {subtitle ? <div className="insp-head__sub">{subtitle}</div> : null}
      </div>
      {badge ? <span className="insp-head__badge">{badge}</span> : null}
    </header>
  );
}
