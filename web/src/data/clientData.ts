// `ClientDataIndex` (Plan/40 section 3.4 rule 6, WP-CLIENT-08): typed lookups over the loaded bundles, built lazily
// per bundle, exposed to 45 as `BundleRegistry`/`TextApi` (45 section 2.2), to 42 as `UiHost.bundles` and to 44 as
// `AudioDeps.data`. Lookups return `undefined` for holes (never throw). `text`/`textColor` keep the GETTEXT rules of
// 14 section 10.6 (hole: `""`/0; at or above the table size: `"error"`/`0xFFFFFFFF`); before the `texts` bundle is in
// they answer the out-of-range values too.

import type {
  BundleDataMap,
  CtrlProp,
  BundleName,
  ItemProp,
  JobRow,
  ModelRow,
  MoverProp,
  NpcRow,
  RowTable,
  SfxRow,
  WorldFolderDto,
  WorldRow,
} from '../gen/bundles';
import type { Unsubscribe } from '../platform/visibility';
import { RowLookup } from './lookup';
import {
  buildTextTable,
  COLOR_OUT_OF_RANGE,
  getText,
  getTextColor,
  TEXT_OUT_OF_RANGE,
  type TextTable,
} from './rowTable';

/** `items.items` / `skills.skills` rows (the client has no `*_key` columns). */
export type ClientItem = Omit<ItemProp, 'name_key' | 'description_key'>;
export type ClientMover = Omit<MoverProp, 'name_key' | 'ex'>;

/** The read side of the loader that the index needs. */
export interface BundleSource {
  get<N extends BundleName>(name: N): BundleDataMap[N] | undefined;
  has(name: BundleName): boolean;
  subscribe(listener: (name: BundleName) => void): Unsubscribe;
}

/** The text API of 45 (`TextApi`). */
export interface TextApi {
  text(tid: number): string;
  textColor(tid: number): number;
}

export class ClientDataIndex implements TextApi {
  private readonly memo = new Map<string, unknown>();
  private versionValue = 0;
  private readonly listeners = new Set<(name: BundleName) => void>();
  private readonly off: Unsubscribe;

  constructor(private readonly bundles: BundleSource) {
    this.off = bundles.subscribe((name) => {
      // a bundle arrived: drop what was derived from the previous state of that name and tell the observers
      for (const k of [...this.memo.keys()]) if (k.startsWith(`${name}:`)) this.memo.delete(k);
      this.versionValue += 1;
      for (const l of [...this.listeners]) l(name);
    });
  }

  /** Bumps whenever a bundle was added (a cheap change check for React selectors). */
  get version(): number {
    return this.versionValue;
  }

  /** Observes the arrival of bundles. */
  onBundle(listener: (name: BundleName) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  dispose(): void {
    this.off();
    this.listeners.clear();
  }

  /** `true` once the bundle is loaded. */
  has(name: BundleName): boolean {
    return this.bundles.has(name);
  }

  /** The parsed `data` section of a bundle (`undefined` until it is loaded). */
  bundle<N extends BundleName>(name: N): BundleDataMap[N] | undefined {
    return this.bundles.get(name);
  }

  private memoized<T>(key: string, build: () => T | undefined): T | undefined {
    if (this.memo.has(key)) return this.memo.get(key) as T | undefined;
    const v = build();
    if (v !== undefined) this.memo.set(key, v);
    return v;
  }

  private lookup<T>(
    name: BundleName,
    table: string,
    pick: () => RowTable<T> | undefined,
  ): RowLookup<T> | undefined {
    return this.memoized(`${name}:${table}`, () => {
      const t = pick();
      return t ? new RowLookup<T>(t) : undefined;
    });
  }

  // ---- texts ---------------------------------------------------------------------------------------------------

  private textTable(): TextTable | undefined {
    return this.memoized('texts:table', () => {
      const b = this.bundles.get('texts');
      return b ? buildTextTable(b) : undefined;
    });
  }

  /** `GETTEXT(tid)`. */
  text(tid: number): string {
    const t = this.textTable();
    return t ? getText(t, tid) : TEXT_OUT_OF_RANGE;
  }

  /** `GETTEXTCOLOR(tid)`. */
  textColor(tid: number): number {
    const t = this.textTable();
    return t ? getTextColor(t, tid) : COLOR_OUT_OF_RANGE;
  }

  /** `true` when `tid` has text (not a hole, not out of range). */
  hasText(tid: number): boolean {
    return this.textTable()?.texts.has(tid) ?? false;
  }

  // ---- items, skills, movers, npcs ---------------------------------------------------------------------------------

  items(): RowLookup<ClientItem> | undefined {
    return this.lookup('items', 'items', () => this.bundles.get('items')?.items);
  }
  item(id: number): ClientItem | undefined {
    return this.items()?.get(id);
  }

  skills(): RowLookup<ClientItem> | undefined {
    return this.lookup('skills', 'skills', () => this.bundles.get('skills')?.skills);
  }
  skill(id: number): ClientItem | undefined {
    return this.skills()?.get(id);
  }

  partySkill(id: number): ClientItem | undefined {
    return this.lookup('skills', 'party', () => this.bundles.get('skills')?.party_skills)?.get(id);
  }

  movers(): RowLookup<ClientMover> | undefined {
    return this.lookup('movers', 'movers', () => this.bundles.get('movers')?.movers);
  }
  mover(id: number): ClientMover | undefined {
    return this.movers()?.get(id);
  }

  ctrl(id: number): CtrlProp | undefined {
    return this.lookup('movers', 'ctrls', () => this.bundles.get('movers')?.ctrls)?.get(id);
  }

  /** NPC definitions by key (`MaFl_Vendor`). */
  npc(key: string): NpcRow | undefined {
    const map = this.memoized('npcs:byKey', () => {
      const b = this.bundles.get('npcs');
      return b ? new Map(b.npcs.map((n) => [n.key, n])) : undefined;
    });
    return map?.get(key);
  }

  // ---- worlds, models, sounds, jobs ------------------------------------------------------------------------------------

  /** A world of `World.inc` by id. */
  world(id: number): WorldRow | undefined {
    const map = this.memoized('worlds:byId', () => {
      const b = this.bundles.get('worlds');
      return b ? new Map(b.worlds.map((w) => [w.id, w])) : undefined;
    });
    return map?.get(id);
  }

  /** The client data of a world folder (`.wld` settings, regions, continents); `folder` in any letter case. */
  worldFolder(folder: string): WorldFolderDto | undefined {
    return this.bundles.get('worlds')?.folders[folder.toLowerCase()];
  }

  /** The model registry row of `(ot, index)` (`OT_MOVER` 5, ...), `undefined` for a hole. */
  model(ot: number, index: number): ModelRow | undefined {
    return this.modelLookup(ot)?.get(index);
  }

  private modelLookup(ot: number): RowLookup<ModelRow> | undefined {
    return this.memoized(`models:ot${String(ot)}:index`, () => {
      const t = this.bundles.get('models')?.types[String(ot)];
      return t ? new RowLookup<ModelRow>(t, 'index') : undefined;
    });
  }

  sfx(id: number): SfxRow | undefined {
    return this.lookup('sfx-table', 'sfx', () => this.bundles.get('sfx-table')?.sfx)?.get(id);
  }

  /**
   * `sound/<file>` of a `SND_`/`VOC_` id: a path, `""` = silent, `null` = the file is missing, `undefined` = no such
   * id (or the bundle is not loaded).
   */
  sound(id: number): string | null | undefined {
    return soundValue(this.bundles.get('sounds')?.sounds[String(id)]);
  }

  /** `music/<file>` of a `BGM_` id (`null` = missing, `undefined` = no such id). */
  music(id: number): string | null | undefined {
    return soundValue(this.bundles.get('music')?.music[String(id)]);
  }

  job(id: number): JobRow | undefined {
    const map = this.memoized('tables:jobs', () => {
      const b = this.bundles.get('tables');
      return b ? new Map(b.jobs.map((j) => [j.id, j])) : undefined;
    });
    return map?.get(id);
  }
}

function soundValue(v: unknown): string | null | undefined {
  if (v === null) return null;
  return typeof v === 'string' ? v : undefined;
}
