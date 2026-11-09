// Distance classes, LOD groups and the occluding-object fade of static objects (Plan/41 sections 11.4-11.6,
// World3D.cpp:672-682, Obj.cpp:246-282 / 342-366; ref/21 section 6.3.2).

/** `MD_*` view-distance classes of a model registry entry. */
export const MD = { far: 0, mid: 1, near: 2, fix: 3 } as const;

/**
 * `fDistant[distantOption][modelElem.distant]` (World3D.cpp:672-682): rows = `RenderOptions.objectDistant` 0 (high),
 * 1 (default), 2 (low), columns = `MD_FAR, MD_MID, MD_NEAR, MD_FIX`.
 */
export const F_DISTANT: readonly (readonly [number, number, number, number])[] = [
  [400, 200, 70, 400],
  [250, 150, 60, 400],
  [150, 100, 40, 400],
];

/** The cull distance of a model of class `distant` under option `option`; an unknown class uses MD_FIX. */
export function cullDistance(option: 0 | 1 | 2, distant: number): number {
  const row = F_DISTANT[option] ?? F_DISTANT[1] ?? [250, 150, 60, 400];
  return row[Math.min(3, Math.max(0, Math.trunc(distant)))] ?? row[3];
}

/** `true` when an object at `dist` is drawn (the original skips `dist > fDistant`). */
export function withinDistance(dist: number, option: 0 | 1 | 2, distant: number): boolean {
  return dist <= cullDistance(option, distant);
}

/** The LOD group step of `RenderOptions.objectDetail` 0/1/2 (`50 / 30 / 15`, Obj.cpp:342-366). */
export const LOD_STEP: readonly [number, number, number] = [50, 30, 15];

/**
 * The LOD group of an object at camera distance `dist`: indoor worlds use group 0, else `min(2, floor(dist / d))`.
 * Models with fewer groups clamp it themselves (`ModelInstance.setLod`).
 */
export function lodGroupFor(dist: number, detail: 0 | 1 | 2, indoor: boolean): number {
  if (indoor) return 0;
  const step = LOD_STEP[detail];
  return Math.min(2, Math.max(0, Math.trunc(dist / step)));
}

/** The minimum blend factor of a faded occluder. */
export const FADE_MIN = 64;

/**
 * One tick of the `m_bTrans` fade (Obj.cpp:246-282), `blend` 64..255. `nearer`: the object is nearer to the camera than
 * the active mover; `hit`: the ray camera -> mover hits the object's model. Nearer and hit: `-10` (min 64); nearer and no
 * hit: `+10` (max 255); farther: `+5` (max 255).
 */
export function stepFade(blend: number, nearer: boolean, hit: boolean): number {
  if (nearer) {
    return hit ? Math.max(FADE_MIN, blend - 10) : Math.min(255, blend + 10);
  }
  return Math.min(255, blend + 5);
}

/** Objects are processed (fades, animation) only within `farPlane / 2` of the camera (World.cpp:789-835). */
export const PROCESS_RADIUS = 256;
