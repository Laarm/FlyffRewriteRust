// The panels of the lazy `help` bundle (`Client/*.inc` of the original) for the event windows: `ctx.data?.bundle( 'help' )?.panels[ key ]`, loading the bundle first when it is not in. The five lines
// of `siege/siegeModule.ts`'s `withPanels`, shared by the secret-room and tax windows (the panel keys are the lower-case stems of `crates/flyff-data/src/client/panels.rs::panel_key`).

import type { ClientContext } from '../../app/context';

/** Calls `fn` with the panels (empty when the bundle cannot be loaded). */
export function withHelpPanels(
  ctx: ClientContext,
  fn: (panels: Readonly<Record<string, string>>) => void,
): void {
  const have = ctx.data?.bundle('help');
  if (have) {
    fn(have.panels);
    return;
  }
  ctx.bundles.ensure('help').then(
    () => {
      fn(ctx.data?.bundle('help')?.panels ?? {});
    },
    () => {
      fn({});
    },
  );
}
