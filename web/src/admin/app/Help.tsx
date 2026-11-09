// The "?" button of a category (Plan/25 section 15 WP-ADMIN-32 "in-app ? links to the guide's anchors"): a dialog with what the
// category is for, which tab needs which capability, and where the operator guide covers it (`docs/admin-console.md`, section anchor).
import { lazy, Suspense } from 'react';
import { CAP_GRANTS } from '../gen/caps';
import { Button } from '../components/Button';
import { Dialog } from '../components/Dialog';
import { openDialog } from '../stores/dialogs';
import { useCaps } from '../stores/session';
import type { CategoryDef } from './nav';
import { S } from './strings';

/** The guide is a lazy chunk (the markdown and its screenshots): the entry does not carry it. */
const GuideDialog = lazy(() => import('./Guide'));

/** Opens the operator guide at an anchor (`players`, `remote-access`). */
export function openGuide(anchor: string): void {
  openDialog((close) => (
    <Suspense fallback={null}>
      <GuideDialog anchor={anchor} onClose={close} />
    </Suspense>
  ));
}

/** The file the "?" dialogs point to, relative to the repository (it is also copied into the release folder as `admin-console.md`). */
export const GUIDE_FILE = 'docs/admin-console.md';

export function guideRef(category: CategoryDef): string {
  return `${GUIDE_FILE}#${category.guide}`;
}

export function HelpDialog({
  category,
  onClose,
}: {
  readonly category: CategoryDef;
  readonly onClose: () => void;
}) {
  const caps = useCaps();
  return (
    <Dialog title={category.label} onClose={onClose} dismissOnOverlay>
      <p>{category.summary}.</p>
      <table className="adm-plain">
        <thead>
          <tr>
            <th>Tab</th>
            <th>Needs</th>
            <th>Your account</th>
          </tr>
        </thead>
        <tbody>
          {category.tabs.map((t) => (
            <tr key={t.id}>
              <td>{t.label}</td>
              <td>
                <code>{t.cap}</code>
                <div className="adm-muted adm-small">{CAP_GRANTS[t.cap]}</div>
              </td>
              <td>{caps.has(t.cap) ? 'yes' : 'no'}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="adm-row" style={{ marginTop: 12 }}>
        <Button
          onClick={() => {
            onClose();
            openGuide(category.guide);
          }}
        >
          Read this part of the guide
        </Button>
        <span className="adm-muted adm-small">
          <code>{guideRef(category)}</code>
        </span>
      </div>
    </Dialog>
  );
}

export function HelpButton({ category }: { readonly category: CategoryDef }) {
  return (
    <Button
      variant="ghost"
      size="sm"
      icon="help"
      aria-label={S.view.help}
      title={S.view.help}
      onClick={() => {
        openDialog((close) => <HelpDialog category={category} onClose={close} />);
      }}
    />
  );
}
