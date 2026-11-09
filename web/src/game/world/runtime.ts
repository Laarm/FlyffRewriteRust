// The world runtime of one `ClientContext` (WP-CLIENT-18..23): the objects the world handlers share - the `SET_PARAMS`
// manager, the pose feeder, the collision streamer, the move reporter, the loading gate, the live record of the own character -
// and the glue that connects them to the loop (tick system, sim events, the `poseFeeder` / `moveReporter` ports) and to the
// state machine (scope resets). One runtime per context, created when the handler modules register.

import type { ClientContext } from '../../app/context';
import type { ClientRegion } from '../../gen/bundles';
import { createWorldStreamer, type WorldStreamer } from '../../assets/land';
import type { AssetManager } from '../../assets/manager';
import type { ResettableScope } from '../../stores/registry';
import { EVENT_KINDS } from '../../gen/sim/wire';
import { EntityKind, type EntityRecord, type EntityRemoveReason } from '../entities';
import { createMoveReporter, type MoveReporter } from '../moveReporter';
import { createPoseFeeder, type PoseFeeder } from '../poseFeeder';
import type { SimLoad } from '../ports';
import type { SimEventRecord } from '../simEvents';
import { createEnvironment, type EnvironmentApi } from './environment';
import { describeWorld, type WorldDescription } from './describe';
import { GateController } from './gate';
import { DEFAULT_PART_FILES, MoverParamsManager, type PartFilesSource } from './params';
import { NO_REGION, regionAt, sameRegion } from './region';
import { newSelfLive, type SelfLive } from './self';

export interface WorldRuntime {
  readonly params: MoverParamsManager;
  readonly feeder: PoseFeeder;
  readonly streamer: WorldStreamer;
  readonly reporter: MoveReporter;
  readonly gate: GateController;
  readonly live: SelfLive;
  readonly environment: EnvironmentApi;
  /** The renderer's part assembly for the bounds of players (default: the body of the sex). */
  parts: PartFilesSource | null;
  /** The world the sim, the renderer and the streamer were told about (`null` before the first join). */
  readonly world: WorldDescription | null;
  /**
   * Where the own player is (LH): the last position read from the render view, else the position of `WorldReadInfo`; `null`
   * outside a world. The renderer streams terrain around it (`FrameInput.activePos`).
   */
  activePos(): { readonly x: number; readonly y: number; readonly z: number } | null;
  /** `WorldReadInfo` / `Replace`: applies the world (sim `set_world`, renderer, streamer, gate). Throws when data is missing. */
  beginWorld(worldId: number, pos: { x: number; y: number; z: number } | undefined): void;
  /** The own mover was added to the sim (`SET_ACTIVE` written): the next report sends a `BEHAVIOR` state. */
  onOwnAdded(rec: EntityRecord): void;
  onEntityRemoved(rec: EntityRecord, reason: EntityRemoveReason): void;
  /** `session` / `world` / `map` scope reset (Plan/40 section 6.5), called by the session host after the stores. */
  onScopeReset(scope: ResettableScope): void;
  /** The bridge applied or rejected a chunk / skeleton / clip load. */
  onSimLoad(load: SimLoad, error: unknown): void;
}

const runtimes = new WeakMap<ClientContext, WorldRuntime>();

/** The runtime of `ctx` if the world modules registered, else `undefined` (tests, dev surfaces without a world). */
export function peekWorldRuntime(ctx: ClientContext): WorldRuntime | undefined {
  return runtimes.get(ctx);
}

/** The runtime of `ctx`, created on first use. */
export function worldRuntime(ctx: ClientContext): WorldRuntime {
  const known = runtimes.get(ctx);
  if (known) return known;
  const rt = createWorldRuntime(ctx);
  runtimes.set(ctx, rt);
  return rt;
}

function createWorldRuntime(ctx: ClientContext): WorldRuntime {
  // `ctx.sim` and `ctx.render` are replaceable ports (the bridge arrives at boot B3, the renderer when its chunk is in)
  const simRef = {
    get cmd() {
      return ctx.sim.cmd;
    },
    get modelBoundsLoaded() {
      return ctx.sim.modelBoundsLoaded;
    },
    enqueueLoad: (load: SimLoad): void => {
      ctx.sim.enqueueLoad(load);
    },
  };
  const assetsRef: Pick<AssetManager, 'acquire'> = {
    acquire: (type, key, prio, distance, options) =>
      ctx.assets.acquire(type, key, prio, distance, options),
  };
  const live = newSelfLive();
  let parts: PartFilesSource | null = null;
  let world: WorldDescription | null = null;
  let anchor: { x: number; y: number; z: number } | null = null;
  let lastPos: { x: number; y: number; z: number } | null = null;

  const params = new MoverParamsManager({
    entities: ctx.entities,
    sim: simRef,
    data: () => ctx.data,
    parts: () => parts ?? DEFAULT_PART_FILES,
  });
  const feeder = createPoseFeeder({
    sim: simRef,
    assets: assetsRef,
    data: () => ctx.data,
    selfId: () => ctx.entities.self()?.objId ?? 0,
    log: ctx.log,
  });
  const streamer = createWorldStreamer({
    assets: assetsRef,
    sim: simRef,
    now: () => ctx.now(),
    loading: () => ctx.machine.screen.s === 'Loading',
    log: ctx.log,
  });
  const reporter = createMoveReporter({
    sender: ctx.net.sender,
    frameSkew: () => ctx.sim.frameSkew,
  });
  const gate = new GateController(ctx, streamer, feeder, () => world !== null);
  const environment = createEnvironment(ctx);

  // the loop ports (step 7: report, step 8: pose requests)
  ctx.poseFeeder = feeder;
  ctx.moveReporter = reporter;

  const regions = (): readonly ClientRegion[] | undefined =>
    world ? ctx.data?.worldFolder(world.folder)?.regions : undefined;

  const rt: WorldRuntime = {
    params,
    feeder,
    streamer,
    reporter,
    gate,
    live,
    environment,
    get parts() {
      return parts;
    },
    set parts(p: PartFilesSource | null) {
      parts = p;
      // equipment-dependent bounds of every player are recomputed with the new rule
      ctx.entities.forEach(EntityKind.Player, (r) => {
        params.mark(r.objId);
      });
    },
    get world() {
      return world;
    },
    activePos() {
      return lastPos ?? anchor;
    },
    beginWorld(worldId, pos) {
      const d = describeWorld(ctx, worldId);
      if (!d) {
        throw new Error(
          `world ${String(worldId)}: the client data (worlds bundle, asset manifest) is not loaded`,
        );
      }
      world = d;
      params.clear();
      feeder.unwatchOwn();
      reporter.reset();
      lastPos = null;
      anchor = { x: pos?.x ?? 0, y: pos?.y ?? 0, z: pos?.z ?? 0 };
      ctx.sim.setWorld(d.sim);
      ctx.render?.setWorld?.(d.render);
      ctx.stores.worldStore.setState({
        worldId,
        folder: d.folder,
        title: d.title,
        mpu: d.mpu,
        lands: [d.landsW, d.landsH],
        loading: { phase: 'lands', progress: 0 },
        region: NO_REGION,
      });
      streamer.setWorld(d.stream, anchor.x, anchor.z);
      gate.begin();
    },
    onOwnAdded() {
      reporter.armBehavior();
    },
    onEntityRemoved() {
      // nothing else holds per-entity state yet (41's mover visuals follow the entity table version)
    },
    onScopeReset(scope) {
      params.clear();
      if (scope === 'session' || scope === 'world') {
        ctx.sim.leaveWorld();
        ctx.render?.setWorld?.(null);
        streamer.setWorld(null);
        feeder.unwatchOwn();
        reporter.reset();
        world = null;
        anchor = null;
        lastPos = null;
        live.objId = 0;
      }
    },
    onSimLoad(load, error) {
      feeder.onSimLoad(load, error);
      streamer.onSimLoad(load, error);
    },
  };

  // the per-step system: window of the collision streamer around the own player, the loading gate
  ctx.tickSystems.register('world', () => {
    const now = ctx.now();
    const self = ctx.entities.self();
    const view = ctx.sim.view;
    if (self && view) {
      const out: [number, number, number] = [0, 0, 0];
      if (view.posOf(self.objId, out)) lastPos = { x: out[0], y: out[1], z: out[2] };
    }
    const p = lastPos ?? anchor;
    if (p) streamer.update(p.x, p.z, now);
    // movers whose `SET_PARAMS` waited for the `FLMB` table or the `model-bounds` bundle are written as soon as both are in (the
    // drain-end hook only runs when a batch arrives)
    if (params.pending > 0) params.flush();
    gate.tick(now);
  });

  // sim events of the loop: the router of the bridge, and `ChunkMissing` for the streamer
  ctx.loop.onSimEvents((events) => {
    ctx.sim.events.dispatch(events);
    for (const raw of events) {
      const e = raw as SimEventRecord;
      if (e.kind === EVENT_KINDS.ChunkMissing.id) streamer.chunkMissing(e.words[0], e.words[1]);
    }
  });

  // `worldStore.region` at 4 Hz from the own position (Plan/40 section 6.4)
  ctx.publisher.register('region', 4, () => {
    const regs = regions();
    const p = lastPos;
    if (!regs || !p || !world) return;
    const settings = ctx.data?.worldFolder(world.folder)?.settings;
    const next = regionAt(regs, settings?.pk_mode ?? 0, p.x, p.z);
    const cur = ctx.stores.worldStore.getState().region;
    if (!sameRegion(cur, next)) ctx.stores.worldStore.setState({ region: next });
  });

  return rt;
}
