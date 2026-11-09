// The sky cross-fade schedule (Plan/41 section 14.3, ref/19 section 18, SkyBox.cpp:1397-1425). Pure data: the environment
// state carries the `SkyLayers` of the game clock and the sky renderer (`Sky.ts`, WP-REND-28: cylinder, cloud dome, sun / moon,
// lens flare) draws them.
//
// Hours are the 1-based game hours 1..24, `min` is 0..59; all arithmetic is integer, weights are 0..255.

/** The three sky textures of the schedule (the WORLD_ENVIR_INFO comment of the original is wrong: [0]=day, [1]=evening, [2]=night). */
export type SkyKind = 'day' | 'sunset' | 'night';

/** One drawn layer of the sky cylinder or of the cloud dome: the texture slot and its blend weight (0..255). */
export interface SkyLayer {
  readonly kind: SkyKind;
  readonly weight: number;
}

/** The layers drawn at one instant, in draw order (the first one is the older of a cross-fade). */
export interface SkyLayers {
  readonly layers: readonly SkyLayer[];
}

/** `255 - n * 255 / d` with the original integer arithmetic (C `int` division truncates). */
const down = (n: number, d: number): number => 255 - Math.trunc((n * 255) / d);
const up = (n: number, d: number): number => Math.trunc((n * 255) / d);

/** The cross-fade of hour `hour` and minute `min` (Plan/41 14.3; the same schedule drives the sky and the clouds). */
export function skySchedule(hour: number, min: number): SkyLayers {
  const h = Math.min(24, Math.max(1, Math.trunc(hour)));
  const m = Math.min(59, Math.max(0, Math.trunc(min)));
  if (h < 6) return { layers: [{ kind: 'night', weight: 255 }] };
  if (h === 6) {
    return {
      layers: [
        { kind: 'night', weight: down(m, 59) },
        { kind: 'day', weight: up(m, 59) },
      ],
    };
  }
  if (h <= 16) return { layers: [{ kind: 'day', weight: 255 }] };
  if (h === 17 || h === 18) {
    const t = h === 17 ? m : m + 60;
    return {
      layers: [
        { kind: 'day', weight: down(t, 119) },
        { kind: 'sunset', weight: up(t, 119) },
      ],
    };
  }
  if (h === 19) {
    return {
      layers: [
        { kind: 'sunset', weight: down(m, 59) },
        { kind: 'night', weight: up(m, 59) },
      ],
    };
  }
  return { layers: [{ kind: 'night', weight: 255 }] };
}

/** Names of the texture triple `[day, sunset, night]` in `Weather/` for a texture quality option (Plan/41 14.3). */
export function skyTextureNames(textureQuality: 0 | 1 | 2): {
  readonly sky: readonly [string, string, string];
  readonly cloud: readonly [string, string, string];
} {
  switch (textureQuality) {
    case 0:
      return {
        sky: ['skybox01.dds', 'skybox02.dds', 'skybox03.dds'],
        cloud: ['cloud01.dds', 'cloud02.dds', 'cloud03.dds'],
      };
    case 1:
      return {
        sky: ['skybox01_low.dds', 'skybox02_low.dds', 'skybox03.dds'],
        cloud: ['cloud01_low.dds', 'cloud02_low.dds', 'cloud03.dds'],
      };
    case 2:
      return {
        sky: ['skybox01_low.dds', 'skybox02_low.dds', 'skybox03_low.dds'],
        cloud: ['cloud01_low.dds', 'cloud02_low.dds', 'cloud03_low.dds'],
      };
  }
}
