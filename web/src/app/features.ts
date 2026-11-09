// Handler modules (Plan/40 section 5.4): the four groups that register handlers with the registry, in this
// fixed order. A module added later appends itself to its group; reordering inside a group needs a review
// because it changes the handler order inside a phase. `registerHandlerModules` runs at boot step B5, before
// the WebSocket opens, and freezes the registry.

import { combatModule } from '../game/combat';
import { fxModule } from '../game/fx';
import { rulesModule } from '../game/rules';
import { WORLD_MODULES } from '../game/world';
import {
  ACTIONSLOT_M5_FEATURE_MODULES,
  BANK_MAIL_M4B_FEATURE_MODULES,
  CFEAT_M3_FEATURE_MODULES,
  GUILD_M6_FEATURE_MODULES,
  GUILD_MARKS_M6_FEATURE_MODULES,
  // ---- housing / guild house (ui-house-quiz)
  HOUSE_M7_FEATURE_MODULES,
  HUD_CHAT_FEATURE_MODULES,
  // ---- rainbow race and quiz (ui-rainbow)
  RAINBOW_M7_FEATURE_MODULES,
  // ---- mini-games, tickets, attendance, GC 1:1 (ui-minigames, M7)
  MINIGAMES_M7_FEATURE_MODULES,
  ITEMS_M4_FEATURE_MODULES,
  ITEMUSE_M4_FEATURE_MODULES,
  LORD_M7_FEATURE_MODULES,
  MESSENGER_M6_FEATURE_MODULES,
  NAV_M4B_FEATURE_MODULES,
  NPC_M4_FEATURE_MODULES,
  PARTY_M5_FEATURE_MODULES,
  PETS_M4B_FEATURE_MODULES,
  PVP_M5_FEATURE_MODULES,
  QUEST_M7_FEATURE_MODULES,
  RELATION_M6_FEATURE_MODULES,
  SCREEN_FEATURE_MODULES,
  // ---- secret room and tax (ui-secret-tax, M7)
  SECRET_TAX_M7_FEATURE_MODULES,
  SIEGE_M6_FEATURE_MODULES,
  SKILL_WINDOWS_M5_FEATURE_MODULES,
  TRADE_VENDOR_M4_FEATURE_MODULES,
  UI_M3_FEATURE_MODULES,
  ULTIMATE_M4B_FEATURE_MODULES,
  UPGRADE_M4B_FEATURE_MODULES,
} from '../features';
import { connectionModule } from './connectionModule';
import type { ClientContext } from './context';
import { sessionModule } from './sessionModule';

export interface HandlerModule {
  /** Unique, used in error messages and the handler's `owner` stat key. */
  readonly id: string;
  register(ctx: ClientContext): void;
}

/** 1. Core modules of doc 40 (phases Core/Sim): session, world (spawn/join/replace), movement, environment. */
export const CORE_MODULES: HandlerModule[] = [
  connectionModule,
  sessionModule,
  // before the world modules: its drain-end hook applies the DST rows before the world's `SET_PARAMS` flush (one record per mover)
  rulesModule,
  ...WORLD_MODULES,
];

/** 2. Runtime modules placed under `game/` by server docs (17 combat, skills, buffs ...). */
export const RUNTIME_MODULES: HandlerModule[] = [combatModule, fxModule];

/** 3. 45's feature modules (`web/src/features/`), registered with their `FeatureContext` (WP-CLIENT-15), in 45's order. */
export const FEATURE_MODULES: HandlerModule[] = [
  ...SCREEN_FEATURE_MODULES,
  ...HUD_CHAT_FEATURE_MODULES,
  ...UI_M3_FEATURE_MODULES,
  ...CFEAT_M3_FEATURE_MODULES,
  ...NPC_M4_FEATURE_MODULES,
  ...ITEMS_M4_FEATURE_MODULES,
  ...ITEMUSE_M4_FEATURE_MODULES,
  ...ACTIONSLOT_M5_FEATURE_MODULES,
  ...PVP_M5_FEATURE_MODULES,
  ...SKILL_WINDOWS_M5_FEATURE_MODULES,
  ...PARTY_M5_FEATURE_MODULES,
  ...TRADE_VENDOR_M4_FEATURE_MODULES,
  ...BANK_MAIL_M4B_FEATURE_MODULES,
  ...ULTIMATE_M4B_FEATURE_MODULES,
  ...UPGRADE_M4B_FEATURE_MODULES,
  ...NAV_M4B_FEATURE_MODULES,
  ...PETS_M4B_FEATURE_MODULES,
  ...MESSENGER_M6_FEATURE_MODULES,
  ...GUILD_M6_FEATURE_MODULES,
  ...SIEGE_M6_FEATURE_MODULES,
  ...RELATION_M6_FEATURE_MODULES,
  ...GUILD_MARKS_M6_FEATURE_MODULES,
  ...QUEST_M7_FEATURE_MODULES,
  ...LORD_M7_FEATURE_MODULES,
  // ---- secret room and tax (ui-secret-tax, M7)
  ...SECRET_TAX_M7_FEATURE_MODULES,
  // ---- housing / guild house (ui-house-quiz)
  ...HOUSE_M7_FEATURE_MODULES,
  // ---- rainbow race and quiz (ui-rainbow)
  ...RAINBOW_M7_FEATURE_MODULES,
  // ---- mini-games, tickets, attendance, GC 1:1 (ui-minigames, M7)
  ...MINIGAMES_M7_FEATURE_MODULES,
];

/** 4. Observers: 44 audio (Pre/Fx), debug probe counters (Fx). */
export const OBSERVERS: HandlerModule[] = [];

export const HANDLER_MODULES: readonly HandlerModule[] = [
  ...CORE_MODULES,
  ...RUNTIME_MODULES,
  ...FEATURE_MODULES,
  ...OBSERVERS,
];

/**
 * Registers every module in order and freezes the registry. A module that throws aborts the boot step
 * (recovery L3 once, then L4, section 3.1), named in the error.
 */
export function registerHandlerModules(
  ctx: ClientContext,
  modules: readonly HandlerModule[] = HANDLER_MODULES,
): void {
  const seen = new Set<string>();
  for (const m of modules) {
    if (seen.has(m.id)) throw new Error(`handler module "${m.id}" is registered twice`);
    seen.add(m.id);
    try {
      m.register(ctx);
    } catch (error) {
      throw new Error(
        `handler module "${m.id}" failed to register: ${error instanceof Error ? error.message : String(error)}`,
        { cause: error },
      );
    }
  }
  ctx.net.registry.freeze();
}
