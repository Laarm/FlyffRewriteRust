// The sun occlusion test of the lens flare (Plan/41 section 14.5; SkyBox.cpp:803-827 `CheckSun`): the original picks the terrain and
// every object's bounding box along the ray from the camera towards `RotX(angle) * (0, 0, 300)`. Visual only (R15): the terrain
// is the CPU height field of the renderer (a march along the ray), the objects are the resident static objects' boxes
// (`StaticObjects.rayHitsBoxes`); movers and ground items do not occlude the sun in this port.

import type { Vec3LH } from '../types';
import type { SunOcclusion } from './LensFlare';

/** The terrain queries of the march. */
export interface HeightSource {
  heightAt(x: number, z: number): number;
}

/** The static objects' box test (three.js space is the callee's business; the arguments are LH). */
export interface BoxSource {
  rayHitsBoxes(origin: Vec3LH, dir: Vec3LH, maxDist: number): boolean;
}

/** March length and step of the terrain test (the far plane is 512; the test needs only the near field). */
export const TERRAIN_MARCH = { maxDist: 400, step: 2 } as const;

/** `true` when the ray dips below the terrain surface within `maxDist` (the first step starts at the camera). */
export function terrainBlocks(
  field: HeightSource,
  origin: Vec3LH,
  dir: Vec3LH,
  maxDist: number = TERRAIN_MARCH.maxDist,
  step: number = TERRAIN_MARCH.step,
): boolean {
  for (let t = step; t <= maxDist; t += step) {
    const y = origin[1] + dir[1] * t;
    // a ray that climbs away from every possible height can stop early
    if (dir[1] > 0 && y > 2000) return false;
    if (y < field.heightAt(origin[0] + dir[0] * t, origin[2] + dir[2] * t)) return true;
  }
  return false;
}

/** Terrain and object boxes together. */
export function createOcclusion(field: HeightSource, statics: BoxSource | null): SunOcclusion {
  return {
    blocked(origin, dir) {
      if (terrainBlocks(field, origin, dir)) return true;
      return statics?.rayHitsBoxes(origin, dir, TERRAIN_MARCH.maxDist) ?? false;
    },
  };
}
