// Per-browser settings of the console (Plan/25 section 2.1, 2.3): theme (light / dark / system), density, the navigation rail,
// the landing category and the last tab per category. They live in `safeStorage.local` (one JSON blob, try/catch wrapped) and
// are per viewer only; nothing here is shared between people.
import { create } from 'zustand';
import { safeStorage } from '../../platform/storage';

export type Theme = 'system' | 'light' | 'dark';
export type Density = 'comfortable' | 'compact';
export type Landing = 'dashboard' | 'world';

export const UI_STORAGE_KEY = 'flyff.admin.ui.v1';

export interface UiSettings {
  readonly theme: Theme;
  readonly density: Density;
  /** The navigation is an icon rail even where it would fit (the user's choice; narrow windows and the Map tab force it). */
  readonly navCollapsed: boolean;
  /** The category opened without a hash (a per-staff setting, Plan/25 section 2.1). */
  readonly landing: Landing;
  /** `category -> tab` last opened. */
  readonly lastTabs: Readonly<Record<string, string>>;
}

export const DEFAULT_UI: UiSettings = {
  theme: 'system',
  density: 'comfortable',
  navCollapsed: false,
  landing: 'dashboard',
  lastTabs: {},
};

/** Reads stored settings; anything malformed falls back to the defaults field by field. */
export function readUi(raw: string | null): UiSettings {
  if (raw === null) return DEFAULT_UI;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_UI;
  }
  if (typeof parsed !== 'object' || parsed === null) return DEFAULT_UI;
  const o = parsed as Record<string, unknown>;
  const lastTabs: Record<string, string> = {};
  if (typeof o['lastTabs'] === 'object' && o['lastTabs'] !== null) {
    for (const [k, v] of Object.entries(o['lastTabs'])) {
      if (typeof v === 'string') lastTabs[k] = v;
    }
  }
  return {
    theme:
      o['theme'] === 'light' || o['theme'] === 'dark' || o['theme'] === 'system'
        ? o['theme']
        : DEFAULT_UI.theme,
    density: o['density'] === 'compact' ? 'compact' : 'comfortable',
    navCollapsed: o['navCollapsed'] === true,
    landing: o['landing'] === 'world' ? 'world' : 'dashboard',
    lastTabs,
  };
}

interface UiState extends UiSettings {
  readonly setTheme: (theme: Theme) => void;
  readonly setDensity: (density: Density) => void;
  readonly setNavCollapsed: (collapsed: boolean) => void;
  readonly setLanding: (landing: Landing) => void;
  readonly rememberTab: (category: string, tab: string) => void;
}

function persist(s: UiSettings): void {
  const { theme, density, navCollapsed, landing, lastTabs } = s;
  safeStorage.local.set(
    UI_STORAGE_KEY,
    JSON.stringify({ theme, density, navCollapsed, landing, lastTabs }),
  );
}

/** Puts theme and density on <html> (`data-theme` is absent for "system"). */
export function applyToDocument(s: Pick<UiSettings, 'theme' | 'density'>): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  if (s.theme === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', s.theme);
  if (s.density === 'compact') root.setAttribute('data-density', 'compact');
  else root.removeAttribute('data-density');
}

export const useUi = create<UiState>()((set, get) => {
  const update = (changes: Partial<UiSettings>): void => {
    set(changes);
    const s = get();
    persist(s);
    applyToDocument(s);
  };
  return {
    ...readUi(safeStorage.local.get(UI_STORAGE_KEY)),
    setTheme: (theme) => {
      update({ theme });
    },
    setDensity: (density) => {
      update({ density });
    },
    setNavCollapsed: (navCollapsed) => {
      update({ navCollapsed });
    },
    setLanding: (landing) => {
      update({ landing });
    },
    rememberTab: (category, tab) => {
      if (get().lastTabs[category] === tab) return;
      update({ lastTabs: { ...get().lastTabs, [category]: tab } });
    },
  };
});

/** Applies the stored theme and density before the first render (no flash of the wrong theme). */
export function applyStoredUi(): void {
  applyToDocument(useUi.getState());
}

/** Tests: reloads the store from storage. */
export function reloadUiFromStorage(): void {
  useUi.setState(readUi(safeStorage.local.get(UI_STORAGE_KEY)));
  applyToDocument(useUi.getState());
}
