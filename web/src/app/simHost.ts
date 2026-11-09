// The production `SimBridge` of a context (Plan/40 section 7.2, WP-CLIENT-18; boot step B3): the `SimWorld` of the main wasm
// instance is created from the typed client features (`SimFlags` bits, the hit-attribute rule, the remote extrapolation cap),
// wrapped by the bridge and installed as `ctx.sim`. The boundary-call hooks (`wasm/call.ts`) are wired here too: a trap is a
// level-3 recovery (the instance is corrupt), a rejected command buffer a level-2 resync (the bridge drops the buffer), the
// other recoverable errors and over-budget iterations are logged.

import { createSimBridge, type SimBridgeImpl, type SimWorldFull } from '../game/simBridge';
import { peekWorldRuntime } from '../game/world/runtime';
import type { ClientFeatureConfig } from '../gen/config/clientFeatures';
import { setWasmCallHooks } from '../wasm/call';
import { createSimWorld, simMemory } from '../wasm/simFactory';
import type { ClientContext } from './context';

/** `wire::FLAG_*` of `flyff-sim` (the `SimWorld::new` word 0). */
const FLAG_BETTER_MOVEMENT = 1;
const FLAG_FIX_RANGER_ROLLER = 2;
const FLAG_ATTACK_RANGE_CODE = 4;
const FLAG_SPEED_CAST = 8;
const FLAG_IMPROVED_PETS = 16;

/** `[SimFlags bits, hit_attr_rule (0 legacy, 1 window), seed, remote_extrap_cap_frames]` (Plan/31 section 17.3 `SimWorld::new`). */
export function simConfigWords(config: ClientFeatureConfig, seed: number): number[] {
  const f = config.features;
  let bits = 0;
  if (f.better_movement) bits |= FLAG_BETTER_MOVEMENT;
  if (f.fix_ranger_roller) bits |= FLAG_FIX_RANGER_ROLLER;
  if (f.attack_range_code) bits |= FLAG_ATTACK_RANGE_CODE;
  if (f.speed_cast) bits |= FLAG_SPEED_CAST;
  if (f.improved_pets) bits |= FLAG_IMPROVED_PETS;
  return [
    bits,
    config.fidelity.hit_attr_rule === 'window' ? 1 : 0,
    seed >>> 0,
    config.fidelity.remote_extrap_cap_frames,
  ];
}

/** The seed of the cosmetic RNG of this page (the sim of the server is seeded by the server). */
function pageSeed(ctx: ClientContext): number {
  return Math.floor(ctx.now() * 1000) >>> 0 || 7;
}

/**
 * Creates the bridge over a new `SimWorld` and installs it as `ctx.sim` (the wasm module must be loaded, `ensureWasm`).
 * `ctx.features` must already hold the features of the bootstrap.
 */
export function installGameSim(ctx: ClientContext): SimBridgeImpl {
  const words = simConfigWords(ctx.features, pageSeed(ctx));
  const sim = createSimWorld(words) as SimWorldFull;
  const bridge = createSimBridge({
    sim,
    memory: simMemory,
    clock: ctx.net.clock,
    now: () => ctx.now(),
    log: ctx.log,
    onCommandError: (error) => {
      // a protocol or sim mismatch: the bridge dropped the buffer, the world is rebuilt from the server (L2)
      ctx.log.error('sim', `command buffer rejected: ${error.message}`);
      ctx.recovery.l2(`sim rejected a command buffer: ${error.message}`);
    },
    onLoad: (load, error) => {
      peekWorldRuntime(ctx)?.onSimLoad(load, error);
    },
  });
  setWasmCallHooks({
    onTrap: (error) => {
      ctx.log.error('sim', error.message);
      ctx.recovery.l3(`wasm trap: ${error.message}`);
    },
    onRecoverable: (error) => {
      ctx.log.warn('sim', error.message);
    },
    onOverBudget: (info) => {
      ctx.log.warn(
        'sim',
        `wasm call budget exceeded: ${String(info.calls)} calls ${JSON.stringify(info.byCategory)}`,
      );
    },
  });
  ctx.sim = bridge;
  ctx.log.info(
    'sim',
    `SimWorld created (flags ${String(words[0])}, extrapolation ${String(words[3])})`,
  );
  return bridge;
}
