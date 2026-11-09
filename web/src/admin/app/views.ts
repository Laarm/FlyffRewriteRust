// The view registry (Plan/25 section 11.2 `views/`): each category has ONE lazily loaded module, `views/<category>/index.tsx`, whose
// default export is a `CategoryModule`. The shell loads it when the category (or a record of its kind) is opened, so nothing of a
// category is fetched before it is used and every category is its own chunk (the map chunk loads only on the Map tab).
//
// To add a tab: create the component in `views/<category>/`, then list it in that category's `index.tsx`:
//
//     const category: CategoryModule = { tabs: { characters: CharactersTab }, drawers: { character: CharacterDrawer } };
//     export default category;
//
// A tab the module does not list shows the NeedsFeature state with the work package of `app/nav.ts`.
import type { ComponentType, ReactNode } from 'react';
import type { Cap } from '../gen/caps';
import type { IconName } from '../components/Icon';
import type { CategoryId, TabDef } from './nav';
import type { RecordKind, RecordRef, Route } from './route';

/** What the shell passes to the component of a tab. */
export interface TabProps {
  /** The current route (the view reads and writes its own query parameters with `stores/route.navigate`). */
  readonly route: Route;
  readonly tab: TabDef;
}

/** What a drawer body can ask of the drawer around it. */
export interface DrawerControls {
  readonly close: () => void;
  /** 720 px instead of 520 px (the Items tab of a character). */
  readonly setWide: (wide: boolean) => void;
  /** The title and subtitle of the drawer header (the character's name, "Lv 15 Mercenary"). */
  readonly setHeader: (title: string, subtitle?: ReactNode) => void;
  /** Copies the deep link of this record. */
  readonly copyLink: () => void;
}

export interface DrawerBodyProps {
  readonly record: RecordRef;
  readonly controls: DrawerControls;
}

/** An entry of the command palette that a category contributes ("ban ...", "broadcast", "grant <item> to <player>"). */
export interface PaletteAction {
  readonly id: string;
  readonly label: string;
  /** Extra words the fuzzy match also finds it by. */
  readonly keywords?: readonly string[];
  /** The capability that shows it. */
  readonly cap?: Cap;
  readonly icon?: IconName;
  readonly run: () => void;
}

export interface CategoryModule {
  readonly tabs: Readonly<Partial<Record<string, ComponentType<TabProps>>>>;
  readonly drawers?: Readonly<Partial<Record<RecordKind, ComponentType<DrawerBodyProps>>>>;
  readonly actions?: readonly PaletteAction[];
}

/** The category that owns each record kind's drawer body (`DrawerBodyProps` components). */
export const RECORD_OWNER: Readonly<Record<RecordKind, CategoryId>> = {
  character: 'players',
  account: 'players',
  guild: 'guilds',
  item: 'items',
  monster: 'world',
};

type Loader = () => Promise<{ default: CategoryModule }>;

/** One dynamic import per category (a static map, so the bundler makes one chunk each). */
const LOADERS: Readonly<Record<CategoryId, Loader>> = {
  dashboard: () => import('../views/dashboard'),
  world: () => import('../views/world'),
  players: () => import('../views/players'),
  items: () => import('../views/items'),
  guilds: () => import('../views/guilds'),
  content: () => import('../views/content'),
  comm: () => import('../views/comm'),
  server: () => import('../views/server'),
  audit: () => import('../views/audit'),
};

const cache = new Map<CategoryId, Promise<CategoryModule>>();
const resolved = new Map<CategoryId, CategoryModule>();

/** Loads (once) the module of a category. A failed load is forgotten so that the next try fetches again. */
export function loadCategory(id: CategoryId): Promise<CategoryModule> {
  let p = cache.get(id);
  if (!p) {
    p = LOADERS[id]().then(
      (m) => {
        resolved.set(id, m.default);
        return m.default;
      },
      (error: unknown) => {
        cache.delete(id);
        throw error;
      },
    );
    cache.set(id, p);
  }
  return p;
}

/** The modules loaded so far (the palette offers their actions without loading the rest). */
export function loadedCategories(): readonly CategoryModule[] {
  return [...resolved.values()];
}

/** Tests: replaces what a category loads (pass `null` to restore). */
export function setCategoryForTests(id: CategoryId, module: CategoryModule | null): void {
  if (module === null) {
    cache.delete(id);
    resolved.delete(id);
  } else {
    cache.set(id, Promise.resolve(module));
    resolved.set(id, module);
  }
}
