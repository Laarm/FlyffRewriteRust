// The effect port of the screen state machine (Plan/40 section 4.2, `SessionHost`): the connection module owns the
// socket effects, the recovery ladder the reload ones; what is left (publishers, input, asset generations, the gated
// bundles) is wired here. `boot` and the dev surfaces that talk to a server call `installSessionHost(ctx)`;
// a test context installs it only when it wants the real sockets (`createTestContext().install()`).

import { peekWorldRuntime } from '../game/world/runtime';
import type { ClientContext } from './context';
import type { SessionHost } from './stateMachine';

export function installSessionHost(ctx: ClientContext, extra: Partial<SessionHost> = {}): void {
  const host = ctx.sessionHost;
  ctx.connection.install(host);
  host.reloadOnce = (reason) => {
    ctx.recovery.reloadOnce(reason);
  };
  host.reloadPage = (reason) => {
    ctx.recovery.performReload(reason);
  };
  host.releaseInput = () => {
    ctx.input.release();
  };
  host.flushPublishers = () => {
    ctx.publisher.flushAll();
  };
  host.fetchGatedBundles = () => {
    // the asset cookie exists now: load the boot set of a gated server (Plan/40 section 3.4)
    ctx.bundles.onLoggedIn().catch((error: unknown) => {
      const text = error instanceof Error ? error.message : String(error);
      ctx.log.error('boot', `gated bundles failed: ${text}`);
      ctx.recovery.l3(`boot: ${text}`);
    });
  };
  host.scopeReset = (scope) => {
    // a world, map or session reset: older queued asset requests are cancelled, older in-flight results dropped
    ctx.assets.bumpGeneration();
    // the sim leaves the world (session, world), the streamer and the renderer drop theirs (game/world/runtime.ts)
    peekWorldRuntime(ctx)?.onScopeReset(scope);
  };
  Object.assign(host, extra);
}
