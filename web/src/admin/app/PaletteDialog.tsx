// The command palette (Plan/25 section 2.3, Ctrl+K / Cmd+K): fuzzy search over the tabs and actions the session may use and, through
// `GET /admin/api/search`, over characters, accounts, guilds, items and places. Enter opens the record in the drawer or runs the
// action. The logic (matching, ranking, providers) is app/palette.ts; this is the window.
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import { Icon } from '../components/Icon';
import { copyText, linkTo } from './clipboard';
import { openDialog } from '../stores/dialogs';
import { getRoute, navigate } from '../stores/route';
import { useCaps, useSession } from '../stores/session';
import { toast } from '../stores/toasts';
import { useUi } from '../stores/ui';
import { visibleCategories, visibleTabs } from './nav';
import {
  degradeToStub,
  groupItems,
  KIND_LABEL,
  rankItems,
  restSearchProvider,
  type PaletteItem,
  type SearchHit,
  type SearchProvider,
} from './palette';
import { PasswordDialog } from './PasswordChange';
import { withRecord, type RecordKind } from './route';
import { S } from './strings';
import { loadedCategories } from './views';

/** The provider the page uses: the REST search, degraded to the stub for good once the route answers `needs_feature`. */
let sharedProvider: SearchProvider = degradeToStub(restSearchProvider);

/** Tests: replaces the provider (`null` restores the real one). */
export function setSearchProviderForTests(p: SearchProvider | null): void {
  sharedProvider = p ?? degradeToStub(restSearchProvider);
}

const HIT_RECORD: Readonly<Record<'character' | 'account' | 'guild' | 'item', RecordKind>> = {
  character: 'character',
  account: 'account',
  guild: 'guild',
  item: 'item',
};

/** The palette rows a search hit becomes; choosing one opens its record or centres the map. */
export function hitToItem(hit: SearchHit): PaletteItem {
  const base = {
    id: `${hit.kind}:${hit.id}`,
    kind: hit.kind,
    title: hit.title,
    ...(hit.subtitle === undefined ? {} : { subtitle: hit.subtitle }),
    ...(hit.online === true ? { online: true } : {}),
  } as const;
  if (hit.kind === 'location') {
    return {
      ...base,
      icon: 'map',
      run: () => {
        navigate({
          category: 'world',
          tab: 'map',
          params: {
            ...(hit.world === undefined ? {} : { w: String(hit.world) }),
            ...(hit.x === undefined ? {} : { x: String(hit.x) }),
            ...(hit.z === undefined ? {} : { z: String(hit.z) }),
          },
        });
      },
    };
  }
  const kind = HIT_RECORD[hit.kind];
  return {
    ...base,
    icon: hit.kind === 'item' ? 'package' : hit.kind === 'guild' ? 'shield' : 'user',
    run: () => {
      const route = getRoute() ?? { category: 'dashboard', tab: null, params: {} };
      navigate(withRecord(route, { kind, id: hit.id }));
    },
  };
}

/** Tabs the session may open, the built-in actions, and the actions of the categories loaded so far. */
export function useLocalItems(): readonly PaletteItem[] {
  const caps = useCaps();
  const info = useSession((s) => s.info);
  const logout = useSession((s) => s.logout);
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  return useMemo(() => {
    const items: PaletteItem[] = [];
    for (const c of visibleCategories(caps)) {
      for (const t of visibleTabs(c, caps)) {
        items.push({
          id: `tab:${c.id}/${t.id}`,
          kind: 'tab',
          title: `${c.label}: ${t.label}`,
          keywords: `${c.id} ${t.id}`,
          icon: c.icon,
          run: () => {
            navigate({ category: c.id, tab: t.id, params: {} });
          },
        });
      }
    }
    items.push(
      {
        id: 'action:theme',
        kind: 'action',
        title: S.actions.toggleTheme,
        keywords: 'theme dark light mode appearance',
        icon: 'moon',
        run: () => {
          const dark =
            theme === 'dark' ||
            (theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
          setTheme(dark ? 'light' : 'dark');
        },
      },
      {
        id: 'action:copy-link',
        kind: 'action',
        title: S.actions.copyLink,
        keywords: 'url share deep link',
        icon: 'link',
        run: () => {
          if (copyText(linkTo(location.hash))) toast.success(S.drawer.copied);
          else toast.info(S.drawer.copyFailed);
        },
      },
      {
        id: 'action:rescue',
        kind: 'action',
        title: S.actions.openRescue,
        keywords: 'lite fallback emergency',
        icon: 'external',
        run: () => {
          window.open('/admin/lite', '_blank', 'noopener');
        },
      },
    );
    if (info?.kind === 'staff') {
      items.push({
        id: 'action:password',
        kind: 'action',
        title: S.actions.changePassword,
        keywords: 'credentials',
        icon: 'key',
        run: () => {
          openDialog((close) => <PasswordDialog onClose={close} />);
        },
      });
    }
    if (info?.idle_expires_at != null) {
      items.push({
        id: 'action:signout',
        kind: 'action',
        title: S.actions.signOut,
        keywords: 'logout',
        icon: 'logout',
        run: () => {
          void logout();
        },
      });
    }
    for (const m of loadedCategories()) {
      for (const a of m.actions ?? []) {
        if (a.cap !== undefined && !caps.has(a.cap)) continue;
        items.push({
          id: `action:${a.id}`,
          kind: 'action',
          title: a.label,
          ...(a.keywords ? { keywords: a.keywords.join(' ') } : {}),
          ...(a.icon ? { icon: a.icon } : {}),
          run: () => {
            a.run();
          },
        });
      }
    }
    return items;
  }, [caps, info, logout, theme, setTheme]);
}

const DEBOUNCE_MS = 150;
const NO_HITS: readonly SearchHit[] = [];

/** The last answer of the search provider and the query it belongs to. */
interface Remote {
  readonly query: string;
  readonly hits: readonly SearchHit[];
  readonly unavailableWp: string | null;
  readonly failed: boolean;
}

export function Palette({ onClose }: { readonly onClose: () => void }) {
  const local = useLocalItems();
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [remote, setRemote] = useState<Remote>({
    query: '',
    hits: [],
    unavailableWp: null,
    failed: false,
  });
  const listId = useId();
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const q = query.trim();
  const searchable = q.length >= 2;
  useEffect(() => {
    if (!searchable) return;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      sharedProvider.search(q, controller.signal).then(
        (res) => {
          if (!controller.signal.aborted) {
            setRemote({
              query: q,
              hits: res.hits,
              unavailableWp: res.unavailableWp ?? null,
              failed: false,
            });
          }
        },
        () => {
          if (!controller.signal.aborted)
            setRemote((r) => ({
              query: q,
              hits: [],
              unavailableWp: r.unavailableWp,
              failed: true,
            }));
        },
      );
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [q, searchable]);

  // an answer for another query is stale: the list shows the local matches until the right one arrives
  const fresh = searchable && remote.query === q;
  const hits = fresh ? remote.hits : NO_HITS;
  const searching = searchable && !fresh;
  const failed = fresh && remote.failed;

  const items = useMemo(() => {
    // remote hits were chosen by the server: they are not filtered again by the local matcher
    const matched = rankItems(local, query);
    return groupItems([...matched, ...hits.map(hitToItem)]);
  }, [local, query, hits]);

  const current = Math.min(active, Math.max(0, items.length - 1));

  const choose = (item: PaletteItem | undefined): void => {
    if (!item) return;
    onClose();
    item.run();
  };

  const onKeyDown = (e: ReactKeyboardEvent): void => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive(Math.min(items.length - 1, current + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive(Math.max(0, current - 1));
    } else if (e.key === 'Home') {
      e.preventDefault();
      setActive(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      setActive(items.length - 1);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      choose(items[current]);
    } else if (e.key === 'Tab') {
      e.preventDefault(); // focus stays in the palette
    }
  };

  useEffect(() => {
    document.getElementById(`${listId}-${String(current)}`)?.scrollIntoView({ block: 'nearest' });
  }, [current, listId]);

  return (
    <div
      className="adm-overlay adm-overlay--top"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="adm-palette"
        role="dialog"
        aria-modal="true"
        aria-label={S.palette.title}
        onKeyDown={onKeyDown}
      >
        <div className="adm-palette__input">
          <Icon name="search" />
          <input
            ref={inputRef}
            type="text"
            role="combobox"
            aria-expanded="true"
            aria-controls={listId}
            aria-activedescendant={items.length > 0 ? `${listId}-${String(current)}` : undefined}
            aria-autocomplete="list"
            autoComplete="off"
            spellCheck={false}
            placeholder={S.palette.placeholder}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
          />
        </div>
        <ul id={listId} role="listbox" className="adm-palette__list" aria-label={S.palette.title}>
          {items.map((item, i) => {
            const heading = i === 0 || items[i - 1]?.kind !== item.kind;
            return (
              <li key={item.id} role="presentation">
                {heading ? <div className="adm-palette__group">{KIND_LABEL[item.kind]}</div> : null}
                <div
                  id={`${listId}-${String(i)}`}
                  role="option"
                  aria-selected={i === current}
                  className={`adm-palette__item ${i === current ? 'is-active' : ''}`}
                  onMouseMove={() => {
                    if (i !== current) setActive(i);
                  }}
                  onClick={() => {
                    choose(item);
                  }}
                >
                  {item.icon ? <Icon name={item.icon} /> : <span className="adm-palette__noicon" />}
                  <span className="adm-ellipsis adm-grow">
                    {item.title}
                    {item.subtitle ? <span className="adm-muted"> {item.subtitle}</span> : null}
                  </span>
                  {item.online === true ? (
                    <span className="adm-badge adm-badge--ok">online</span>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
        {items.length === 0 ? (
          <div className="adm-palette__empty" role="status">
            {searching ? S.palette.searching : S.palette.empty}
            {!searching ? <div className="adm-muted adm-small">{S.palette.emptyHint}</div> : null}
          </div>
        ) : null}
        <footer className="adm-palette__foot">
          <span className="adm-muted adm-small">
            {failed
              ? S.palette.failed
              : remote.unavailableWp
                ? S.palette.stub(remote.unavailableWp)
                : S.palette.hintKeys}
          </span>
        </footer>
      </div>
    </div>
  );
}
