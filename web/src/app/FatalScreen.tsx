// L4 fatal screen skeleton (Plan/40 section 12.2): needs no game assets. Shows the reason, the build
// id and the last log lines as selectable text (the clipboard API is not used, Plan/02 11.7).

export interface FatalScreenProps {
  readonly reason: string;
  readonly buildId: string;
  readonly logLines: readonly string[];
  readonly onReload: () => void;
  readonly onBackToLogin: () => void;
}

export function FatalScreen({
  reason,
  buildId,
  logLines,
  onReload,
  onBackToLogin,
}: FatalScreenProps) {
  return (
    <main className="fatal" role="alert" data-testid="fatal">
      <h1>Something went wrong</h1>
      <p data-testid="fatal-reason">{reason}</p>
      <p>build {buildId}</p>
      <pre data-testid="fatal-log">{logLines.join('\n')}</pre>
      <p>
        <button type="button" onClick={onReload}>
          Reload
        </button>{' '}
        <button type="button" onClick={onBackToLogin}>
          Back to login
        </button>
      </p>
    </main>
  );
}
