// The core world handler modules of doc 40 (Plan/40 section 1.3, 5.4; WP-CLIENT-19, -20, -23): registered in `CORE_MODULES` of
// `app/features.ts`, after the connection and session modules.
//
//   `world`          WorldReadInfo, Replace (replace.ts), Spawn*, DelObj (spawn.ts), GameTimer, Environment, EnvironmentAll
//                    (environment.ts), the own character (self.ts: SpawnSelf, SetPointParam, SetExperience, SetLevel)
//   `world.movement` the movement messages (movement.ts) and the move reporter's armed BEHAVIOR report

import type { HandlerModule } from '../../app/features';
import { registerEnvironmentHandlers } from './environment';
import { registerMovementHandlers } from './movement';
import { registerReplaceHandlers } from './replace';
import { worldRuntime } from './runtime';
import { registerSelfHandlers } from './self';
import { registerSpawnHandlers } from './spawn';

export const worldModule: HandlerModule = {
  id: 'world',
  register(ctx) {
    const rt = worldRuntime(ctx);
    registerReplaceHandlers(ctx, rt);
    registerSpawnHandlers(ctx, rt);
    registerSelfHandlers(ctx, rt.live);
    registerEnvironmentHandlers(ctx);
  },
};

export const movementModule: HandlerModule = {
  id: 'world.movement',
  register(ctx) {
    worldRuntime(ctx);
    registerMovementHandlers(ctx);
  },
};

export const WORLD_MODULES: readonly HandlerModule[] = [worldModule, movementModule];

export { peekWorldRuntime, worldRuntime, type WorldRuntime } from './runtime';
export { describeWorld, type WorldDescription } from './describe';
export { clockSampleAt, gameClockNow, type GameClockSample } from './environment';
export { GATE_TIMEOUT_MS, GateController, type GateSnapshot } from './gate';
export { regionAt, type RegionState } from './region';
