// States of a panel (Plan/25 section 2.3): empty with a hint, error with retry, and "not available yet" for phase-gated panels
// (ADM-D10: the panel names the missing work package and never fakes data).
import type { ReactNode } from 'react';
import { AdminApiError } from '../api/rest';
import { Button } from './Button';
import { Icon, type IconName } from './Icon';

export interface EmptyStateProps {
  readonly title: string;
  readonly hint?: ReactNode;
  readonly icon?: IconName;
  readonly action?: ReactNode;
}

export function EmptyState({ title, hint, icon = 'info', action }: EmptyStateProps) {
  return (
    <div className="adm-state" role="status">
      <Icon name={icon} size={28} />
      <h3>{title}</h3>
      {hint ? <p className="adm-muted">{hint}</p> : null}
      {action}
    </div>
  );
}

export interface ErrorStateProps {
  readonly error: unknown;
  readonly onRetry?: () => void;
  readonly title?: string;
}

/** An error with its server code and a retry button. A `needs_feature` answer shows the NeedsFeature state instead. */
export function ErrorState({
  error,
  onRetry,
  title = 'Could not load this panel',
}: ErrorStateProps) {
  if (error instanceof AdminApiError && error.code === 'needs_feature') {
    return <NeedsFeature wp={error.details?.wp ?? 'a later work package'} />;
  }
  const code = error instanceof AdminApiError ? error.code : null;
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div className="adm-state adm-state--error" role="alert">
      <Icon name="alert" size={28} />
      <h3>{title}</h3>
      <p className="adm-muted">
        {message}
        {code ? (
          <>
            {' '}
            <code>{code}</code>
          </>
        ) : null}
      </p>
      {onRetry ? (
        <Button icon="refresh" onClick={onRetry}>
          Retry
        </Button>
      ) : null}
    </div>
  );
}

export interface NeedsFeatureProps {
  /** The work package that lands the backend or the view (`WP-ADMIN-17`). */
  readonly wp: string;
  readonly title?: string;
  readonly children?: ReactNode;
}

/** "Not available yet (needs WP-...)": the disabled state of a panel whose backend or view has not landed (ADM-D10). */
export function NeedsFeature({ wp, title = 'Not available yet', children }: NeedsFeatureProps) {
  return (
    <div className="adm-state adm-state--needs" role="status" data-needs-feature={wp}>
      <Icon name="clock" size={28} />
      <h3>{title}</h3>
      <p className="adm-muted">
        This panel needs <code>{wp}</code>, which has not landed in this build.
      </p>
      {children}
    </div>
  );
}
