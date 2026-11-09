// The left navigation (Plan/25 section 2.1): the categories of section 2.2 in order, only those the session may open. Real anchors
// (`#/players`) so that middle click and "copy link" work; a click opens the category's last remembered tab. Below 1280 px, and always on
// the Map tab, it is a 44 px icon rail (the label stays as `aria-label` and tooltip).
import { Icon } from '../components/Icon';
import { navigate } from '../stores/route';
import { useUi } from '../stores/ui';
import { visibleCategories, type CapSet } from './nav';
import { formatHash, type Route } from './route';
import { S } from './strings';

export function Nav({
  caps,
  current,
  rail,
}: {
  readonly caps: CapSet;
  readonly current: string | null;
  readonly rail: boolean;
}) {
  const categories = visibleCategories(caps);
  const lastTabs = useUi((s) => s.lastTabs);
  const navCollapsed = useUi((s) => s.navCollapsed);
  const setNavCollapsed = useUi((s) => s.setNavCollapsed);
  return (
    <nav className="adm-nav" aria-label={S.nav.label}>
      {categories.length === 0 ? <p className="adm-muted adm-nav__none">{S.nav.none}</p> : null}
      <ul>
        {categories.map((c) => {
          const route: Route = { category: c.id, tab: lastTabs[c.id] ?? null, params: {} };
          const active = c.id === current;
          return (
            <li key={c.id}>
              <a
                href={formatHash({ ...route, tab: null })}
                className={`adm-nav__item ${active ? 'is-active' : ''}`}
                aria-current={active ? 'page' : undefined}
                aria-label={rail ? c.label : undefined}
                title={rail ? `${c.label}: ${c.summary}` : c.summary}
                onClick={(e) => {
                  if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
                  e.preventDefault();
                  navigate(route);
                }}
              >
                <Icon name={c.icon} size={18} />
                <span className="adm-nav__label">{c.label}</span>
              </a>
            </li>
          );
        })}
      </ul>
      <button
        type="button"
        className="adm-nav__toggle"
        onClick={() => {
          setNavCollapsed(!navCollapsed);
        }}
        aria-label={navCollapsed ? S.nav.expand : S.nav.collapse}
        title={navCollapsed ? S.nav.expand : S.nav.collapse}
      >
        <Icon name={rail && navCollapsed ? 'chevronRight' : 'chevronLeft'} size={16} />
      </button>
    </nav>
  );
}
