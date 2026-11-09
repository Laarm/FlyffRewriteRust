// Visual-QA test hooks of the game page (Plan/60 section 13.2, Plan/41 section 26, WP-REND-17): `setCamera`, `cameraPath`,
// `setWeather` and `hideHud` of `__flyffDebug.test` (the fifth hook, `setGameHour`, lives next to `setGameClock` in
// `game/world/environment.ts` because it shares the pin), and the probe section `qa` that reads back what they set:
//
//   qa.camera()    the pose of the last `setCamera` (or `null`)
//   qa.path()      the name of the running camera path (or `null`)
//   qa.gameHour()  the hour the renderer draws with (the pin of `setGameClock` / `setGameHour`, else the server clock)
//   qa.weather()   'none' | 'rain' | 'snow' as `worldStore.env` holds it
//   qa.hud()       `false` after `hideHud(true)`
//
// The hooks exist only on dev builds or with `?e2e=1` (`probe.test`); without them `installQaHooks` does nothing and no `qa`
// section is attached. State is per probe (a `WeakMap`), never module-global. The camera applies to what is DRAWN: 43's input
// layer and the picking keep their own camera (QA captures only).

import type { FlyffDebugProbe, Vec3Tuple } from '../debug/probe';
import { SEASON_SUMMER, SEASON_WINTER } from '../gen/defs/define';
import { peekWorldRuntime } from '../game/world/runtime';
import type { CameraState, Vec3LH } from '../render/types';
import type { ClientContext } from './context';

/** The argument of `test.setCamera` and the pose `qa.camera()` answers. */
export interface QaCameraPose {
  /** The LH point looked at. */
  readonly pos: Vec3Tuple;
  /** Degrees; the view direction is `(sin yaw cos pitch, sin pitch, cos yaw cos pitch)`. */
  readonly yaw: number;
  /** Degrees; negative looks down. */
  readonly pitch: number;
  /** Distance of the eye behind `pos` along the view direction (0: the eye is at `pos`). */
  readonly dist: number;
}

/** What `__flyffDebug.qa` reads. */
export interface QaProbeSection {
  camera(): QaCameraPose | null;
  path(): string | null;
  gameHour(): number | null;
  weather(): 'none' | 'rain' | 'snow';
  hud(): boolean;
}

/** A scripted camera: the pose for the frame at `nowMs`, following the own character at `active`. */
export type QaCameraPath = (active: Vec3LH, nowMs: number) => QaCameraPose;

/** Milliseconds of one turn of `flaris-orbit`. */
export const ORBIT_PERIOD_MS = 24_000;

/** The named camera paths of `test.cameraPath(name)` (WP-TEST-15 and the visual-QA tooling use these). */
export const CAMERA_PATHS: Readonly<Record<string, QaCameraPath>> = {
  // around the own character at 14 units, looking down 20 degrees, one turn per 24 s of the frame clock
  'flaris-orbit': (active, nowMs) => ({
    pos: [active[0], active[1] + 1.3, active[2]],
    yaw:
      (((nowMs % ORBIT_PERIOD_MS) + ORBIT_PERIOD_MS) % ORBIT_PERIOD_MS) * (360 / ORBIT_PERIOD_MS),
    pitch: -20,
    dist: 14,
  }),
};

const RAD = Math.PI / 180;

/** The full `CameraState` of a pose: the eye `dist` behind `pos` (at `pos` for 0, then it looks 10 units ahead). */
export function cameraFromPose(pose: QaCameraPose): CameraState {
  const cy = Math.cos(pose.pitch * RAD);
  const dir: Vec3LH = [
    Math.sin(pose.yaw * RAD) * cy,
    Math.sin(pose.pitch * RAD),
    Math.cos(pose.yaw * RAD) * cy,
  ];
  const eye: Vec3LH = [
    pose.pos[0] - dir[0] * pose.dist,
    pose.pos[1] - dir[1] * pose.dist,
    pose.pos[2] - dir[2] * pose.dist,
  ];
  const look: Vec3LH =
    pose.dist > 0
      ? [pose.pos[0], pose.pos[1], pose.pos[2]]
      : [eye[0] + dir[0] * 10, eye[1] + dir[1] * 10, eye[2] + dir[2] * 10];
  return {
    pos: eye,
    lookAt: look,
    up: [0, 1, 0],
    fovDiv: 4,
    telescope: 0,
    yawDeg: pose.yaw,
    mode: 'normal',
  };
}

interface QaState {
  camera: QaCameraPose | null;
  path: string | null;
  hud: boolean;
}

const states = new WeakMap<FlyffDebugProbe, QaState>();

const finite = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n);

/** Checks a pose of `setCamera` (a malformed pose throws: a spec that mistypes a field must not silently draw the old picture). */
function checkPose(pose: QaCameraPose): QaCameraPose {
  const p = pose.pos as readonly unknown[];
  if (p.length !== 3 || !p.every(finite)) throw new Error('setCamera: pos must be [x, y, z]');
  if (!finite(pose.yaw) || !finite(pose.pitch) || !finite(pose.dist) || pose.dist < 0) {
    throw new Error('setCamera: yaw, pitch and dist (>= 0) must be finite numbers');
  }
  return {
    pos: [pose.pos[0], pose.pos[1], pose.pos[2]],
    yaw: pose.yaw,
    pitch: pose.pitch,
    dist: pose.dist,
  };
}

/**
 * The camera of the QA hooks for the frame being drawn, or `null` (no hook active: the input camera or the stand-in is used).
 * `active` is the own character's position (LH) of `renderHost`.
 */
export function qaCamera(
  ctx: ClientContext,
  nowMs: number,
  active: Vec3LH | null,
): CameraState | null {
  const state = states.get(ctx.probe);
  if (!state) return null;
  if (state.camera) return cameraFromPose(state.camera);
  if (state.path !== null && active) {
    const path = CAMERA_PATHS[state.path];
    if (path) return cameraFromPose(path(active, nowMs));
  }
  return null;
}

const STYLE_ATTR = 'data-flyff-qa';
const HUD_CSS =
  '[data-testid="ui-root"],[data-testid="world-overlay"]{visibility:hidden !important;}';

function setHudHidden(on: boolean): void {
  if (typeof document === 'undefined') return;
  const html = document.documentElement;
  const known = document.head.querySelector(`style[${STYLE_ATTR}]`);
  if (on) {
    html.dataset['hud'] = 'off';
    if (!known) {
      const style = document.createElement('style');
      style.setAttribute(STYLE_ATTR, '');
      style.textContent = HUD_CSS;
      document.head.append(style);
    }
  } else {
    delete html.dataset['hud'];
    known?.remove();
  }
}

/**
 * Registers the QA hooks and attaches the `qa` section. Returns at once when the probe has no test hooks (production build
 * without `?e2e=1`). Called once at the end of `createContextProbe`.
 */
export function installQaHooks(ctx: ClientContext, probe: FlyffDebugProbe): void {
  if (!probe.test) return;
  const state: QaState = { camera: null, path: null, hud: true };
  states.set(probe, state);

  probe.setTestHook('setCamera', (pose) => {
    // a pose replaces a running path; `null` hands the camera back to the input layer
    state.camera = pose === null ? null : checkPose(pose);
    if (pose !== null) state.path = null;
  });
  probe.setTestHook('cameraPath', (name) => {
    if (name !== null && !Object.hasOwn(CAMERA_PATHS, name)) {
      throw new Error(
        `cameraPath: unknown path "${name}" (known: ${Object.keys(CAMERA_PATHS).join(', ')})`,
      );
    }
    state.path = name;
    if (name !== null) state.camera = null;
  });
  probe.setTestHook('setWeather', (kind) => {
    const { worldStore } = ctx.stores;
    const env = worldStore.getState().env;
    // rain = summer and snow = winter, both with the effect on; `none` / `clear` turn the effect off and keep the season
    if (kind === 'rain') worldStore.setState({ env: { season: SEASON_SUMMER, weather: 1 } });
    else if (kind === 'snow') worldStore.setState({ env: { season: SEASON_WINTER, weather: 1 } });
    else if ((kind as string) === 'none' || (kind as string) === 'clear')
      worldStore.setState({ env: { ...env, weather: 0 } });
    else throw new Error(`setWeather: unknown kind "${kind as string}"`);
  });
  probe.setTestHook('hideHud', (on) => {
    state.hud = !on;
    setHudHidden(on);
  });

  const section: QaProbeSection = {
    camera: () => (state.camera ? { ...state.camera, pos: [...state.camera.pos] } : null),
    path: () => state.path,
    gameHour: () => peekWorldRuntime(ctx)?.environment.sample().hour ?? null,
    weather: () => {
      const env = ctx.stores.worldStore.getState().env;
      if (env.weather === 0) return 'none';
      return env.season === SEASON_WINTER ? 'snow' : 'rain';
    },
    hud: () => state.hud,
  };
  probe.extend('qa', section);
}
