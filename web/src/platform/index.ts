// Platform layer: the only code that touches storage, console and page visibility (Plan/02 11.7).
export { createLogger, formatEntry, log, type LogEntry, type Logger, type LogLevel } from './log';
export { createSafeStorage, safeStorage, type SafeStorage } from './storage';
export {
  createSessionKeys,
  session,
  SESSION_KEYS,
  type InWorldMarker as SessionInWorldMarker,
  type SessionKeys,
} from './session';
export { persistedStore, type PersistedStore, type PersistedStoreOptions } from './persistedStore';
export {
  clientFrame,
  clientFrameAt,
  FRAME_MS,
  now,
  setTimeSource,
  wallNow,
  type TimeSource,
} from './time';
export { isHidden, onVisibility, type Unsubscribe, type VisibilityEvent } from './visibility';
export { hasGesture, onFirstGesture } from './gesture';
