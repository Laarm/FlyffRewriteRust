// The navigation model of the console (Plan/25 section 2.2): nine categories in the plan's order, each with its tabs, the
// capability that opens a tab and the work package that builds it. Pure data and pure functions; the shell renders them.
//
// Gating: a tab is visible when the session holds the tab's capability; a category is visible when at least one of its tabs is.
// The server decides for real (every route and subscription checks the capability); this only hides what would be refused.
import type { Cap } from '../gen/caps';
import type { IconName } from '../components/Icon';

/** The URL slug of a category (`#/<category>/<tab>`). */
export type CategoryId =
  'dashboard' | 'world' | 'players' | 'items' | 'guilds' | 'content' | 'comm' | 'server' | 'audit';

/** MUST / SHOULD / MAY (Plan/25 section 0.5). */
export type Need = 'MUST' | 'SHOULD' | 'MAY';

export interface TabDef {
  /** The URL slug of the tab. */
  readonly id: string;
  readonly label: string;
  /** The capability that shows the tab (the server checks the finer ones per action). */
  readonly cap: Cap;
  /** The work package that builds the view (shown by the NeedsFeature state while the view is missing). */
  readonly wp: string;
  readonly need: Need;
}

export interface CategoryDef {
  readonly id: CategoryId;
  readonly label: string;
  readonly icon: IconName;
  readonly tabs: readonly TabDef[];
  /** The one-line purpose (title of the navigation entry and of the guide link). */
  readonly summary: string;
  /** The anchor of this category in `docs/admin-console.md` (the "?" links of the shell). */
  readonly guide: string;
}

const tab = (id: string, label: string, cap: Cap, wp: string, need: Need = 'SHOULD'): TabDef => ({
  id,
  label,
  cap,
  wp,
  need,
});

/** The nine categories in navigation order. */
export const CATEGORIES: readonly CategoryDef[] = [
  {
    id: 'dashboard',
    label: 'Dashboard',
    icon: 'dashboard',
    summary: 'Status cards, metrics history, alerts and the latest admin actions',
    guide: 'dashboard',
    tabs: [
      tab('overview', 'Overview', 'view.dashboard', 'WP-ADMIN-25'),
      tab('alerts', 'Alerts', 'view.dashboard', 'WP-ADMIN-25'),
      tab('activity', 'Activity', 'view.dashboard', 'WP-ADMIN-25'),
    ],
  },
  {
    id: 'world',
    label: 'Live World',
    icon: 'map',
    summary: 'The live map of every world and layer, and the live instances',
    guide: 'live-world',
    tabs: [
      tab('map', 'Map', 'view.map', 'WP-ADMIN-09', 'MUST'),
      tab('instances', 'Instances & layers', 'view.map', 'WP-ADMIN-10'),
    ],
  },
  {
    id: 'players',
    label: 'Players',
    icon: 'users',
    summary: 'Who is online, characters, accounts and moderation',
    guide: 'players',
    tabs: [
      tab('online', 'Online', 'view.players', 'WP-ADMIN-18', 'MUST'),
      tab('characters', 'Characters', 'view.players', 'WP-ADMIN-18', 'MUST'),
      tab('accounts', 'Accounts', 'view.players', 'WP-ADMIN-18'),
      tab('moderation', 'Moderation', 'view.players', 'WP-ADMIN-18'),
    ],
  },
  {
    id: 'items',
    label: 'Items & Economy',
    icon: 'package',
    summary: 'Item builder, grants and removals, kits, ownership search and the economy',
    guide: 'items-and-economy',
    tabs: [
      tab('builder', 'Item builder', 'act.items', 'WP-ADMIN-21', 'MUST'),
      tab('kits', 'Kits & templates', 'act.items', 'WP-ADMIN-21'),
      tab('grants', 'Grants & removals', 'act.items', 'WP-ADMIN-21', 'MUST'),
      tab('search', 'Item search', 'view.inventory', 'WP-ADMIN-21'),
      tab('economy', 'Economy', 'view.inventory', 'WP-ADMIN-22', 'MAY'),
    ],
  },
  {
    id: 'guilds',
    label: 'Guilds',
    icon: 'shield',
    summary: 'Guild list and detail; the guild creation wizard with the logo picker',
    guide: 'guilds',
    tabs: [
      tab('guilds', 'Guilds', 'view.players', 'WP-ADMIN-24'),
      tab('create', 'Create guild', 'act.guild', 'WP-ADMIN-24', 'MUST'),
    ],
  },
  {
    id: 'content',
    label: 'World & Content',
    icon: 'globe',
    summary: 'Rates, event flags, weather, spawns, content panels and the shared locations',
    guide: 'world-and-content',
    tabs: [
      tab('rates', 'Rates & events', 'view.dashboard', 'WP-ADMIN-30'),
      tab('spawns', 'Spawns', 'view.map', 'WP-ADMIN-30'),
      tab('content', 'Content', 'view.map', 'WP-ADMIN-31'),
      tab('locations', 'Locations', 'view.map', 'WP-ADMIN-14'),
    ],
  },
  {
    id: 'comm',
    label: 'Communication',
    icon: 'message',
    summary: 'Broadcast, scheduled and recurring notices; system mail',
    guide: 'communication',
    tabs: [
      tab('notices', 'Notices', 'act.broadcast', 'WP-ADMIN-27'),
      tab('mail', 'System mail', 'act.items', 'WP-ADMIN-27'),
    ],
  },
  {
    id: 'server',
    label: 'Server',
    icon: 'server',
    summary: 'Sessions, staff and access, configuration, scheduler, backups, logs and the console',
    guide: 'server',
    tabs: [
      tab('sessions', 'Sessions & network', 'view.players', 'WP-ADMIN-26'),
      tab('staff', 'Staff & access', 'view.staff', 'WP-ADMIN-18'),
      tab('config', 'Configuration', 'act.server', 'WP-ADMIN-26'),
      tab('scheduler', 'Scheduler', 'act.server', 'WP-ADMIN-26'),
      tab('backups', 'Backups & maintenance', 'act.server', 'WP-ADMIN-26'),
      tab('logs', 'Logs', 'act.server', 'WP-ADMIN-26'),
      tab('console', 'Console', 'act.server', 'WP-ADMIN-26'),
    ],
  },
  {
    id: 'audit',
    label: 'Audit',
    icon: 'scroll',
    summary: 'The audit log and your own recent actions with undo',
    guide: 'audit',
    tabs: [
      tab('log', 'Audit log', 'view.audit', 'WP-ADMIN-29'),
      tab('mine', 'My actions', 'view.audit.own', 'WP-ADMIN-29'),
    ],
  },
];

/** The capabilities of a session, as a set. */
export type CapSet = ReadonlySet<string>;

export function capSet(caps: readonly string[]): CapSet {
  return new Set(caps);
}

export function categoryById(id: string): CategoryDef | undefined {
  return CATEGORIES.find((c) => c.id === id);
}

export function isCategoryId(id: string): id is CategoryId {
  return CATEGORIES.some((c) => c.id === id);
}

/** The tabs of `category` that `caps` opens, in order. */
export function visibleTabs(category: CategoryDef, caps: CapSet): readonly TabDef[] {
  return category.tabs.filter((t) => caps.has(t.cap));
}

/** The categories with at least one permitted tab, in navigation order (a category without any is hidden). */
export function visibleCategories(caps: CapSet): readonly CategoryDef[] {
  return CATEGORIES.filter((c) => visibleTabs(c, caps).length > 0);
}

/** What a location in the console resolves to for a session. */
export type Resolved =
  | { readonly kind: 'tab'; readonly category: CategoryDef; readonly tab: TabDef }
  /** The category exists but the session may open none of the requested tab (or of any tab). */
  | { readonly kind: 'forbidden'; readonly category: CategoryDef; readonly tab: TabDef | null }
  | { readonly kind: 'unknown' };

/**
 * Resolves `category` and an optional `tab` slug for a session. Without a tab the last remembered tab of the category (if still
 * permitted) or its first permitted tab opens. A tab slug the category does not have falls back the same way; a tab the session
 * may not open resolves to `forbidden` (a deep link to it shows "no access" instead of silently showing another tab).
 */
export function resolveLocation(
  categoryId: string,
  tabId: string | null,
  caps: CapSet,
  lastTabs: Readonly<Record<string, string>> = {},
): Resolved {
  const category = categoryById(categoryId);
  if (!category) return { kind: 'unknown' };
  const allowed = visibleTabs(category, caps);
  const requested = tabId === null ? undefined : category.tabs.find((t) => t.id === tabId);
  if (requested) {
    return caps.has(requested.cap)
      ? { kind: 'tab', category, tab: requested }
      : { kind: 'forbidden', category, tab: requested };
  }
  const remembered = allowed.find((t) => t.id === lastTabs[category.id]);
  const pick = remembered ?? allowed[0];
  return pick ? { kind: 'tab', category, tab: pick } : { kind: 'forbidden', category, tab: null };
}

/** The first location a session can open: the Dashboard, else the first permitted category (`null` when it can open none). */
export function landing(
  caps: CapSet,
  preferred: CategoryId = 'dashboard',
): { category: CategoryDef; tab: TabDef } | null {
  const order = [preferred, ...CATEGORIES.map((c) => c.id)];
  for (const id of order) {
    const r = resolveLocation(id, null, caps);
    if (r.kind === 'tab') return { category: r.category, tab: r.tab };
  }
  return null;
}
