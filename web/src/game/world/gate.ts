// The loading gate (Plan/40 section 9.8 step 5-7, WP-CLIENT-23; ref/21 port note 6): while the screen is `Loading` the gate
// opens, with `gateAssetsReady`, when ALL of these hold:
//
//   * collision Ready for every land of the window (so no remote mover in range walks on a `Pending` land) - present or not;
//   * terrain Ready for every present land of the inner 3 x 3 (the render side, `RenderApi.worldGate`);
//   * the own player's skeleton and STAND clip are in the sim and its parts are loaded (the pose feeder + `ownModelReady`);
//   * the static models of the player's own land are loaded (the render side);
//   * the dialog windows of the UI are loaded (`ui.windowsReady()`: their chunks start loading with the UI and are done long before).
//
// `SpawnSelf` processed is the state machine's own half of the condition (`gate.selfSpawned`). The weighted progress is
// terrain 40 % (over the present lands of the inner 3 x 3), collision 20 % (over every land of the window), own model 30 %,
// own-land statics 10 %; a category with nothing to wait for counts as complete and `progress` reaches 1 exactly when the gate
// opens. After 60 s the gate opens anyway (the state machine's `gateTimeout`, a warning in the log): missing pieces stream in.
// The state machine owns the screen and the in-world marker; this controller only evaluates the conditions.

import type { ClientContext } from '../../app/context';
import type { WorldState } from '../../stores/world';
import type { WorldStreamer } from '../../assets/land';
import type { PoseFeeder } from '../poseFeeder';

/** The gate waits at most this long (ms). */
export const GATE_TIMEOUT_MS = 60_000;

export interface GateSnapshot {
  readonly terrain: { readonly ready: number; readonly wanted: number };
  readonly collision: { readonly ready: number; readonly wanted: number };
  readonly statics: { readonly ready: number; readonly wanted: number };
  readonly ownModel: boolean;
  readonly worldKnown: boolean;
  readonly progress: number;
  readonly phase: WorldState['loading']['phase'];
  readonly open: boolean;
}

const frac = (c: { ready: number; wanted: number }): number =>
  c.wanted <= 0 ? 1 : Math.min(1, c.ready / c.wanted);

export class GateController {
  private lastProgress = -1;
  private lastPhase: WorldState['loading']['phase'] | null = null;
  private opened = false;
  private timedOutFor: number | null = null;

  constructor(
    private readonly ctx: ClientContext,
    private readonly streamer: WorldStreamer,
    private readonly feeder: PoseFeeder,
    /** `true` once the world description (sim, render, streamer) was applied. */
    private readonly worldKnown: () => boolean,
  ) {}

  /** A new `Loading` (enter, world change, re-attach): the gate starts over. */
  begin(): void {
    this.opened = false;
    this.timedOutFor = null;
    this.lastProgress = -1;
    this.lastPhase = null;
  }

  /** The conditions of this moment (pure reads); `selfSpawned` is the state machine's half of the gate. */
  snapshot(selfSpawned = true): GateSnapshot {
    const known = this.worldKnown();
    const collision = this.streamer.gate();
    const render = this.ctx.render;
    const rg = render?.worldGate?.() ?? null;
    const terrain = rg ? rg.terrain : { ready: 0, wanted: 0 };
    const statics = rg ? rg.statics : { ready: 0, wanted: 0 };
    const selfId = this.ctx.entities.self()?.objId ?? 0;
    const ownModel =
      selfSpawned &&
      this.feeder.ownReady() &&
      (render?.ownModelReady ? render.ownModelReady(selfId) : true);
    const renderOpen = rg ? rg.open : true;
    const collisionOpen = collision.ready >= collision.wanted && known;
    const windowsReady = this.ctx.ui?.windowsReady?.() ?? true;
    const open = known && selfSpawned && collisionOpen && renderOpen && ownModel && windowsReady;
    let progress =
      0.4 * frac(terrain) + 0.2 * frac(collision) + 0.3 * (ownModel ? 1 : 0) + 0.1 * frac(statics);
    if (!known) progress = 0;
    else if (open) progress = 1;
    else progress = Math.min(progress, 0.99);
    const phase: WorldState['loading']['phase'] = !known
      ? 'world'
      : !collisionOpen || (rg && !(terrain.ready >= terrain.wanted))
        ? 'lands'
        : open
          ? 'ready'
          : 'models';
    return { terrain, collision, statics, ownModel, worldKnown: known, progress, phase, open };
  }

  /** Called every 60 Hz step; evaluates the gate while the screen is `Loading` and updates `worldStore.loading`. */
  tick(nowMs: number): void {
    const screen = this.ctx.machine.screen;
    if (screen.s !== 'Loading') return;
    const snap = this.snapshot(screen.gate.selfSpawned);
    if (snap.progress !== this.lastProgress || snap.phase !== this.lastPhase) {
      this.lastProgress = snap.progress;
      this.lastPhase = snap.phase;
      this.ctx.stores.worldStore.setState({
        loading: { phase: snap.phase, progress: snap.progress },
      });
    }
    if (snap.open && !this.opened) {
      this.opened = true;
      this.ctx.machine.dispatch({ type: 'gateAssetsReady' });
      return;
    }
    if (
      this.timedOutFor !== screen.gate.startedAtMs &&
      nowMs - screen.gate.startedAtMs >= GATE_TIMEOUT_MS
    ) {
      this.timedOutFor = screen.gate.startedAtMs;
      this.ctx.log.warn(
        'world',
        `loading gate timed out after ${String(GATE_TIMEOUT_MS / 1000)} s ` +
          `(collision ${String(snap.collision.ready)}/${String(snap.collision.wanted)}, ` +
          `terrain ${String(snap.terrain.ready)}/${String(snap.terrain.wanted)}, own model ${String(snap.ownModel)})`,
      );
      this.ctx.machine.dispatch({ type: 'gateTimeout', now: nowMs });
    }
  }
}
