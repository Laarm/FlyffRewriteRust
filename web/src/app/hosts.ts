// Host adapters (Plan/40 section 2.4, WP-CLIENT-15): the UI framework (42) and the feature modules (45) never see
// `ClientContext`; they receive narrow host objects built from it. The contracts below are the interfaces of 42
// section 2.3 (`UiHost`), 45 section 2.2 (`FeatureContext`) and 44 section 2.2 (`AudioDeps`), declared here
// STRUCTURALLY because `ui/`, `features/` and `audio/` are written by other work packages: when those modules exist,
// their interfaces must be assignable from these (`uiHost(ctx) satisfies UiHost`), which the adapters' tests assert for
// the contract types of this file already. Everything that may appear later is a GETTER (`rules` is `null` before
// login), so a host built early sees late construction.

import type { AssetManager } from '../assets';
import type { AudioApi, UiApi } from '../game/ports';
import type { EntityTable } from '../game/entities';
import type { ClientFeatureConfig } from '../gen/config/clientFeatures';
import type { BundleDataMap } from '../gen/bundles';
import type { ClientDataIndex, TextApi } from '../data';
import { Phase, type Case, type Value } from '../net/registry';
import type { ClientMsg } from '../net/codec';
import type { SendOptions, SendResult } from '../net/sender';
import type { Logger } from '../platform/log';
import { onVisibility, type Unsubscribe } from '../platform/visibility';
import type { ScreenState } from '../stores/screen';
import type { ClientContext } from './context';
import { questOf } from '../features/quest/api';
import { inputProvidersOf } from './inputProviders';

/** The authority letter of the first game-master level (`AUTH_GAMEMASTER` = `'L'`, Plan/23 section 1.1). */
export const AUTH_GAMEMASTER = 76;

/** A `TextApi` for the time before the `texts` bundle is in: everything is out of range (`"error"`). */
export const NULL_TEXTS: TextApi = {
  text: () => 'error',
  textColor: () => 0xffffffff,
};

// ---- 42: UiHost ---------------------------------------------------------------------------------------------------

/** 42's `UiOptionsStore` view (the options of 45 that the UI reads). */
export interface UiOptionsPort {
  get(): Readonly<Record<string, unknown>>;
  subscribe(fn: (options: Readonly<Record<string, unknown>>) => void): Unsubscribe;
}

/** The parts of the host that other documents provide (41 previews and overlays, 44 click, 45 options). */
export interface UiHostParts {
  readonly options?: UiOptionsPort;
  readonly sound?: { click(): void };
  readonly overlays?: unknown;
  readonly previews?: unknown;
  /** The session hooks of 42's standard dialogs (`TID_DIAG_0035` -> login state, `postLogout` -> logout). */
  readonly dialogs?: { returnToLogin?(): void; logout?(): void };
  /** The image decoder of the UI (42 §6.1); the browser's PNG decoder when omitted (`features/session/uiMount.ts` adds the DDS path). */
  readonly decode?: UiImageDecoder;
  /**
   * The id of the last in-world character, for the window-rect key after the self store was reset (quit, kick, logout:
   * `toLogin` / `charList` clear it before the screen listeners run; BFIX1 #021). Used only when the self store has no id.
   */
  readonly fallbackPlayerId?: () => number | null;
}

/** Decodes the image behind a URL to straight-alpha RGBA (`ui/skin/images.ts` `ImageDecoder`); `null` for a missing file. */
export type UiImageDecoder = (url: string) => Promise<{
  readonly width: number;
  readonly height: number;
  readonly data: Uint8ClampedArray;
  readonly hasAlpha: boolean;
} | null>;

export interface UiHostContract {
  readonly bundles: {
    /** `resdata-layouts`, `ui-atlases`: `undefined` until loaded. */
    readonly layouts: BundleDataMap['resdata-layouts'] | undefined;
    readonly atlases: BundleDataMap['ui-atlases'] | undefined;
    readonly texts: TextApi;
    readonly items: ClientDataIndex['item'];
    readonly skills: ClientDataIndex['skill'];
  };
  readonly assets: {
    assetUrl(vfsPath: string): string | null;
    nibbleUrl(vfsPath: string): string | null;
    cursorUrl(stem: string): string | null;
    cursorHotspot(stem: string): [number, number] | null;
  };
  readonly features: ClientFeatureConfig;
  readonly log: Logger;
  readonly options: UiOptionsPort;
  /** `RulesContext`, `null` before login. */
  readonly rules: object | null;
  /** The session hooks of the standard dialogs (42 §14). */
  readonly dialogs?: { returnToLogin?(): void; logout?(): void };
  /** The image decoder of the UI; omitted = the browser's PNG decoder. */
  readonly decode?: UiImageDecoder;
  /** `selfStore.idPlayer` of the logged-in character (the window-rect key, 42 §17); `null` before login. */
  playerId(): number | null;
  readonly sound: { click(): void };
  readonly overlays: unknown;
  readonly previews: unknown;
  account(): string | null;
  isGm(): boolean;
  /** `performance.now()` ms. */
  now(): number;
}

const NULL_OPTIONS: UiOptionsPort = { get: () => ({}), subscribe: () => () => undefined };

/** Builds 42's `UiHost` from the context (Plan/40 section 2.4). */
export function uiHost(ctx: ClientContext, parts: UiHostParts = {}): UiHostContract {
  const source = () => ctx.assetLayer.source;
  return {
    bundles: {
      get layouts() {
        return ctx.bundles.get('resdata-layouts');
      },
      get atlases() {
        return ctx.bundles.get('ui-atlases');
      },
      get texts() {
        return ctx.data ?? NULL_TEXTS;
      },
      items: (id) => ctx.data?.item(id),
      skills: (id) => ctx.data?.skill(id),
    },
    assets: {
      assetUrl: (path) => source().url({ ns: 'res', path }),
      nibbleUrl: (path) => source().url({ ns: 'gen', kind: 'png-nk', path }),
      cursorUrl: (stem) => source().url({ ns: 'gen', kind: 'cursor', stem }),
      cursorHotspot: (stem) => source().cursorHotspot(stem),
    },
    get features() {
      return ctx.features;
    },
    log: ctx.log,
    options: parts.options ?? NULL_OPTIONS,
    get rules() {
      return ctx.rules;
    },
    sound: parts.sound ?? {
      click: () => {
        // 44 plays SND_INF_CLICK through `ctx.audio.cues` once the engine exists
        (ctx.audio as { cues?: { click?: () => void } }).cues?.click?.();
      },
    },
    ...(parts.dialogs ? { dialogs: parts.dialogs } : {}),
    ...(parts.decode ? { decode: parts.decode } : {}),
    playerId: () => {
      const id = ctx.stores.selfStore.getState().idPlayer;
      return id !== 0 ? id : (parts.fallbackPlayerId?.() ?? null);
    },
    overlays: parts.overlays ?? null,
    previews: parts.previews ?? null,
    account: () => ctx.stores.sessionStore.getState().account,
    isGm: () => ctx.stores.sessionStore.getState().authority >= AUTH_GAMEMASTER,
    now: () => ctx.now(),
  };
}

// ---- 45: FeatureContext -------------------------------------------------------------------------------------------

export interface FeatureContextContract {
  readonly net: {
    /** Registers a handler in phase `Feature`: `(message, objId)` after the `Core` and `Sim` handlers ran. */
    on<K extends Case>(c: K, handler: (message: Value<K>, objId: number) => void): void;
    /** One `ClientEnvelope`; `'not_connected'` (and a counted drop) while there is no session. */
    send(msg: ClientMsg, options?: SendOptions): SendResult;
  };
  /** 42's UI API; `null` until `createUi`. */
  readonly ui: UiApi | null;
  readonly texts: TextApi;
  readonly data: ClientDataIndex | null;
  readonly rules: object | null;
  readonly lobbyRules: null;
  readonly features: ClientFeatureConfig;
  readonly audio: AudioApi;
  readonly entities: EntityTable;
}

/** Builds 45's `FeatureContext` for the feature `featureId` (Plan/40 section 2.4). */
export function featureContext(ctx: ClientContext, featureId: string): FeatureContextContract {
  return {
    net: {
      on: (c, handler) => {
        ctx.net.registry.on(c, Phase.Feature, featureId, (objId, value) => {
          handler(value, objId);
        });
      },
      send: (msg, options) => ctx.net.sender.send(msg, options),
    },
    get ui() {
      return ctx.ui;
    },
    get texts() {
      return ctx.data ?? NULL_TEXTS;
    },
    get data() {
      return ctx.data;
    },
    get rules() {
      return ctx.rules;
    },
    lobbyRules: null,
    get features() {
      return ctx.features;
    },
    get audio() {
      return ctx.audio;
    },
    entities: ctx.entities,
  };
}

// ---- 44: AudioDeps ------------------------------------------------------------------------------------------------

/** Read-only views over the stores and the entity table (44 section 2.2 `AudioStateReaders`). */
export interface AudioStateReaders {
  screen(): ScreenState;
  self(): ReturnType<ClientContext['stores']['selfStore']['getState']>;
  world(): ReturnType<ClientContext['stores']['worldStore']['getState']>;
  mover(objId: number): ReturnType<EntityTable['get']>;
  /** The live LH position of a mover from the sim's render view into `out`; `false` when the view does not know it. */
  posOf(objId: number, out: [number, number, number]): boolean;
  /** The own position (render view, else the 4 Hz store value) into `out`; `false` before the join. */
  selfPos(out: [number, number, number]): boolean;
  /** The own character fights in the 1:1 guild combat arena (T13: no death jingle); the provider is installed by the M7 combat feature. */
  inGuildCombat1to1?(): boolean;
  /** The own quest book for the quest jingles (T38, render-audio-m7); the observers run before the quest feature applies a message. */
  readonly quests?: {
    isActive(questId: number): boolean;
    isCompleted(questId: number): boolean;
    activeCount(): number;
  };
}

export interface AudioOptionsPort {
  get(): Readonly<Record<string, unknown>>;
  subscribe(fn: (options: Readonly<Record<string, unknown>>) => void): Unsubscribe;
}

export interface AudioDepsContract {
  readonly data: () => ClientDataIndex | null;
  readonly assets: AssetManager;
  readonly assetUrl: (path: string) => string;
  readonly now: () => number;
  readonly log: Logger;
  readonly visibility: { on(fn: (e: 'hidden' | 'visible' | 'pagehide') => void): Unsubscribe };
  readonly landHeight: (x: number, z: number) => number | undefined;
  readonly state: AudioStateReaders;
  readonly options: AudioOptionsPort;
  readonly notice: (text: string) => void;
}

/** Builds the `deps` of 44's `createAudio` (Plan/40 section 2.4, 44 section 2.2). */
export function audioDeps(
  ctx: ClientContext,
  parts: { options?: AudioOptionsPort; notice?: (text: string) => void } = {},
): AudioDepsContract {
  return {
    data: () => ctx.data,
    assets: ctx.assets,
    assetUrl: (path) => ctx.assetLayer.source.url({ ns: 'res', path }) ?? '',
    now: () => ctx.now(),
    log: ctx.log,
    visibility: {
      on: (fn) =>
        onVisibility((e) => {
          if (e === 'hidden' || e === 'visible' || e === 'pagehide') fn(e);
        }),
    },
    landHeight: (x, z) =>
      (ctx.sim as { landHeight?: (x: number, z: number) => number | undefined }).landHeight?.(x, z),
    state: {
      screen: () => ctx.machine.screen,
      self: () => ctx.stores.selfStore.getState(),
      world: () => ctx.stores.worldStore.getState(),
      mover: (objId) => ctx.entities.get(objId),
      posOf: (objId, out) => {
        const view = ctx.sim.view;
        return view?.valid === true && view.posOf(objId, out);
      },
      selfPos: (out) => {
        const self = ctx.stores.selfStore.getState();
        if (self.objId === 0) return false;
        const view = ctx.sim.view;
        if (view?.valid === true && view.posOf(self.objId, out)) return true;
        out[0] = self.pos[0];
        out[1] = self.pos[1];
        out[2] = self.pos[2];
        return true;
      },
      inGuildCombat1to1: () => inputProvidersOf(ctx).gc1to1?.participant() ?? false,
      // ---- render-audio-m7 (44 T38): the quest book the jingles compare the message with (the OLD book in phase Pre)
      quests: {
        isActive: (id) => questOf(ctx)?.record(id) !== undefined,
        isCompleted: (id) => questOf(ctx)?.store.getState().completed.includes(id) ?? false,
        activeCount: () => questOf(ctx)?.store.getState().active.length ?? 0,
      },
    },
    options: parts.options ?? NULL_OPTIONS,
    notice:
      parts.notice ??
      ((text) => {
        ctx.log.info('audio', text);
      }),
  };
}
