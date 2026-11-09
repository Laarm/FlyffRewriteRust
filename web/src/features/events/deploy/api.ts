// The public face of the placement controller (`deployOf( ctx )`), in a module of its own so that the features that only START or END a placement (the housing list, the guild storage, the furniture menu) import it
// without the controller and its collision math.

import type { ClientContext } from '../../../app/context';

/** Which manager places: the mini room (`CDeployManager`) or the guild house (`CGuildDeployManager`). */
export type DeployKind = 'housing' | 'guild';

/** The state of a running placement (`null` = idle). */
export interface DeployState {
  readonly kind: DeployKind;
  /** The guild mode: 2 = SETUP, 4 = RESET (0 for the mini room). */
  readonly mode: 0 | 2 | 4;
  readonly itemId: number;
  /** The ctrl model of the item (`prop.link_kind`). */
  readonly ctrlId: number;
  /** The index in the furniture list of the house (guild), -1 for the mini room. */
  readonly slot: number;
  /** The object id the placement ignores in the collision test (a reinstall: the original piece), `NULL_ID` for none. */
  readonly exceptObjId: number;
  readonly pos: readonly [number, number, number];
  readonly angle: number;
  readonly keyAngle: number;
  readonly collide: boolean;
}

export interface DeployFeature {
  /** A placement runs. */
  active(): boolean;
  /** The state of the placement, `null` when idle. */
  state(): DeployState | null;
  /** `LoadToDeploy` of the mini room: the furniture of `itemId` follows the pointer (an active placement ends first, nothing sent). */
  beginHousing(itemId: number): boolean;
  /** `LoadToDeploy` of the guild house: the furniture of the row `slot`; `mode` 2 = SETUP, 4 = RESET. */
  beginGuild(slot: number, mode: 2 | 4): boolean;
  /** The pointer's terrain point (every world tick while the pointer is inside), `null` keeps the last one. */
  cursor(p: readonly [number, number, number] | null): void;
  /** The wheel: `angle += 2.5 * notches` (positive = towards the user). */
  wheel(notches: number): void;
  /** PageDown / PageUp key-up: +45 / -45 degrees of the key accumulator. */
  pageKey(code: 'PageUp' | 'PageDown'): void;
  /** The click: refused while colliding (`TID_GAME_HOUSING_INSTALL_FAIL01`), else the placement ends and the request is sent. */
  confirm(): void;
  /** Esc or any other end: the placement ends without a send; `true` when one was running. */
  cancel(): boolean;
}

const features = new WeakMap<ClientContext, DeployFeature>();

/** The placement controller of a context; `undefined` before the module registered. */
export function deployOf(ctx: ClientContext): DeployFeature | undefined {
  return features.get(ctx);
}

/** Called by the module once it exists. */
export function setDeployOf(ctx: ClientContext, feature: DeployFeature): void {
  features.set(ctx, feature);
}
