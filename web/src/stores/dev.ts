// devStore (Plan/40 section 6.2.4; scope `never`): debug toggles, never persisted.

export interface DevState {
  /** `?debug=net`: packet inspector. */
  readonly net: boolean;
  /** `?debug=perf`: performance overlay. */
  readonly perf: boolean;
  readonly wireframe: boolean;
  readonly colliders: boolean;
}

export function initialDevState(): DevState {
  return { net: false, perf: false, wireframe: false, colliders: false };
}
