// The rules runtime of the game (Plan/40 section 7.10; WP-CLIENT-30): the DST replica sync, the derived stats of the own
// character and the `RulesContext` lifecycle. See `runtime.ts`.
export {
  DERIVED_PERIOD_MS,
  peekRulesRuntime,
  rulesConfigOf,
  rulesModule,
  rulesRuntime,
  type RulesRuntime,
  type RulesRuntimeOptions,
} from './runtime';
export { derivedMatches, equipOf, flattenDerived, statInputOf, weaponItemOf } from './selfStats';
export {
  MAX_HELD_OPS,
  RulesSync,
  type RulesPort,
  type RulesSyncDeps,
  type RulesSyncStats,
} from './sync';
