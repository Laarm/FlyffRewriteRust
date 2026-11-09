// The shell (Plan/25 section 2.1): top bar, left navigation, the content area with the tabs of the category, the record drawer and the
// command palette. It resolves the hash route against the session's capabilities: no route opens the landing view, a route without a tab
// opens the remembered one, and a tab the account may not open shows "No access" instead of another tab.
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { EmptyState } from '../components/EmptyState';
import { Button } from '../components/Button';
import { navigate, useRouteStore } from '../stores/route';
import { useCaps, useSession } from '../stores/session';
import { useUi } from '../stores/ui';
import { DrawerHost } from './DrawerHost';
import { landing, resolveLocation } from './nav';
import { Nav } from './NavView';
import { Palette } from './PaletteDialog';
import { isPinned, recordOf, type Route } from './route';
import { S } from './strings';
import { TopBar } from './TopBar';
import { usePolling } from './polling';
import { useMediaQuery } from './useMediaQuery';
import { ViewArea } from './ViewArea';

/** `true` when the key event is a plain character typed into something editable (single-key shortcuts stay off there). */
function typingInto(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) ||
    target.closest('[role="dialog"]') !== null
  );
}

export function Shell() {
  const info = useSession((s) => s.info);
  const caps = useCaps();
  const route = useRouteStore((s) => s.route);
  const lastTabs = useUi((s) => s.lastTabs);
  const landingPref = useUi((s) => s.landing);
  const navCollapsed = useUi((s) => s.navCollapsed);
  const rememberTab = useUi((s) => s.rememberTab);
  const narrow = useMediaQuery('(max-width: 1279px)');
  const refreshSession = useSession((s) => s.refresh);
  // who the server thinks we are (a cookie that expired on the host leaves the owner of the host): every minute while the page is in use
  usePolling(refreshSession, 60_000);
  const [paletteOpen, setPaletteOpen] = useState(false);

  const resolved = useMemo(
    () => (route ? resolveLocation(route.category, route.tab, caps, lastTabs) : null),
    [route, caps, lastTabs],
  );

  // no route: the landing view; a route without a tab: the remembered or first permitted tab (replace, so Back leaves the console's own redirect)
  useEffect(() => {
    if (route === null || route.category === '') {
      const l = landing(caps, landingPref);
      if (l) navigate({ category: l.category.id, tab: l.tab.id, params: {} }, { replace: true });
      return;
    }
    if (resolved?.kind === 'tab') {
      if (route.tab === null) navigate({ ...route, tab: resolved.tab.id }, { replace: true });
      rememberTab(resolved.category.id, resolved.tab.id);
    }
  }, [route, resolved, caps, landingPref, rememberTab]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if (e.key === '/' && !e.ctrlKey && !e.metaKey && !e.altKey && !typingInto(e.target)) {
        e.preventDefault();
        setPaletteOpen(true);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, []);

  const onMap =
    resolved?.kind === 'tab' && resolved.category.id === 'world' && resolved.tab.id === 'map';
  const rail = narrow || navCollapsed || onMap;
  const pinned = route !== null && recordOf(route) !== null && isPinned(route);
  const current = route && resolved && resolved.kind !== 'unknown' ? resolved.category.id : null;
  const insecureRemote = info !== null && info.remote && !info.tls;

  const main = ((): ReactNode => {
    if (route === null || route.category === '') return null;
    if (pinned) return <DrawerHost route={route} />;
    if (!resolved || resolved.kind === 'unknown') {
      return (
        <EmptyState
          icon="alert"
          title={S.view.unknownTitle}
          hint={S.view.unknownHint}
          action={
            <Button
              variant="primary"
              onClick={() => {
                navigate(null);
              }}
            >
              {S.view.goHome}
            </Button>
          }
        />
      );
    }
    const r: Route = route;
    return (
      <ViewArea
        category={resolved.category}
        tab={resolved.tab}
        route={r}
        caps={caps}
        forbidden={resolved.kind === 'forbidden'}
      />
    );
  })();

  return (
    <div className="adm-shell" data-rail={rail ? 'true' : 'false'} data-testid="shell">
      {insecureRemote ? (
        <div className="adm-banner adm-banner--danger" role="alert">
          {S.banner.plainHttp}
        </div>
      ) : null}
      <TopBar
        onOpenPalette={() => {
          setPaletteOpen(true);
        }}
      />
      <Nav caps={caps} current={current} rail={rail} />
      <main className="adm-main" id="adm-main">
        {main}
      </main>
      {route !== null && !pinned ? <DrawerHost route={route} /> : null}
      {paletteOpen ? (
        <Palette
          onClose={() => {
            setPaletteOpen(false);
          }}
        />
      ) : null}
    </div>
  );
}
