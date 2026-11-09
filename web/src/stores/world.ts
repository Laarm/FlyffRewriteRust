// worldStore (Plan/40 section 6.2.3; scope `world`, the per-map fields reset with scope `map`).

export type LoadingPhase = 'idle' | 'world' | 'lands' | 'models' | 'ready';

export interface WorldState {
  readonly worldId: number;
  readonly layer: number;
  readonly folder: string;
  readonly title: string;
  /** Meters per unit of the land grid (`MPU` of the .wld file). */
  readonly mpu: number;
  readonly lands: readonly [number, number];
  readonly loading: { readonly phase: LoadingPhase; readonly progress: number };
  /** From the self position, 4 Hz. */
  readonly region: {
    readonly title: string | null;
    readonly attribute: number;
    readonly safe: boolean;
    readonly music: number;
  };
  readonly env: { readonly season: number; readonly weather: number };
  /** Section 5.6: the sample 41 consumes is computed from this. */
  readonly gameClock: { readonly baseMs: number; readonly receivedAtServerMs: number };
  /** Entity counts, 1 Hz. */
  readonly index: {
    readonly version: number;
    readonly players: number;
    readonly npcs: number;
    readonly items: number;
  };
}

export function initialWorldState(): WorldState {
  return {
    worldId: 0,
    layer: 0,
    folder: '',
    title: '',
    mpu: 4,
    lands: [0, 0],
    loading: { phase: 'idle', progress: 0 },
    region: { title: null, attribute: 0, safe: false, music: 0 },
    env: { season: 0, weather: 0 },
    gameClock: { baseMs: 0, receivedAtServerMs: 0 },
    index: { version: 0, players: 0, npcs: 0, items: 0 },
  };
}

/** The per-map part of the world state: reset with scope `map` (Replace), Plan/40 section 6.5. */
export function worldMapReset(): Partial<WorldState> {
  return {
    loading: { phase: 'idle', progress: 0 },
    region: { title: null, attribute: 0, safe: false, music: 0 },
    index: { version: 0, players: 0, npcs: 0, items: 0 },
  };
}
