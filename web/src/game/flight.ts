// The flight state of the own character as the HUD needs it (Plan/40 section 7.4 `SetFuel`, Plan/41 section 19.4 turbo gauge and speed
// digits; WP-CLIENT-37): the turbo fuel and the speed of the flight.
//
//   fuel      `SetFuel.fuel`: the fuel of the ride (`m_nFuel`, the server decides when it is empty)
//   accFuel   `m_tmAccFuel`, the turbo time in ms: `SetFuel.acc_fuel` every 16 server ticks while the player flies and accelerates, and
//             `ftoi(accFuel - 1000/60)` per stepped frame in between while ACC and TURBO are on (the sim burns the same, 31 section 9.2:
//             on BOTH roles, so the gauge does not wait for the next message). The mirror is display state only: the sim owns the real one.
//   speed     `g_nFlySpeed`: `ftoi(ftoi(|v| * 60 * 60 * 60) / 200)`, the digits next to the gauge
//
// The state is per context (`flightOf(ctx)`); `installFlight(ctx)` registers the 60 Hz tick system that burns the mirror.

import type { ClientContext } from '../app/context';
import { PARTS_RIDE } from '../gen/defs/neuz';
import { REPORT } from '../gen/sim/wire';
import { OBJMSG } from './combat/constants';
import { EntityKind } from './entities';
import { OBJSTAF } from './world/constants';
import { peekWorldRuntime } from './world/runtime';

/** The turbo gauge is drawn against 12 seconds of acceleration (`WndWorld.cpp:1015`, `12 * 1000`). */
export const FUEL_GAUGE_MAX = 12 * 1000;
/** `1000 / 60`: the turbo time burnt by one frame (`fly.rs TURBO_FUEL_MS_PER_FRAME`, the f32 of the original). */
export const TURBO_BURN_MS = 16.666666;

/** `ftoi` of the x86 build: truncation towards zero. */
const ftoi = (x: number): number => Math.trunc(x);

/** `g_nFlySpeed` of a per-frame velocity (`ActionMoverState2.cpp:474-475`). */
export function flySpeedDigits(vx: number, vy: number, vz: number): number {
  const len = Math.sqrt(vx * vx + vy * vy + vz * vz);
  return ftoi(ftoi(len * 60 * 60 * 60) / 200);
}

export class FlightState {
  /** `m_nFuel` (the last `SetFuel`). */
  fuel = 0;
  /** `m_tmAccFuel` in ms (the last `SetFuel` minus the turbo burnt since). */
  accFuel = 0;
  /** `g_nFlySpeed` of the last frame that flew. */
  speed = 0;
  /** `SetFuel` messages received (the probe). */
  messages = 0;
  /** The ride rule of the equip notifications (take-off and landing of the sim copy). */
  readonly rides = new RideSync();

  /** `OnSetFuel`: the server's figures replace the mirror. */
  onSetFuel(fuel: number, accFuel: number): void {
    this.fuel = fuel | 0;
    this.accFuel = Math.min(accFuel, 0x7fff_ffff) | 0;
    this.messages += 1;
  }

  /** `frames` stepped frames with the turbo burning (ACC and TURBO on): the same truncation as the sim, clamped at 0. */
  burn(frames: number): void {
    for (let i = 0; i < frames && this.accFuel > 0; i += 1) {
      this.accFuel = Math.max(0, ftoi(this.accFuel - TURBO_BURN_MS));
    }
  }

  reset(): void {
    this.fuel = 0;
    this.accFuel = 0;
    this.speed = 0;
    this.rides.reset();
  }
}

const states = new WeakMap<ClientContext, FlightState>();

/** The flight state of a context (created on first use). */
export function flightOf(ctx: ClientContext): FlightState {
  let s = states.get(ctx);
  if (!s) {
    s = new FlightState();
    states.set(ctx, s);
  }
  return s;
}

/** Ticks a take-off or a landing waits for the parameters record of the sim (the model bounds of the world are not loaded yet). */
const RIDE_SYNC_PATIENCE = 60;

/**
 * The ride rule of `RedoEquip` (ref/10 section 13.6, `MoverEquip.cpp:1960-2040`) for the client copy of a player: a worn ride that
 * the mover does not fly on makes it fly (`OBJMSG_MODE_FLY`, forced), no ride on a flying mover lands it (`OBJMSG_MODE_GROUND`,
 * forced). The server runs the same rule on its copy (`flyff_game::movement::flying::reconcile`) and tells the clients only the
 * equipment (`DoEquipNtf`); the sim of the client boards or leaves the ride on its own, so the equip notification of a mover is
 * a request here and the rule runs at the next tick, when the item handlers have updated the visual equipment: the parameters
 * record (`params.ride`) goes first so that the board finds its ride, then the act.
 */
export class RideSync {
  /** Movers whose equipment changed: the server frame of the notification and the ticks waited for the parameters. */
  readonly pending = new Map<number, { frame: number; waited: number }>();
  /** Takes off and landings sent (the probe). */
  readonly sent = { takeOff: 0, land: 0 };

  /** `DoEquipNtf` of mover `id` at server frame `frame`. */
  request(id: number, frame: number): void {
    if (id !== 0) this.pending.set(id, { frame, waited: 0 });
  }

  /** Runs the rule for the pending movers; the ones the sim cannot take yet stay pending. */
  run(ctx: ClientContext): void {
    if (this.pending.size === 0) return;
    const view = ctx.sim.view;
    if (view?.valid !== true) return;
    const params = peekWorldRuntime(ctx)?.params;
    for (const [id, p] of this.pending) {
      const rec = ctx.entities.get(id);
      if (!rec || rec.removed || !rec.simHandle || rec.kind !== EntityKind.Player || !rec.visual) {
        this.pending.delete(id);
        continue;
      }
      if (params) {
        params.mark(id);
        // not written: the bounds of the world are not loaded yet, try again in a moment
        if (!params.flushOne(id) && p.waited < RIDE_SYNC_PATIENCE) {
          p.waited += 1;
          continue;
        }
      }
      this.pending.delete(id);
      if (rec.hp <= 0) continue; // a dead mover keeps its flight state (the server's rule stops at death, too)
      const ride = rec.visual.equip.get(PARTS_RIDE) ?? null;
      const flying = (view.stateFlagsOf(id) & OBJSTAF.FLY) !== 0;
      if (ride !== null && !flying) {
        ctx.sim.cmd.act(id, p.frame, OBJMSG.MODE_FLY, [ride.itemId | 0, 0, 1, 0, 0], false);
        this.sent.takeOff += 1;
      } else if (ride === null && flying) {
        ctx.sim.cmd.act(id, p.frame, OBJMSG.MODE_GROUND, [0, 0, 1, 0, 0], false);
        this.sent.land += 1;
      }
    }
  }

  reset(): void {
    this.pending.clear();
  }
}

/** The yaw of the own mover in the render view (degrees); `null` when the view has no such mover. */
function ownAngle(ctx: ClientContext): number | null {
  const self = ctx.entities.self();
  const view = ctx.sim.view;
  if (!self || view?.valid !== true) return null;
  const a = view.angleOf(self.objId);
  return Number.isNaN(a) ? null : a;
}

const FLY_TURBO = OBJSTAF.FLY | OBJSTAF.ACC | OBJSTAF.TURBO;

/**
 * The 60 Hz tick system `flight`: while the own mover flies with ACC and TURBO the mirror burns `due` frames; the speed digits follow the
 * report of the last frame. Registered once per context (a second call is a no-op).
 */
export function installFlight(ctx: ClientContext): void {
  if (ctx.tickSystems.names.includes('flight')) return;
  const state = flightOf(ctx);
  // `window.__flyffDebug.flight.state()`: the mirror of the HUD and the ride rule's counters (the Playwright flight spec reads it)
  if (!ctx.probe.sections().includes('flight')) {
    ctx.probe.extend('flight', {
      state: () => ({
        /** The yaw (degrees) of the own mover in the render view; `null` before there is one. */
        angle: ownAngle(ctx),
        fuel: state.fuel,
        accFuel: state.accFuel,
        speed: state.speed,
        messages: state.messages,
        rides: { ...state.rides.sent, pending: state.rides.pending.size },
      }),
    });
  }
  ctx.tickSystems.register('flight', (due) => {
    state.rides.run(ctx);
    const self = ctx.entities.self();
    const view = ctx.sim.view;
    if (!self || view?.valid !== true) return;
    const sf = view.stateFlagsOf(self.objId);
    // the own state as the sim has it: the entity record keeps the state of the spawn snapshot and the self store the flight flag of
    // it, but the windows read both (an item with a ready time needs `OBJSTA_STAND`, a flying player uses none): the board is
    // taken off by wearing it, so both must follow the sim (`MoverStateNtf` goes to the sim, not to the record)
    const simState = view.stateOf(self.objId);
    if (self.state !== simState) self.state = simState;
    const flying = (sf & OBJSTAF.FLY) !== 0;
    if (ctx.stores.selfStore.getState().isFlying !== flying) {
      ctx.stores.selfStore.setState({ isFlying: flying });
    }
    if (!flying) {
      state.speed = 0;
      return;
    }
    if ((sf & FLY_TURBO) === FLY_TURBO) state.burn(due);
    const r = view.report;
    const s = REPORT.slot.delta;
    state.speed = flySpeedDigits(r[s] ?? 0, r[s + 1] ?? 0, r[s + 2] ?? 0);
  });
}
