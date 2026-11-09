// The placement routing of the input layer (Plan/43 §7.13, Plan/45 §28.10, §28.11; WP-INPUT-17; the C++ is `Neuz.cpp:1291-1330, 1384-1447, 1462-1485`, `WndWorld.cpp:6709-6713, 7014-7046, 7449-7453, 8994-9015,
// 11799-11830`, `Housing.cpp:1291-1345`). While a housing or guild-house furniture follows the pointer (`hooks.deploy.active()`), the input layer routes five things to the placement controller of `features/events/deploy`
// instead of the world:
//
//   left press       after the L+R chord rule and before any selection: `confirm()`; no focus, no `SetTargetReq`, no click destination (`OnLButtonDown`)
//   right release    on the spot where the button went down and with the menu allowed: `confirm()` and no player menu (`MenuException`)
//   left release     no ground control while active (`OnLButtonUp`), and none for the double click (`OnLButtonDblClk`)
//   wheel            `wheel( n )` (positive = towards the user = `zDelta < 0`: +2.5 degrees per notch); the camera ignores the wheel through `deployActive`
//   PageUp / PageDown key-up   `pageKey( code )`; the zoom keys are suppressed by the camera through `deployActive`
//   Escape           `cancel()` ends the placement (`true` = the key is taken: no Esc chain, no start menu); else the selected guild-house furniture is cleared and the chain runs
//   every world tick the ground point under the pointer (`RAY_TERRAIN` only, `ClientPointToVector( ..., FALSE )`) goes to `cursor( p )`; a miss is `cursor( null )` (keeps the last point), no query slot skips the tick
//
// The hook is the provider of the placement feature (`inputProvidersOf( ctx ).deploy`); without it every method answers "not mine" and the world behaves as before.

import type { Vec3LH } from '../render/types';
import { GROUND_PICK_RANGE, pickRayLH, RAY_TERRAIN } from './picking';
import type { InputCore } from './worldTick';

export class DeployRouting {
  /** The terrain point of the last cursor update (the probe and the tests read it), `null` for none. */
  lastPoint: Vec3LH | null = null;
  /** Cursor updates delivered (one per world tick with the pointer inside). */
  updates = 0;

  constructor(private readonly c: InputCore) {}

  /** `CDeployManager::IsReady() || GuildDeploy()->IsReady()`. */
  active(): boolean {
    const hooks = this.c.deps.hooks;
    return hooks?.deploy?.active() ?? hooks?.deployActive?.() ?? false;
  }

  /** The left press: `true` = the placement took it (the caller returns). */
  press(): boolean {
    const hook = this.c.deps.hooks?.deploy;
    if (hook?.active() !== true) return false;
    hook.confirm();
    return true;
  }

  /** The right release on the spot with the menu allowed: `true` = the placement took it (no player menu). */
  rightRelease(): boolean {
    return this.press();
  }

  /** Wheel notches over the world: `true` = rotated the furniture, the camera zoom must not see them. */
  wheel(notches: number): boolean {
    const hook = this.c.deps.hooks?.deploy;
    if (hook?.active() !== true) return false;
    if (notches !== 0) hook.wheel(notches);
    return true;
  }

  /** `PageUp` / `PageDown` key-up: `true` = the angle moved by 45 degrees. */
  pageKey(code: string): boolean {
    if (code !== 'PageUp' && code !== 'PageDown') return false;
    const hook = this.c.deps.hooks?.deploy;
    if (hook?.active() !== true) return false;
    hook.pageKey(code);
    return true;
  }

  /** The Esc key-down: `true` = a placement ended and the key is taken; else the selected furniture is cleared (the chain runs). */
  escape(): boolean {
    const hooks = this.c.deps.hooks;
    if (hooks?.deploy?.cancel() === true) return true;
    hooks?.clearFurnitureSelection?.();
    return false;
  }

  /** One world tick: the terrain point under the pointer goes to the placement while the pointer is over the world. */
  tick(): void {
    const c = this.c;
    const hook = c.deps.hooks?.deploy;
    if (hook?.active() !== true || !c.mouse.inside) return;
    const cam = c.displayed;
    if (cam === null) return;
    const vp = c.deps.viewport();
    if (vp.w <= 0 || vp.h <= 0) return;
    const ray = pickRayLH(cam, c.mouse.pos.x, c.mouse.pos.y, vp.w, vp.h);
    const hit = c.deps.sim.rayCast(ray.origin, ray.dir, GROUND_PICK_RANGE, RAY_TERRAIN);
    // no query slot this iteration: the next tick asks again
    if (hit === undefined) return;
    this.updates += 1;
    if (hit === null) {
      hook.cursor(null);
      return;
    }
    const p: Vec3LH = [hit.pos[0], hit.pos[1], hit.pos[2]];
    this.lastPoint = p;
    hook.cursor(p);
  }

  reset(): void {
    this.lastPoint = null;
  }
}
