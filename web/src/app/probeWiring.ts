// Wires the debug probe (Plan/40 section 13.2) to a `ClientContext`: the data providers of every section
// that exists, and the test hooks whose behaviour this layer owns (`sentLog`, `enterWorld`, `back`,
// `simulateHidden`). Other WPs wire their hooks with `ctx.probe.setTestHook` and attach their sections with
// `ctx.probe.extend`.

import {
  createProbe,
  type FlyffDebugProbe,
  type ProbeEntity,
  type ProbeEntityKind,
  type ProbeSelf,
  type ProbeSession,
} from '../debug/probe';
import { EntityKind, type EntityRecord } from '../game/entities';
import { enterWorld, quit } from '../game/session/actions';
import { peekWorldRuntime } from '../game/world/runtime';
import { callStats } from '../wasm/call';
import { createAuthActions } from './authActions';
import type { ClientContext } from './context';
import { installQaHooks } from './qaHooks';

const KIND_NAMES: Readonly<Record<number, ProbeEntityKind | 'pet'>> = {
  [EntityKind.Player]: 'player',
  [EntityKind.Npc]: 'npc',
  [EntityKind.Item]: 'item',
  [EntityKind.Ctrl]: 'ctrl',
  [EntityKind.LocalPet]: 'pet',
};

const KIND_BY_NAME: Readonly<Record<ProbeEntityKind, EntityKind>> = {
  player: EntityKind.Player,
  npc: EntityKind.Npc,
  item: EntityKind.Item,
  ctrl: EntityKind.Ctrl,
};

function entityName(rec: EntityRecord): string {
  if (rec.visual) return rec.visual.name;
  const spawn = rec.spawn;
  if ('player' in spawn && spawn.player) return spawn.player.name;
  return '';
}

function entityPos(rec: EntityRecord): readonly [number, number, number] {
  const spawn = rec.spawn;
  const p = 'obj' in spawn ? spawn.obj?.pos : undefined;
  return [p?.x ?? 0, p?.y ?? 0, p?.z ?? 0];
}

export function createContextProbe(ctx: ClientContext, options: { e2e: boolean }): FlyffDebugProbe {
  const probe = createProbe(
    {
      build: () => ({
        id: ctx.build.id,
        protoHash: ctx.build.protoHash,
        dataVersion: ctx.dataVersion,
      }),
      state: () => ctx.machine.screen.s,
      stateHistory: () => ctx.machine.history(),
      session: () => {
        const session = ctx.stores.sessionStore.getState();
        const self = ctx.stores.selfStore.getState();
        const world = ctx.stores.worldStore.getState();
        const out: { -readonly [K in keyof ProbeSession]: ProbeSession[K] } = {
          reattached: session.reattached,
          authority: session.authority,
          characters: ctx.machine.ctx.chars.map((c) => ({
            slot: c.slot,
            idPlayer: c.idPlayer,
            name: c.name,
          })),
        };
        if (session.account !== null) out.account = session.account;
        if (self.idPlayer !== 0) out.idPlayer = self.idPlayer;
        if (world.worldId !== 0) out.worldId = world.worldId;
        return out;
      },
      self: (): ProbeSelf | null => {
        const s = ctx.stores.selfStore.getState();
        if (s.objId === 0) return null;
        const rec = ctx.entities.get(s.objId);
        return {
          objId: s.objId,
          name: s.name,
          level: s.level,
          hp: rec?.hp ?? s.hp,
          maxHp: s.maxHp,
          pos: s.pos,
          state: rec?.state ?? 0,
        };
      },
      entities: (kind) => {
        const out: ProbeEntity[] = [];
        // the position of a mover the sim knows is read from the render view (LH, the logical position of the last step;
        // `drawnPos` is where the frame draws it, interpolated with alpha); the spawn position is the fallback (items, a view
        // that is not valid yet)
        const view = ctx.sim.view;
        const live = view?.valid === true ? view : null;
        const at: [number, number, number] = [0, 0, 0];
        const drawn: [number, number, number] = [0, 0, 0];
        ctx.entities.forEach(kind ? KIND_BY_NAME[kind] : 0, (rec) => {
          const entity: { -readonly [K in keyof ProbeEntity]: ProbeEntity[K] } = {
            objId: rec.objId,
            kind: KIND_NAMES[rec.kind] ?? 'unknown',
            name: entityName(rec),
            pos: entityPos(rec),
          };
          if (rec.visual?.activeAttack === true) entity.activeAttack = true;
          if (live?.posOf(rec.objId, at) === true) {
            entity.pos = [at[0], at[1], at[2]];
            entity.renderPos = [at[0], at[1], at[2]];
            if (live.drawnPosOf(rec.objId, drawn)) entity.drawnPos = [drawn[0], drawn[1], drawn[2]];
          }
          out.push(entity);
        });
        return out;
      },
      net: () => {
        const reg = ctx.net.registry.stats;
        return {
          connected: ctx.net.transport?.isOpen ?? false,
          rttMs: ctx.net.clock.rttMs,
          offsetFrames: ctx.net.clock.offset,
          batches: reg.batches,
          sent: ctx.net.sender.stats.sent,
          unknown: reg.unknown,
          unhandled: reg.unhandled,
          handlerErrors: reg.errors,
        };
      },
      loop: () => {
        const s = ctx.loop.stats;
        const w = callStats();
        return {
          simFrame: s.simFrame,
          clientFrame: s.clientFrame,
          frameSkew: s.frameSkew,
          stepsPerSec: s.stepsPerSec,
          fps: s.fps,
          droppedSteps: s.droppedSteps,
          hidden: s.hidden,
          lastFrameMs: s.lastFrameMs,
          alpha: s.alpha,
          wasmCallsMax: w.maxCalls,
          wasmCallsByCategoryMax: w.maxByCategory,
          wasmOverBudget: w.overBudget,
          wasmUiCallsMax: w.maxUiCalls,
          frameTimes: (n) => ctx.loop.frameTimes.last(n),
        };
      },
      assets: () => {
        const a = ctx.assets.stats;
        // lands the renderer holds (`render.landsResident`, once the world renderer is up) and collision chunks in the sim
        const rendered = (probe as unknown as { render?: { landsResident?: unknown } }).render;
        const lands = typeof rendered?.landsResident === 'number' ? rendered.landsResident : 0;
        return {
          inflight: a.inflight,
          queued: a.queued,
          failed: [...a.failed],
          cpuBytes: a.cpuBytes,
          gpuBytes: a.gpuBytes,
          landsReady: lands,
          chunksReady: peekWorldRuntime(ctx)?.streamer.stats.chunksReady ?? 0,
        };
      },
      recovery: () => ctx.recovery.history,
      logs: (n) => ctx.log.lines(n),
    },
    { testHooks: options.e2e },
  );

  // `__flyffDebug.data`: the bundle loader (names, sizes, fetch+parse times) for the perf specs and Playwright
  probe.extend('data', {
    dataVersion: () => ctx.bundles.dataVersion,
    gated: () => ctx.bundles.gated,
    loaded: () => [...ctx.bundles.loaded].sort(),
    stats: () => Object.fromEntries(ctx.bundles.stats),
    textCount: () => ctx.data?.bundle('texts')?.size ?? 0,
  });
  probe.setTestHook('sentLog', (n) => ctx.net.sender.recent(n));
  probe.setTestHook('simulateHidden', (on) => {
    ctx.background.simulateHidden(on);
  });
  probe.setTestHook('enterWorld', (slot) => {
    const c = ctx.machine.ctx.chars.find((x) => x.slot === slot);
    if (!c) throw new Error(`no character in slot ${String(slot)}`);
    enterWorld(ctx, slot, c.idPlayer);
  });
  probe.setTestHook('back', async () => {
    await quit(ctx);
  });
  probe.setTestHook('forceRecovery', (level) => {
    if (level === 2) ctx.recovery.l2('forced by the test hook');
    else ctx.recovery.l3('forced by the test hook');
  });
  // `test.forceInput(UP, 3000)`: the held input bits of Plan/31 section 17.5 for `ms`, fed to the sim as the local input record
  // (43's keyboard capture replaces the record while keys are down; WP-INPUT-05 may rewire this hook)
  probe.setTestHook('forceInput', (buttons, ms) => {
    ctx.sim.forceInput(buttons, ms);
  });
  probe.setTestHook('dropSocket', () => {
    ctx.connection.simulateDrop();
  });
  // `login` resolves when character select shows the characters (`CharList` arrived), rejects with the notice
  // of a refused login (103 without `force` is refused as well: the hook never asks a question)
  probe.setTestHook('login', async (account, password, force = false) => {
    const actions = createAuthActions(ctx);
    const outcome = await actions.login(account, password, { force });
    if (outcome.kind === 'takeover')
      throw new Error('login refused: the account is already connected (103)');
    if (outcome.kind === 'failed') {
      throw new Error(
        `login refused: ${outcome.notice.text ?? `message ${String(outcome.notice.tid)}`}`,
      );
    }
    await new Promise<void>((resolve, reject) => {
      const done = (screen: ClientContext['machine']['screen']): boolean => {
        if (screen.s === 'CharSelect' && !screen.busy) {
          resolve();
          return true;
        }
        if (screen.s === 'Login' || screen.s === 'Fatal' || screen.s === 'Reloading') {
          reject(new Error(`login ended in ${screen.s}`));
          return true;
        }
        return false;
      };
      if (done(ctx.machine.screen)) return;
      const off = ctx.machine.subscribe((screen) => {
        if (done(screen)) off();
      });
    });
  });
  // WP-REND-17: setCamera / cameraPath / setWeather / hideHud and the `qa` section (nothing without test hooks)
  installQaHooks(ctx, probe);
  return probe;
}
