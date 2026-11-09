// The state of the navigator window (`CWndNavigator` members: the place filters, `m_size`, the region name in the title, the own mark
// `m_nvPoint`, `m_idMurderer`; WP-CFEAT-28): plain in-memory state with subscribers. The filters and the zoom are persisted with the
// window rect by the window (`SerializeRegInfo`); the model only holds them.

import type { NavigatorView } from '../../ui/windows/nav/ports';
import { DEFAULT_FILTERS, type NavFilters, OBJFILTER } from './dots';
import { clampZoom, stepZoom, ZOOM_DEFAULT } from './geometry';

/** `Time = 200` render frames of the own mark at 60 fps (the party marks use the same 3 334 ms). */
export const OWN_MARK_MS = 3334;

export interface OwnMark {
  readonly x: number;
  readonly y: number;
  readonly z: number;
  /** `ctx.now()` when the mark was set. */
  readonly at: number;
}

export class NavigatorModel {
  #filters: NavFilters = DEFAULT_FILTERS;
  #zoom = ZOOM_DEFAULT;
  #region = '';
  #mark: OwnMark | null = null;
  #murderer = 0;
  #version = 0;
  #view: NavigatorView | null = null;
  #viewIndoor = false;
  readonly #listeners = new Set<() => void>();

  get filters(): NavFilters {
    return this.#filters;
  }
  get zoom(): number {
    return this.#zoom;
  }
  get regionName(): string {
    return this.#region;
  }
  /** `m_idMurderer`: the player who killed the character last (0 = nobody). */
  get murdererId(): number {
    return this.#murderer;
  }
  get version(): number {
    return this.#version;
  }

  /** The own mark while it is still drawn (`Time != 0`). */
  ownMark(now: number): OwnMark | null {
    const m = this.#mark;
    return m && now - m.at < OWN_MARK_MS ? m : null;
  }

  #bump(): void {
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }

  subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };

  /** A tick of the place menu: item `id` (`OBJFILTER_*`) flips. Returns the new state of that filter. */
  toggleFilter(id: number): boolean | null {
    const f = this.#filters;
    switch (id) {
      case OBJFILTER.PLAYER:
        this.#filters = { ...f, player: !f.player };
        this.#bump();
        return this.#filters.player;
      case OBJFILTER.PARTY:
        this.#filters = { ...f, party: !f.party };
        this.#bump();
        return this.#filters.party;
      case OBJFILTER.NPC:
        this.#filters = { ...f, npc: !f.npc };
        this.#bump();
        return this.#filters.npc;
      case OBJFILTER.MONSTER:
        this.#filters = { ...f, monster: !f.monster };
        this.#bump();
        return this.#filters.monster;
      default:
        return null;
    }
  }

  /** The zoom buttons (`+`/`-` by 32, clamped to 128..352). */
  zoomBy(direction: 1 | -1): number {
    const next = stepZoom(this.#zoom, direction);
    if (next !== this.#zoom) {
      this.#zoom = next;
      this.#bump();
    }
    return this.#zoom;
  }

  /** `SerializeRegInfo` read back: the filters and the zoom of the previous session. */
  restore(filters: NavFilters, zoom: number): void {
    this.#filters = filters;
    this.#zoom = clampZoom(zoom);
    this.#bump();
  }

  /** `SetRegionName`: `''` shows the default title. */
  setRegionName(name: string): void {
    if (name === this.#region) return;
    this.#region = name;
    this.#bump();
  }

  /** `OnLButtonDown`: the own mark at a world position. */
  setOwnMark(x: number, y: number, z: number, now: number): void {
    this.#mark = { x, y, z, at: now };
  }

  clearOwnMark(): void {
    this.#mark = null;
  }

  setMurderer(idPlayer: number): void {
    this.#murderer = idPlayer;
  }

  /** The world was left: the region name and the mark go. */
  reset(): void {
    this.#region = '';
    this.#mark = null;
    this.#murderer = 0;
    this.#bump();
  }

  /** The read model of the window: the same object while nothing it shows changed. */
  view(indoor: boolean): NavigatorView {
    if (this.#view && this.#view.version === this.#version && this.#viewIndoor === indoor) {
      return this.#view;
    }
    this.#viewIndoor = indoor;
    this.#view = {
      version: this.#version,
      regionName: this.#region,
      zoom: this.#zoom,
      filters: this.#filters,
      indoor,
    };
    return this.#view;
  }
}
