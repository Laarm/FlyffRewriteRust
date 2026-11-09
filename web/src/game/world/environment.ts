// `GameTimer`, `Environment`, `EnvironmentAll` (Plan/40 section 5.6, 6.2.3; Plan/15 section 17; ref/09a section 5.9, 6.8):
// the in-game clock sample the renderer consumes once per drawn frame, the season and weather of `worldStore.env`, and the 1 Hz
// `world.index` publisher.
//
// `GameTimer.current_time_ms` arrives in the join batch only; the client advances it with the server-time estimate:
// `ms = baseMs + (serverUnixMsNow - receivedAtServerMs)`, then (`CGameTimer::Compute`, `_Common/timer.cpp:167-200`, TIMESPEED 10)
// `s = floor(ms / 10)`, `sec = s % 60`, `min = floor(s / 60) % 60`, `hour = floor(s / 3600) % 24 + 1`,
// `day = floor(s / 86400) % 30 + 1`: one real second is 100 game seconds, one game hour 36 real seconds. The guild-war world
// forces 19:00:00 (`DPClient.cpp:2425-2436`).

import type { ClientContext } from '../../app/context';
import { WI_WORLD_GUILDWAR } from '../../gen/defs/world';
import { Phase } from '../../net/registry';
import type { WorldState } from '../../stores/world';
import { EntityKind } from '../entities';

const OWNER = 'world.environment';

/** The game clock as the original keeps it (Plan/41 section 8.7): hour 1..24, minute, second, day 1..30. */
export interface GameClockSample {
  readonly hour: number;
  readonly min: number;
  readonly sec: number;
  readonly day: number;
}

/** `TIMESPEED` of `CGameTimer`. */
export const TIMESPEED = 10;

/** The sample of a game time in real milliseconds (`CGameTimer::Compute`, 64-bit arithmetic). */
export function clockSampleAt(ms: number): GameClockSample {
  const s = Math.floor(Math.max(0, ms) / TIMESPEED);
  return {
    sec: s % 60,
    min: Math.floor(s / 60) % 60,
    hour: (Math.floor(s / 3600) % 24) + 1,
    day: (Math.floor(s / 86400) % 30) + 1,
  };
}

/** The 19:00:00 sky of the guild-war world. */
export const GUILD_WAR_CLOCK: GameClockSample = { hour: 19, min: 0, sec: 0, day: 1 };

/** The sample for the current server time: `world.gameClock` advanced by the estimate of the server clock. */
export function gameClockNow(world: WorldState, serverUnixMsNow: number): GameClockSample {
  if (world.worldId === WI_WORLD_GUILDWAR) return GUILD_WAR_CLOCK;
  return clockSampleAt(
    world.gameClock.baseMs + (serverUnixMsNow - world.gameClock.receivedAtServerMs),
  );
}

export interface EnvironmentApi {
  /** The `GameClockSample` for the frame being drawn (cheap: one store read and a few integer divisions). */
  sample(): GameClockSample;
}

export function createEnvironment(ctx: ClientContext): EnvironmentApi {
  // `?e2e=1` pages: `__flyffDebug.test.setGameClock(hour, min)` pins the day cycle (visual QA of the sky at fixed hours)
  let pinned: GameClockSample | null = null;
  ctx.probe.setTestHook('setGameClock', (hour, min = 0) => {
    pinned =
      hour === null
        ? null
        : {
            hour: Math.min(24, Math.max(1, Math.trunc(hour))),
            min: Math.min(59, Math.max(0, Math.trunc(min))),
            sec: 0,
            day: 1,
          };
  });
  // `setGameHour(h)` (WP-REND-17) is the same pin at hour:00:00; `null` releases it
  ctx.probe.setTestHook('setGameHour', (hour) => {
    pinned =
      hour === null
        ? null
        : { hour: Math.min(24, Math.max(1, Math.trunc(hour))), min: 0, sec: 0, day: 1 };
  });
  return {
    sample: () =>
      pinned ??
      gameClockNow(ctx.stores.worldStore.getState(), ctx.net.clock.serverUnixMsNow(ctx.now())),
  };
}

export function registerEnvironmentHandlers(ctx: ClientContext): void {
  const { registry } = ctx.net;
  const { worldStore } = ctx.stores;

  registry.on('gameTimer', Phase.Core, OWNER, (_objId, m) => {
    worldStore.setState({
      gameClock: {
        baseMs: m.currentTimeMs,
        receivedAtServerMs: ctx.net.clock.serverUnixMsNow(ctx.now()),
      },
    });
  });

  // `effect` is the weather (rain or snow by season); the event title text belongs to 45's `Environment` handler
  registry.on('environment', Phase.Core, OWNER, (_objId, m) => {
    const env = worldStore.getState().env;
    const weather = m.effect ? 1 : 0;
    if (env.weather !== weather) worldStore.setState({ env: { ...env, weather } });
  });

  registry.on('environmentAll', Phase.Core, OWNER, (_objId, m) => {
    const env = worldStore.getState().env;
    if (env.season !== m.season) worldStore.setState({ env: { ...env, season: m.season } });
  });

  // entity counts at 1 Hz (`world.index`, Plan/40 section 6.4)
  ctx.publisher.register('world.index', 1, () => {
    const index = worldStore.getState().index;
    const players = ctx.entities.count(EntityKind.Player);
    const npcs = ctx.entities.count(EntityKind.Npc);
    const items = ctx.entities.count(EntityKind.Item);
    const version = ctx.entities.version;
    if (
      index.version !== version ||
      index.players !== players ||
      index.npcs !== npcs ||
      index.items !== items
    ) {
      worldStore.setState({ index: { version, players, npcs, items } });
    }
  });
}
