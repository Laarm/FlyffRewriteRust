// `features/taskbar` (WP-UI-19): the `TaskBar` ingest, the slot messages and the ports of the shortcut kinds.
export { taskbarModule, taskbarOf, motionIcon } from './taskbarModule';
export type { TaskbarFeature, TaskbarItemSource, TaskbarSkillSource } from './taskbarModule';
export {
  createWireActions,
  cutUtf8,
  MAX_SHORTCUT_TEXT,
  shortcutFromWire,
  shortcutToWire,
  snapshotFromWire,
} from './wire';
