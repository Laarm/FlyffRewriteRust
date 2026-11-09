// The layout registry: the 365 `resdata.inc` window layouts of the `resdata-layouts` bundle indexed by APP id and
// name, with controls in file order (Plan/42 §3.1-§3.3; Plan/14 §10.5; ref/22 §7).

import type { AppletRow, ControlRow, ResdataBundle } from '../../gen/bundles';
import { APP_ALIASES } from './styles';
import type { WType } from './styles';

/** A window layout (the bundle row; `controls` in file order). */
export interface AppletLayout extends Readonly<Omit<AppletRow, 'controls'>> {
  readonly controls: readonly ControlLayout[];
}
/** A control of a layout (bundle row; `type` is a `WTYPE_*` value, `id` is `-1` for the four undefined names). */
export interface ControlLayout extends Readonly<Omit<ControlRow, 'type' | 'rect'>> {
  readonly type: WType;
  /** `[left, top, right, bottom]` relative to the parent's CLIENT rect (never clamped; may be negative). */
  readonly rect: readonly [l: number, t: number, r: number, b: number];
}

/** A numeric APP id or an `APP_*` name (aliases of `AppDefine.h` included). */
export type AppKey = number | `APP_${string}`;
/** A numeric WIDC id or a `WIDC_*` name. */
export type WidcKey = number | `WIDC_${string}`;

/** The id of a control whose name is not defined (Plan/14 §6.19): reachable by name only. */
export const UNDEFINED_CONTROL_ID = -1;

/** Counts of a loaded registry (the C21 corpus numbers: 365 / 2 809 / 20 / 4 with the real data). */
export interface RegistryCounts {
  applets: number;
  controls: number;
  emptyApplets: number;
  undefinedNames: number;
}

/**
 * Lookup of window layouts. Layouts and controls are immutable views of the bundle.
 *
 * * `byId(app)` / `byName('APP_INVENTORY')` / `resolve(key)` find an applet (aliases `APP_SHOP_`,
 *   `APP_CONFIRM_BUY_`, `APP_MESSENGER_` resolve to the `…EX` layouts).
 * * `control(app, key)` finds a control by WIDC id or name. Several names can share a value
 *   (`WIDC_YES = WIDC_BTN_YES = 101`): a lookup by id returns the FIRST control in file order with that id.
 */
export class LayoutRegistry {
  #supplier: () => ResdataBundle | undefined;
  #applets: readonly AppletLayout[] = [];
  readonly #byId = new Map<number, AppletLayout>();
  readonly #byName = new Map<string, AppletLayout>();
  /** Per applet: id -> first control, name -> control (built lazily on the first control lookup). */
  readonly #controlIndex = new Map<AppletLayout, ControlIndex>();
  #loaded = false;

  private constructor(supplier: () => ResdataBundle | undefined) {
    this.#supplier = supplier;
  }

  /** Builds the registry of a parsed `resdata-layouts` bundle (`BundleEnvelope.data`). */
  static fromBundle(b: ResdataBundle): LayoutRegistry {
    return new LayoutRegistry(() => b);
  }

  /**
   * A registry over a bundle that may not be loaded yet (`UiHost.bundles.layouts` is `undefined` until the bundle
   * is in): it is EMPTY until the supplier returns the bundle, then indexes it once (never re-read).
   */
  static fromSupplier(supplier: () => ResdataBundle | undefined): LayoutRegistry {
    return new LayoutRegistry(supplier);
  }

  /** `true` once the bundle is available and indexed. */
  get isLoaded(): boolean {
    this.#ensure();
    return this.#loaded;
  }

  #ensure(): void {
    if (this.#loaded) return;
    const b = this.#supplier();
    if (!b) return;
    // the bundle rows already have the shape of AppletLayout/ControlLayout; the cast only narrows `type`
    const applets = b.applets as unknown as readonly AppletLayout[];
    this.#applets = applets;
    for (const a of applets) {
      // the first definition wins (the original keeps one applet per id; duplicates do not occur in the data)
      if (!this.#byId.has(a.id)) this.#byId.set(a.id, a);
      if (!this.#byName.has(a.name)) this.#byName.set(a.name, a);
    }
    this.#loaded = true;
  }

  /** Every applet in file order. */
  applets(): readonly AppletLayout[] {
    this.#ensure();
    return this.#applets;
  }

  get size(): number {
    this.#ensure();
    return this.#applets.length;
  }

  byId(appId: number): AppletLayout | undefined {
    this.#ensure();
    return this.#byId.get(appId);
  }

  /** `byName('APP_INVENTORY')`; the `AppDefine.h` aliases resolve to their layout. */
  byName(name: string): AppletLayout | undefined {
    this.#ensure();
    const direct = this.#byName.get(name);
    if (direct) return direct;
    const alias = APP_ALIASES[name];
    return alias === undefined ? undefined : this.#byId.get(alias);
  }

  /** An applet by numeric id or `APP_*` name. */
  resolve(key: AppKey): AppletLayout | undefined {
    return typeof key === 'number' ? this.byId(key) : this.byName(key);
  }

  /** The numeric APP id of a key (`undefined` when no layout exists: dead ids are never created). */
  appId(key: AppKey): number | undefined {
    return this.resolve(key)?.id;
  }

  /** A control of `app` by WIDC id (first in file order; `-1` finds nothing) or by name. */
  control(app: AppletLayout, key: WidcKey): ControlLayout | undefined {
    const index = this.#indexOf(app);
    return typeof key === 'number' ? index.byId.get(key) : index.byName.get(key);
  }

  /** The numbers of the loaded set (the corpus facts the tests pin). */
  counts(): RegistryCounts {
    this.#ensure();
    let controls = 0;
    let emptyApplets = 0;
    let undefinedNames = 0;
    for (const a of this.#applets) {
      controls += a.controls.length;
      if (a.controls.length === 0) emptyApplets += 1;
      for (const c of a.controls) {
        if (c.id === UNDEFINED_CONTROL_ID) undefinedNames += 1;
      }
    }
    return { applets: this.#applets.length, controls, emptyApplets, undefinedNames };
  }

  #indexOf(app: AppletLayout): ControlIndex {
    let index = this.#controlIndex.get(app);
    if (!index) {
      index = { byId: new Map(), byName: new Map() };
      for (const c of app.controls) {
        if (c.id !== UNDEFINED_CONTROL_ID && !index.byId.has(c.id)) index.byId.set(c.id, c);
        if (!index.byName.has(c.name)) index.byName.set(c.name, c);
      }
      this.#controlIndex.set(app, index);
    }
    return index;
  }
}

interface ControlIndex {
  byId: Map<number, ControlLayout>;
  byName: Map<string, ControlLayout>;
}
