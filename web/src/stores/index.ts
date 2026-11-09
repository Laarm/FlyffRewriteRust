// Stores (Plan/40 section 6): the registry, the core stores, the publisher and the React bindings.
// React-free modules are exported here; the React bindings (`StoresProvider`, `useStoreSel`, `useNow`)
// come from `./framework` (the only module of this directory that imports React, eslint/rules.js).
export { createCoreStores, type CoreStores } from './core';
export { initialDevState, type DevState } from './dev';
export {
  createPublisher,
  publishIfChanged,
  type PublishHz,
  type Publisher,
  type PublisherOptions,
  type PublisherStats,
} from './publisher';
export {
  createStoreRegistry,
  ObserverWriteError,
  type PersistSpec,
  type ResettableScope,
  type StoreDef,
  type StoreInfo,
  type StoreRegistry,
  type StoreRegistryOptions,
  type StoreScope,
} from './registry';
export {
  CONNECTED_SCREENS,
  type FatalInfo,
  type LoadingGate,
  type LoadingReason,
  type ScreenName,
  type ScreenState,
  type UiNotice,
} from './screen';
export { initialSelfState, type DerivedStatsView, type SelfState, type Vec3Tuple } from './self';
export {
  applyHelloAck,
  initialSessionState,
  type BootstrapAuth,
  type ConnectionState,
  type OverlayKind,
  type PrepareState,
  type ServerStatus,
  type SessionState,
} from './session';
export { initialWorldState, worldMapReset, type LoadingPhase, type WorldState } from './world';
