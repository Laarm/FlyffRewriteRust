// Typed access to `ClientFeatureConfig` (Plan/45 §0.8, §1.2) and the guard of the reserved features.
//
// `features.ftool` and `features.farm_bot` are RESERVED (PL-140, PL-141, PD-04): `true` shows nothing in the client. The
// applet registry (42 WP-UI-15) and the hotkey table (43) pass their candidates through `filterReservedApplets` /
// `filterReservedHotkeys` before they register anything, so neither the F-Tool / Refresher applets nor their letter
// hotkeys (`T`, `R`) ever exist, whatever the config says. `R` as the whisper-reply key of the chat (43/45) is not an
// applet hotkey and is untouched.

import { APP_FTOOL, APP_REFRESHER } from '../../gen/defs/resdata';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';

export type { ClientFeatureConfig };

/** The applets that are never registered (the F-Tool and the farm-bot "Refresher"), by `APP_*` id. */
export const RESERVED_APPLETS: ReadonlySet<number> = new Set([APP_FTOOL, APP_REFRESHER]);

/** The applet letter hotkeys of the reserved applets (`KeyboardEvent.code`; 42 §12.1: `T` = F-Tool, `R` = Refresher). */
export const RESERVED_APPLET_HOTKEYS: ReadonlyMap<number, string> = new Map([
  [APP_FTOOL, 'KeyT'],
  [APP_REFRESHER, 'KeyR'],
]);

/**
 * `true` for an applet that must not be registered. Independent of the config values on purpose: with `ftool` /
 * `farm_bot` set to `true` the client still builds nothing (the server warns at start-up, doc 10).
 */
export function isReservedApplet(app: number): boolean {
  return RESERVED_APPLETS.has(app);
}

/** `true` for an applet hotkey binding (`app` -> letter `code`) that must not exist. */
export function isReservedAppletHotkey(app: number, code: string): boolean {
  return RESERVED_APPLET_HOTKEYS.get(app) === code;
}

/** The candidates of the applet registry without the reserved applets. */
export function filterReservedApplets<T extends { readonly app: number }>(
  entries: readonly T[],
): T[] {
  return entries.filter((e) => !isReservedApplet(e.app));
}

/** The candidates of the hotkey table without the reserved applet bindings (`{ app, code }`). */
export function filterReservedHotkeys<T extends { readonly app: number; readonly code: string }>(
  entries: readonly T[],
): T[] {
  return entries.filter((e) => !isReservedApplet(e.app) && !isReservedAppletHotkey(e.app, e.code));
}

/** A boolean key of `[features]` (the `features.*` table of 03 §4). */
export type FeatureKey = {
  [K in keyof ClientFeatureConfig['features']]: ClientFeatureConfig['features'][K] extends boolean
    ? K
    : never;
}[keyof ClientFeatureConfig['features']];

/** `features.<key>`; both branches of every flag are implemented by the windows that read it. */
export function feature(config: ClientFeatureConfig, key: FeatureKey): boolean {
  return config.features[key];
}
