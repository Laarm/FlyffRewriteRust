// The host interface of the UI (Plan/42 §2.3): everything the UI needs from the rest of the client, injected by the
// adapter `uiHost(ctx)` of `app/hosts.ts` (WP-CLIENT-15). Fields that belong to later UI work packages are added
// to this interface by those packages (options, rules, overlays, previews, items/skills/texts APIs).

import type { Logger } from '../platform/log';
import type { ResdataBundle, UiAtlasesBundle } from '../gen/bundles';
import type { ClientFeatureConfig } from '../gen/config/clientFeatures';
import type { UiOptionsSource } from './options';
import type { ImageDecoder, UiAssetUrls } from './skin/images';

/** The texts of the `texts` bundle (45's `TextApi`): `getText(tid)`. */
export interface UiTexts {
  text(tid: number): string;
}

/** The hooks of the standard dialogs (Plan/42 §14): what the original did with `WM_CLOSE` / `WM_LOGOUT`. */
export interface UiDialogHooks {
  /** The `TID_DIAG_0035` message box was confirmed: back to the login state (the original exited the client). */
  returnToLogin?(): void;
  /** An upper message box with `postLogout` was confirmed (`WM_LOGOUT`). */
  logout?(): void;
}

export interface UiHost {
  /** The parsed `resdata-layouts` and `ui-atlases` bundles (`BundleEnvelope.data`); read lazily, through getters. */
  readonly bundles: {
    /** `undefined` until the (public, pre-login) bundle is loaded: the registry is empty until then. */
    readonly layouts: ResdataBundle | undefined;
    readonly atlases: UiAtlasesBundle | undefined;
    /** `getText(tid)` for captions, tooltips and the standard dialogs (English literals where it is missing). */
    readonly texts?: UiTexts;
  };
  /** URL resolution of the UI images (24 §6.5): `assetUrl` (exact key) and `nibbleUrl` (skin key). */
  readonly assets: UiAssetUrls;
  /** `performance.now()` in ms (a fake clock in tests). */
  now(): number;
  /** Image decoder; the browser decoder (`createBrowserDecoder`) when omitted. */
  readonly decode?: ImageDecoder;
  /** `ClientFeatureConfig` (03 §4). */
  readonly features?: ClientFeatureConfig;
  /** `SND_INF_CLICK`, the only UI sound (44). */
  readonly sound?: { click(): void };
  /** Lower-case account name for the §17 storage key. */
  account?(): string | null;
  /** Authority >= GAMEMASTER. */
  isGm?(): boolean;
  /** Logger (a quiet default when omitted). */
  readonly log?: Logger;
  /** Session hooks of the standard dialogs. */
  readonly dialogs?: UiDialogHooks;
  /** The options of 45's store that the UI reads (`uiScale`, `windowAlpha`, `gamma`, ...); the defaults apply when omitted. */
  readonly options?: UiOptionsSource;
  /** `selfStore.idPlayer` of the logged-in character (the `regInfo` key, §17); `null` before login. */
  playerId?(): number | null;
}
