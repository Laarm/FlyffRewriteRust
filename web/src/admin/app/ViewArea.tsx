// The content area (Plan/25 section 2.1): the category's header with its tabs, and the view of the selected tab. The view comes
// from the category's lazily loaded module (app/views.ts); a tab the module does not list shows NeedsFeature with its work package, a
// tab the session may not open shows "No access", a failed chunk load shows the error with a retry and the rescue page.
import type { ReactNode } from 'react';
import { EmptyState, ErrorState, NeedsFeature } from '../components/EmptyState';
import { Skeleton } from '../components/Spinner';
import { Tabs, tabButtonId, tabPanelId, type TabItem } from '../components/Tabs';
import { navigate } from '../stores/route';
import { ErrorBoundary } from './ErrorBoundary';
import { HelpButton } from './Help';
import { visibleTabs, type CapSet, type CategoryDef, type TabDef } from './nav';
import { withoutRecord, type Route } from './route';
import { S } from './strings';
import { useCategory } from './useCategory';

export interface ViewAreaProps {
  readonly category: CategoryDef;
  readonly tab: TabDef | null;
  readonly route: Route;
  readonly caps: CapSet;
  /** The session may not open the requested tab (or any tab of the category). */
  readonly forbidden: boolean;
}

export function ViewArea({ category, tab, route, caps, forbidden }: ViewAreaProps) {
  const tabs = visibleTabs(category, caps);
  const items: TabItem[] = tabs.map((t) => ({ id: t.id, label: t.label }));
  const prefix = `view-${category.id}`;
  const selected = tab?.id ?? tabs[0]?.id ?? '';
  const { state, retry } = useCategory(forbidden ? null : category.id);

  const body = ((): ReactNode => {
    if (forbidden || tab === null) {
      return (
        <EmptyState
          icon="lock"
          title={S.view.forbiddenTitle}
          hint={tab ? S.view.forbiddenHint(tab.cap) : S.view.forbiddenCategory}
        />
      );
    }
    if (state.status === 'loading') return <Skeleton lines={5} height={18} />;
    if (state.status === 'error') {
      return <ErrorState title={S.view.loadFailed} error={state.error} onRetry={retry} />;
    }
    const View = state.module.tabs[tab.id];
    if (!View) return <NeedsFeature wp={tab.wp} title={S.view.notBuilt(tab.label)} />;
    return <View route={route} tab={tab} />;
  })();

  return (
    <section className="adm-view" aria-label={category.label}>
      <header className="adm-view__head">
        <h1>{category.label}</h1>
        <div className="adm-tabsrow">
          {items.length > 0 ? (
            <Tabs
              tabs={items}
              value={selected}
              idPrefix={prefix}
              label={`${category.label} tabs`}
              onChange={(id) => {
                navigate({ category: category.id, tab: id, params: withoutRecord(route.params) });
              }}
            />
          ) : null}
          <HelpButton category={category} />
        </div>
      </header>
      <div
        className="adm-view__body"
        role="tabpanel"
        id={tabPanelId(prefix)}
        aria-labelledby={tab ? tabButtonId(prefix, tab.id) : undefined}
        tabIndex={-1}
      >
        <ErrorBoundary inline resetKey={`${route.category}/${route.tab ?? ''}`}>
          {body}
        </ErrorBoundary>
      </div>
    </section>
  );
}
