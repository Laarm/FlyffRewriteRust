// The record drawer (Plan/25 section 2.1): the record named by the route (`?c=<id>` character, `?a=` account, `?g=` guild, `?i=` item,
// `?m=` object) opens in a right-hand panel over any view; pinned (`pin=1`) it fills the content area. The body is the drawer component of
// the category that owns the record kind (app/views.ts `RECORD_OWNER`); without one the drawer says what is missing. Closing removes the
// record from the URL, so the back button and a shared link behave.
import { useCallback, useMemo, useState, type ReactNode } from 'react';
import { Drawer } from '../components/Drawer';
import { ErrorState, NeedsFeature } from '../components/EmptyState';
import { Skeleton } from '../components/Spinner';
import { navigate } from '../stores/route';
import { toast } from '../stores/toasts';
import { copyText, linkTo } from './clipboard';
import { ErrorBoundary } from './ErrorBoundary';
import {
  formatHash,
  isPinned,
  recordOf,
  withPinned,
  withRecord,
  type RecordKind,
  type RecordRef,
  type Route,
} from './route';
import { S } from './strings';
import { useCategory } from './useCategory';
import { RECORD_OWNER, type DrawerControls } from './views';

/** The work package that builds the drawer body of each kind (shown while the category has none). */
const DRAWER_WP: Readonly<Record<RecordKind, string>> = {
  character: 'WP-ADMIN-18',
  account: 'WP-ADMIN-18',
  guild: 'WP-ADMIN-24',
  item: 'WP-ADMIN-18',
  monster: 'WP-ADMIN-13',
};

function RecordDrawer({ record, route }: { readonly record: RecordRef; readonly route: Route }) {
  const { state, retry } = useCategory(RECORD_OWNER[record.kind]);
  const [wide, setWide] = useState(false);
  const [header, setHeaderState] = useState<{ title: string; subtitle?: ReactNode }>({
    title: `${S.drawer.title[record.kind]} ${record.id}`,
  });
  const pinned = isPinned(route);

  const setHeader = useCallback((title: string, subtitle?: ReactNode) => {
    setHeaderState(subtitle === undefined ? { title } : { title, subtitle });
  }, []);

  const controls: DrawerControls = useMemo(
    () => ({
      close: () => {
        navigate(withRecord(route, null));
      },
      setWide,
      setHeader,
      copyLink: () => {
        if (copyText(linkTo(formatHash(route)))) toast.success(S.drawer.copied);
        else toast.info(S.drawer.copyFailed);
      },
    }),
    [route, setHeader],
  );

  const body = ((): ReactNode => {
    if (state.status === 'loading') return <Skeleton lines={6} height={18} />;
    if (state.status === 'error')
      return <ErrorState error={state.error} onRetry={retry} title={S.view.loadFailed} />;
    const Body = state.module.drawers?.[record.kind];
    if (!Body) return <NeedsFeature wp={DRAWER_WP[record.kind]} title={S.drawer.notBuilt} />;
    return <Body record={record} controls={controls} />;
  })();

  return (
    <Drawer
      title={header.title}
      {...(header.subtitle === undefined ? {} : { subtitle: header.subtitle })}
      wide={wide}
      pinned={pinned}
      onClose={controls.close}
      onCopyLink={controls.copyLink}
      onTogglePin={() => {
        navigate(withPinned(route, !pinned), { replace: true });
      }}
    >
      <ErrorBoundary inline resetKey={`${record.kind}:${record.id}`}>
        {body}
      </ErrorBoundary>
    </Drawer>
  );
}

/** The drawer of the route's record, or nothing. */
export function DrawerHost({ route }: { readonly route: Route }) {
  const record = recordOf(route);
  if (!record) return null;
  return <RecordDrawer key={`${record.kind}:${record.id}`} record={record} route={route} />;
}
