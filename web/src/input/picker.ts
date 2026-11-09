// The picker (Plan/43 §4.10, §5; WP-INPUT-06): the stateful face of `picking.ts`. One `hover()` per rendered frame (highlight,
// hover bracket, cursor) and `pickAt` / `selectObj` for the click handlers, both over the camera that was DISPLAYED.

import { WorldCursor, selectCursor, type CursorTarget } from './cursor';
import { pickObject, pickRayLH, rayToThree, type PickFilter, type PickHit } from './picking';
import type { InputCore } from './worldTick';

/** The apps of `BLOCKING_MINIGAMES_AND_SMELT` (Plan/43 §4.4): they block the world clicks and show `CUR_NO`. */
export interface PickerHost {
  /** An own vendor, a vendor window or a blocking mini-game / smelt window is open. */
  worldBlocked(): boolean;
}

export class Picker {
  /** `CObj::m_pObjHighlight`: the object under the pointer of the last rendered frame. */
  highlight: PickHit | null = null;
  readonly cursor: WorldCursor;
  /** The last pick (the debug surface reads it). */
  lastPick: { id: number; kind: string; t: number } | null = null;

  constructor(
    private readonly c: InputCore,
    private readonly host: PickerHost,
  ) {
    this.cursor = new WorldCursor((id) => {
      c.deps.setWorldCursor(id);
    });
  }

  private filter(): PickFilter {
    const c = this.c;
    const f = c.deps.features();
    const o = c.deps.options();
    return {
      selfId: c.deps.self()?.id ?? 0,
      onlyNpc: c.keys.isHeld('Tab'),
      hidePetsFeature: f?.hide_pets === true,
      offTargetOrPets: o.offTarget || o.offPets,
    };
  }

  /** The closest verified object under `(x, y)` (CSS px of the world canvas), `null` for none. */
  pickAt(x: number, y: number): PickHit | null {
    const c = this.c;
    const cam = c.displayed;
    const render = c.deps.render();
    if (cam === null || render === null) return null;
    const proxies = render.visibleObjects?.() ?? [];
    if (proxies.length === 0) return null;
    const vp = c.deps.viewport();
    if (vp.w <= 0 || vp.h <= 0) return null;
    const ray = rayToThree(pickRayLH(cam, x, y, vp.w, vp.h));
    // a renderer without the precise test accepts the box hit
    const hasPrecise = render.raycastPrecise !== undefined;
    const hit = pickObject(proxies, ray, this.filter(), (id, r) =>
      hasPrecise ? (render.raycastPrecise?.(id, r) ?? null) : 0,
    );
    this.lastPick = hit ? { id: hit.id, kind: hit.kind, t: hit.t } : null;
    return hit;
  }

  /**
   * `selectObj(p)` (§5.4): picks, rejects an invisible (GM `TRANSPARENT_MODE`) mover, and gives the pick the focus unless an
   * auto-attack runs (then the click is a next-target candidate). Returns the picked object.
   */
  selectObj(x: number, y: number): PickHit | null {
    return this.selectPicked(this.pickAt(x, y));
  }

  /** {@link Picker.selectObj} for an object the caller already picked. */
  selectPicked(picked: PickHit | null): PickHit | null {
    const c = this.c;
    let hit = picked;
    if (hit !== null && hit.kind === 'mover') {
      const info = c.deps.entityInfo(hit.id);
      // an invisible (GM `TRANSPARENT_MODE`) mover, and on the guild-war map a dead one (`WndWorld.cpp:6556-6590`), cannot be selected
      if (info?.transparent === true) hit = null;
      else if (info?.dead === true && c.deps.hooks?.guildWarMap?.() === true) hit = null;
    }
    if (!c.targeting.autoAttack) c.targeting.setFocus(hit?.id ?? null);
    return hit;
  }

  /** The target description of the cursor rule for an object. */
  private targetOf(hit: PickHit): CursorTarget {
    const info = this.c.deps.entityInfo(hit.id);
    return {
      kind: hit.kind,
      isPlayer: hit.proxy.isPlayer,
      peaceful: info?.peaceful ?? false,
      housing: info?.housing ?? false,
    };
  }

  /** Every rendered frame, after the camera (§4.10): highlight, hover bracket, renderer highlight, cursor. */
  hover(): void {
    const c = this.c;
    const render = c.deps.render();
    const self = c.deps.self();
    const m = c.mouse;
    if (c.deps.mode() !== 'game' || self === null) {
      this.clear(render);
      return;
    }
    if (!m.inside) {
      // over a window (or outside): nothing is highlighted, the UI owns the cursor; over a pass-through icon the canvas is still the
      // element under the pointer, so it must not keep the sword or hand of the last monster (BFIX1 #004: the default cursor)
      this.highlight = null;
      c.targeting.selectRenderObj = null;
      render?.setHighlight?.(0);
      this.cursor.apply(selectCursor({ middleDown: false, target: null, blocked: false }));
      return;
    }
    // `HighlightObj` is skipped while dead
    const hit = self.dead ? null : this.pickAt(m.pos.x, m.pos.y);
    this.highlight = hit;
    c.targeting.selectRenderObj = c.targeting.focus === null && hit ? hit.id : null;
    render?.setHighlight?.(hit?.id ?? 0);
    if (hit?.kind === 'item')
      c.deps.hooks?.itemTooltip?.(hit.id, render?.projectBounds?.(hit.id) ?? null);
    this.cursor.apply(
      selectCursor({
        middleDown: m.M.down,
        target: hit ? this.targetOf(hit) : null,
        blocked: this.host.worldBlocked(),
      }),
    );
  }

  private clear(render: ReturnType<InputCore['deps']['render']>): void {
    if (this.highlight !== null) render?.setHighlight?.(0);
    this.highlight = null;
    this.c.targeting.selectRenderObj = null;
  }
}
