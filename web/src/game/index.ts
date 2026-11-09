// Game runtime (Plan/40 sections 6.3, 7, 10): the entity table, the fixed-step loop with its ports, the tick
// systems and the hidden-tab pump. `simBridge.ts` (WP-CLIENT-18), `moveReporter.ts` (-20) and `world/`
// (-19/-23) are added by their work packages; session actions: `./session`.
export {
  createBackgroundPump,
  PUMP_INTERVAL_MS,
  type BackgroundPump,
  type BackgroundPumpDeps,
} from './background';
export {
  createEntityTable,
  EntityKind,
  type AssetHandleLike,
  type EntityRecord,
  type EntityRemoveReason,
  type EntitySpawn,
  type EntitySpawnInit,
  type EntityTable,
  type EntityViewSource,
  type MoverVisualState,
} from './entities';
export { FrameTimeRing, SectionTimes, SlidingSum } from './frameStats';
export {
  createGameLoop,
  DEFAULT_BUDGET_MS,
  FRAME_MS,
  MAX_STEPS_PER_ITERATION,
  OVERLOAD_ITERATION_MS,
  rafScheduler,
  type FrameScheduler,
  type GameLoop,
  type IterationInfo,
  type LoopDeps,
  type LoopSection,
  type LoopStats,
} from './loop';
export {
  NULL_AUDIO,
  NULL_INPUT,
  NULL_LOOP_PORTS,
  NULL_SIM,
  type AudioApi,
  type CameraSample,
  type InputApi,
  type LoopPorts,
  type MoveReporterApi,
  type PoseFeederApi,
  type RenderApi,
  type RenderFrame,
  type SimBridge,
  type SimFrameResult,
  type UiApi,
  type UploadsApi,
} from './ports';
export { createTickSystems, type TickSystemFn, type TickSystems } from './tickSystems';
