// The DOM face of the UI over the game (Plan/40 §2.1, 42 §4.8; `ui/NOTES.md` "Mounting"): `UiRoot` above the 3-D canvas. It
// is transparent to the pointer; its document listeners feed the window manager. The title states (login, character
// select/create) paint the wallpaper, force `WindowAlpha` 255 and take the keyboard (`keys`: Enter, Space, Tab of the
// `WBS_KEY` frames); in game 43's key router feeds the manager instead.

import type { ClientContext } from '../../app/context';
import { useStoreSel } from '../../stores/framework';
import type { ScreenName } from '../../stores/screen';
import { UiRoot, WorldMapLayer } from '../../ui';
import { fullUi } from '../common/module';

/** The states in which the title wallpaper shows and the UI owns the keyboard. */
export const TITLE_SCREENS: ReadonlySet<ScreenName> = new Set<ScreenName>([
  'Login',
  'Connecting',
  'CharSelect',
  'CharCreate',
  'Entering',
]);

/** The states without a UI layer (the boot screen, the fatal screen and the reload line are plain HTML). */
const NO_UI: ReadonlySet<ScreenName> = new Set<ScreenName>(['Boot', 'Fatal', 'Reloading']);

export function UiLayer({ ctx }: { ctx: ClientContext }) {
  const screen = useStoreSel(ctx.stores.sessionStore, (s) => s.screen.s);
  const ui = fullUi(ctx);
  if (!ui || NO_UI.has(screen)) return null;
  const title = TITLE_SCREENS.has(screen);
  return (
    <UiRoot
      wm={ui.wm}
      scaler={ui.scaler}
      options={ui.options}
      ramp={ui.ramp}
      tooltip={{ service: ui.tooltip, images: ui.images }}
      cursor={ui.cursor}
      title={title}
      keys={title}
      underlay={title ? undefined : <WorldMapLayer view={ui.nav.worldMap} env={ui.wm.env} />}
    />
  );
}
