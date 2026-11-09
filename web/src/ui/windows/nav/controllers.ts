// The navigator and teleporter windows of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-36). The four applets
// are plain imports like the other applets (the service builds them synchronously, so a dynamic import here would only warn).
import type { WndCtor } from '../../catalog';
import { WndCollecting } from './WndCollecting';
import { WndNavigator } from './WndNavigator';
import { WndQueryEquip } from './WndQueryEquip';
import { WndTele16 } from './WndTele16';

export const NAV_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_NAVIGATOR: () => Promise.resolve(WndNavigator),
  APP_TELE16: () => Promise.resolve(WndTele16),
  APP_QUERYEQUIP: () => Promise.resolve(WndQueryEquip),
  APP_COLLECTING: () => Promise.resolve(WndCollecting),
};
