// The feature module contract (Plan/45 §2.2, WP-CFEAT-01): every feature of `web/src/features/` is a `FeatureModule`
// registered at boot (before the WebSocket opens, step B5) with a `FeatureContext`: the narrow set of things a feature
// may use (the handler registry, the UI, the texts, the data, the rules, the features config, audio, the entity table).
// Rules: handlers only reduce into slices and trigger windows; no handler awaits; every reducer is a pure function with
// a table test.
//
// `FeatureContext` extends 45's list by `app` (the client context, used by the lobby and session features that talk to
// the screen state machine of doc 40: 45 §1.6 "this doc opens/closes the 42 windows in reaction to the state"), `options`
// (the persisted options store), `chat` (42's chat router once it exists), `gate` and `transactional` (the per-context
// helpers of §2.4/§2.5).

import type { ClientContext } from '../../app/context';
import type { HandlerModule } from '../../app/features';
import { featureContext, type FeatureContextContract } from '../../app/hosts';
import type { ClientDataIndex, TextApi } from '../../data';
import type { EntityTable } from '../../game/entities';
import type { AudioApi } from '../../game/ports';
import type { ClientFeatureConfig } from '../../gen/config/clientFeatures';
import type { UiApi } from '../../ui';
import type { OptionsStore } from '../options';
import { defineOptionsStore } from '../options';
import { detectLobbyRules, type LobbyRules } from './lobbyRules';
import { sharedOf } from './shared';
import type { ChatPort } from './text';
import type { RequestGate } from './requestGate';
import type { TransactionalRegistry } from './transactional';

/**
 * The feature ids of 45 §2.2 plus `screens` (the screen bodies, the UI mounting and logout/quit of `features/session/`; the id
 * `session` is taken by the core handler module of doc 40).
 */
export type FeatureId =
  | 'lobby'
  | 'loading'
  | 'screens'
  | 'hud'
  | 'status'
  | 'chat'
  | 'taskbar'
  | 'inventory'
  | 'tooltip'
  | 'buffs'
  | 'itemuse'
  | 'appearance'
  | 'npc'
  | 'bank'
  | 'trade'
  | 'vendor'
  | 'mail'
  | 'pockets'
  | 'upgrade'
  | 'ultimate'
  | 'exchange'
  | 'pet'
  | 'skills'
  | 'jobchange'
  | 'motions'
  | 'character'
  | 'party'
  | 'guild'
  | 'messenger'
  | 'relation'
  | 'pvp'
  | 'map'
  | 'options'
  | 'notice'
  | 'modelView'
  | 'quest'
  | 'events'
  | 'guildmarks'
  | 'gm';

export interface FeatureContext {
  readonly id: FeatureId;
  readonly net: FeatureContextContract['net'] & {
    /** Flushes the sender queue at once (lobby actions flush themselves, 40 §5.2 S4). */
    flush(): void;
  };
  /** 42's UI (`wm`, `msgbox`, ...); `null` until boot step B5 created it. */
  readonly ui: UiApi | null;
  readonly texts: TextApi;
  readonly data: ClientDataIndex | null;
  /** `RulesContext` (M3); `null` before. */
  readonly rules: object | null;
  /** The free wasm lobby rules; `null` while the module does not export them (45 §5.2 fallback). */
  readonly lobbyRules: LobbyRules | null;
  readonly features: ClientFeatureConfig;
  readonly audio: AudioApi;
  readonly entities: EntityTable;
  readonly options: OptionsStore;
  readonly chat: ChatPort | null;
  readonly gate: RequestGate;
  readonly transactional: TransactionalRegistry;
  /** The client context (screen state machine, session actions, stores, probe). */
  readonly app: ClientContext;
}

export interface FeatureModule {
  readonly id: FeatureId;
  readonly milestone: 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;
  /** Called once at boot, before the WebSocket opens. */
  register(fc: FeatureContext): void;
}

/** The full UI API when `createUi` ran (`ctx.ui` is typed as the loop port `{ process }`). */
export function fullUi(ctx: ClientContext): UiApi | null {
  const ui = ctx.ui as Partial<UiApi> | null;
  return ui?.wm ? (ui as UiApi) : null;
}

/** 42's chat router once the hud-chat work package provides `ui.chat.putString`; `null` before. */
function chatOf(ctx: ClientContext): ChatPort | null {
  const chat = (ctx.ui as { chat?: Partial<ChatPort> } | null)?.chat;
  return typeof chat?.putString === 'function' ? (chat as ChatPort) : null;
}

/** Builds the `FeatureContext` of a feature from the client context (the adapter of 40 §2.4, extended). */
export function createFeatureContext(ctx: ClientContext, id: FeatureId): FeatureContext {
  const base = featureContext(ctx, id);
  const shared = sharedOf(ctx);
  const options = defineOptionsStore(ctx.stores.registry);
  return {
    id,
    net: {
      on: (c, handler) => {
        base.net.on(c, handler);
      },
      send: (msg, options) => base.net.send(msg, options),
      flush: () => {
        ctx.net.sender.flush();
      },
    },
    get ui() {
      return fullUi(ctx);
    },
    get texts() {
      return base.texts;
    },
    get data() {
      return ctx.data;
    },
    get rules() {
      return ctx.rules;
    },
    get lobbyRules() {
      return detectLobbyRules();
    },
    get features() {
      return ctx.features;
    },
    get audio() {
      return ctx.audio;
    },
    entities: ctx.entities,
    options,
    get chat() {
      return chatOf(ctx);
    },
    gate: shared.gate,
    transactional: shared.transactional,
    app: ctx,
  };
}

/** Adapts a `FeatureModule` to the handler-module list of `app/features.ts` (its `FEATURE_MODULES` group). */
export function toHandlerModule(m: FeatureModule): HandlerModule {
  return {
    id: m.id,
    register(ctx) {
      m.register(createFeatureContext(ctx, m.id));
    },
  };
}
