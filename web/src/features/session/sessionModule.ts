// The `session` feature module (Plan/45 §1.6, §7.13; WP-CFEAT-08, UI mounting): the screen bodies of the states that have
// no window of their own (the title states are windows, `Reconnecting` and `Reloading` are overlays) and the logout / quit
// controller with its countdown.

import type { ClientContext } from '../../app/context';
import type { FeatureModule } from '../common/module';
import { addProbeMember } from '../common/probe';
import { LobbyBody, ReconnectingBody, ReloadingBody } from './bodies';
import { createLogoutController, type LogoutController } from './logout';

const controllers = new WeakMap<ClientContext, LogoutController>();

/** The logout / quit controller of a context (the taskbar's start menu, `APP_LOGOUT` / `APP_QUIT` call it). */
export function logoutOf(ctx: ClientContext): LogoutController | undefined {
  return controllers.get(ctx);
}

export const sessionModule: FeatureModule = {
  id: 'screens',
  milestone: 2,
  register(fc) {
    const screens = fc.app.screens;
    for (const name of [
      'Login',
      'Connecting',
      'CharSelect',
      'CharCreate',
      'Entering',
      'Loading',
      'InGame',
      'Leaving',
    ] as const) {
      screens.register(name, LobbyBody);
    }
    screens.register('Reconnecting', ReconnectingBody);
    screens.register('Reloading', ReloadingBody);

    const logout = createLogoutController(fc);
    controllers.set(fc.app, logout);
    // what OK of the Logout / Quit boxes of 42 does, and the attack-mode pre-check of the Logout applet (45 §7.13)
    fc.ui?.session.setActions({
      logout: () => {
        logout.logout();
      },
      quit: () => {
        logout.quit();
      },
      mayLogout: () => logout.mayLogout(),
    });
    addProbeMember(fc.app, 'logout', () => ({
      counting: logout.counting,
      secondsLeft: logout.secondsLeft,
    }));
    // e2e drivers of the Logout / Quit applets (the taskbar start menu of 42 calls the controller directly)
    if (fc.app.probe.test) {
      addProbeMember(fc.app, 'requestLogout', () => logout.logout());
      addProbeMember(fc.app, 'requestQuit', () => logout.quit());
      // a new character starts in a safe region (logout is immediate there): the specs of the countdown path mark the region
      // as unsafe on the client; the server's own `ReqLeave` / `ReturnToCharSelect` rules stay in force
      addProbeMember(fc.app, 'forceSafeRegion', (safe: boolean) => {
        fc.app.stores.worldStore.setState((w) => ({ region: { ...w.region, safe } }));
      });
    }
  },
};
