// The pocket window of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-25): `APP_BAG_EX` is an applet and stays a plain class (the applet registry
// builds it synchronously).
import type { WndCtor } from '../../catalog';
import { WndBagEx } from './WndBagEx';

export const POCKET_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_BAG_EX: () => Promise.resolve(WndBagEx),
};
