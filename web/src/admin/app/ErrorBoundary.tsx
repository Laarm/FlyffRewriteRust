// Error boundaries (Plan/25 section 9.10): a render error in the console shows the way to the rescue page `/admin/lite`, which
// has no part of the console's code and keeps working. `ViewBoundary` guards one view so the shell around it stays usable.
import { Component, type ErrorInfo, type ReactNode } from 'react';
import { log } from '../../platform/log';
import { S } from './strings';

interface Props {
  readonly children: ReactNode;
  /** The boundary resets when this changes (a route change leaves a broken view). */
  readonly resetKey?: string;
  /** Compact: inside the shell, instead of a full page. */
  readonly inline?: boolean;
}

interface State {
  readonly error: Error | null;
  readonly key: string | undefined;
}

export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null, key: this.props.resetKey };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey !== state.key ? { error: null, key: props.resetKey } : null;
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    log.error('admin', `render error: ${error.message}${info.componentStack ?? ''}`);
  }

  override render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div
        className={this.props.inline === true ? 'adm-state adm-state--error' : 'adm-fatal'}
        role="alert"
      >
        <h2>{S.rescue.renderFailed}</h2>
        <p className="adm-muted">{error.message}</p>
        <p className="adm-muted">{S.rescue.hint}</p>
        <div className="adm-row" style={{ justifyContent: 'center' }}>
          <a className="adm-btn adm-btn--primary" href="/admin/lite">
            <span className="adm-btn__label">{S.rescue.link}</span>
          </a>
          <button
            type="button"
            className="adm-btn adm-btn--secondary"
            onClick={() => {
              location.reload();
            }}
          >
            <span className="adm-btn__label">{S.rescue.reload}</span>
          </button>
        </div>
      </div>
    );
  }
}
