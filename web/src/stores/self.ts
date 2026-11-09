// selfStore (Plan/40 section 6.2.2; scope `world`): the field contract of 45 section 2.3. Continuous
// values (hp, mp, fp, exp, expPercentX100, pos) are written only by the Publisher at <= 10 Hz (pos 4 Hz),
// everything else by handlers (the reducers are `selfReducers.ts`, WP-CLIENT-29).

export type Vec3Tuple = readonly [number, number, number];

/** Derived-stat view filled from `RulesContext` (Plan/40 section 7.10, WP-CLIENT-30). */
export type DerivedStatsView = Readonly<Record<string, number>>;

export interface SelfState {
  readonly objId: number;
  readonly idPlayer: number;
  readonly name: string;
  /** 0 male, 1 female. */
  readonly sex: number;
  /** `JOB_*` id (defineJob.h). */
  readonly job: number;
  readonly level: number;
  readonly exp: number;
  readonly expPercentX100: number;
  readonly deathExp: number;
  readonly deathLevel: number;
  readonly hp: number;
  readonly mp: number;
  readonly fp: number;
  readonly maxHp: number;
  readonly maxMp: number;
  readonly maxFp: number;
  readonly base: {
    readonly str: number;
    readonly sta: number;
    readonly dex: number;
    readonly int: number;
  };
  readonly statPoints: number;
  readonly skillPoints: number;
  readonly gold: number;
  /** `AUTH_*` letter code of the character. */
  readonly authority: number;
  readonly modeFlags: number;
  readonly worldId: number;
  /** Logical LH position (Plan/01 section 8). */
  readonly pos: Vec3Tuple;
  readonly isFlying: boolean;
  readonly guildId: number;
  readonly partyId: number;
  readonly fame: number;
  readonly smTimers: readonly { readonly kind: number; readonly endUnix: number }[];
  readonly pk: { readonly pink: boolean; readonly propensity: number; readonly value: number };
  readonly derived: DerivedStatsView | null;
  /** Flight experience (`SetFxp`, `PlayerSelf.fxp`). */
  readonly fxp: number;
  /** Flight level (`SetFxp`). */
  readonly flightLevel: number;
  /**
   * The own character is dead: set by `MoverDeath`, cleared by the revival messages and by the first `SetPointParam` that
   * brings HP back (Plan/17 section 11.1). The death and revival windows (45) read it.
   */
  readonly dead: boolean;
}

export function initialSelfState(): SelfState {
  return {
    objId: 0,
    idPlayer: 0,
    name: '',
    sex: 0,
    job: 0,
    level: 1,
    exp: 0,
    expPercentX100: 0,
    deathExp: 0,
    deathLevel: 0,
    hp: 0,
    mp: 0,
    fp: 0,
    maxHp: 0,
    maxMp: 0,
    maxFp: 0,
    base: { str: 0, sta: 0, dex: 0, int: 0 },
    statPoints: 0,
    skillPoints: 0,
    gold: 0,
    authority: 0,
    modeFlags: 0,
    worldId: 0,
    pos: [0, 0, 0],
    isFlying: false,
    guildId: 0,
    partyId: 0,
    fame: 0,
    smTimers: [],
    pk: { pink: false, propensity: 0, value: 0 },
    derived: null,
    fxp: 0,
    flightLevel: 0,
    dead: false,
  };
}
