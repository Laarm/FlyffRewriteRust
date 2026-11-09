// The input layer (Plan/43; WP-INPUT-01..08): keyboard routing and the key map, pointer plumbing, picking and the cursor, the
// ground control record, click-to-move and follow, the hotkeys and the Esc chain, and the `CBackCamera` port. `createInput(deps)`
// is the `InputApi` of the game loop; `app/inputHost.ts` builds its ports from the client context. Layering rules: eslint/rules.js.
export { createInput } from './input';
export type { InjectEvent, InputDebug, InputInstance, InputSimEvent } from './input';
export {
  KEYMAP,
  APPLET_HOTKEYS,
  KeyAction,
  codesOf,
  modifierGateOpen,
  shouldPreventDefault,
} from './keymap';
export { KeyState } from './keys';
export type { KeyCode, Modifiers } from './keys';
export { InputRouter } from './router';
export type { InputMode, KeyEventLike, RouterDeps, RouterWindows } from './router';
export {
  PointerController,
  WorldMouse,
  DBLCLK_MS,
  DBLCLK_PX,
  LR_JUMP_MS,
  LR_SUPPRESS_MS,
} from './pointer';
export { WheelNormalizer } from './wheel';
export { AutoRun } from './autorun';
export { BackCamera } from './camera/backCamera';
export type { BoundResult, CameraSubject } from './camera/backCamera';
export { CameraDriver, mouseSpeedFactor } from './camera/cameraControls';
export { groundPick, pickObject, pickRayLH, rayToThree, rayVsProxyBox } from './picking';
export { selectCursor } from './cursor';
export { escChain } from './escChain';
export { LocalCommands } from './commands';
export { Targeting } from './targeting';
export type {
  ChatPort,
  InputDeps,
  InputFeatures,
  InputHooks,
  InputOptions,
  ObjectInfo,
  RenderPort,
  SelfInfo,
  SimPort,
  TaskbarPort,
  WorldWindows,
} from './ports';
