// The machine-readable window catalog (Plan/42 §19.5): one entry per `resdata.inc` layout (365) with its priority,
// milestone, batch work package and behaviour owner. The rows are generated from the plan tables
// (`tools/gen_catalog.py` -> `catalog.data.ts`); this file adds the part that is edited by hand: the table of
// built windows (`BUILT_CONTROLLERS`, one line per ported window; the window batch WPs add theirs).

import type { WndNeuz } from './wnd/WndNeuz';
import { CATALOG_ROWS } from './catalog.data';
import { HUD_CONTROLLERS } from './hudControllers';
import { APPEARANCE_CONTROLLERS } from './windows/appearance/controllers';
import { BANK_CONTROLLERS } from './windows/bank/controllers';
import { CHAR_CONTROLLERS } from './windows/char/controllers';
import { COUPLE_CONTROLLERS } from './windows/couple/controllers';
import { DIALOG_CONTROLLERS } from './windows/dialogs/controllers';
import { DUEL_CONTROLLERS } from './windows/duel/controllers';
import { EXCHANGE_CONTROLLERS } from './windows/exchange/controllers';
import { GUILD_CONTROLLERS } from './windows/guild/controllers';
import { ITEMUSE_CONTROLLERS } from './windows/itemuse/controllers';
import { JOB_CONTROLLERS } from './windows/jobs/controllers';
import { LOGIN_CONTROLLERS } from './windows/login/controllers';
import { MESSENGER_CONTROLLERS } from './windows/messenger/controllers';
import { MAIL_CONTROLLERS } from './windows/mail/controllers';
import { NAV_CONTROLLERS } from './windows/nav/controllers';
import { NPC_CONTROLLERS } from './windows/npc/controllers';
import { PARTY_CONTROLLERS } from './windows/party/controllers';
import { PET_CONTROLLERS } from './windows/pets/controllers';
import { POCKET_CONTROLLERS } from './windows/pockets/controllers';
import { QUEST_CONTROLLERS } from './windows/quest/controllers';
// ---- lord (ui-lord, M7)
import { GC1TO1_CONTROLLERS } from './windows/gc1to1/controllers';
import { LORD_CONTROLLERS } from './windows/lord/controllers';
// ---- housing / guild house (ui-house-quiz)
import { HOUSING_CONTROLLERS } from './windows/housing/controllers';
import { QUIZ_CONTROLLERS } from './windows/quiz/controllers';
// ---- secret room and tax (ui-secret-tax, M7)
import { SECRETROOM_CONTROLLERS } from './windows/secretroom/controllers';
// ---- rainbow race and quiz (ui-rainbow)
import { RAINBOW_CONTROLLERS } from './windows/rainbow/controllers';
// ---- mini-games (ui-minigames, M7)
import { MINIGAME_CONTROLLERS } from './windows/minigame/controllers';
import { READABLE_CONTROLLERS } from './windows/readable/controllers';
import { SIEGE_CONTROLLERS } from './windows/siege/controllers';
import { SKILL_CONTROLLERS } from './windows/skills/controllers';
import { TRADE_CONTROLLERS } from './windows/trade/controllers';
import { VENDOR_CONTROLLERS } from './windows/vendor/controllers';
import { ULTIMATE_CONTROLLERS } from './windows/ultimate/controllers';
import { UPGRADE_CONTROLLERS } from './windows/upgrade/controllers';

export type Priority = 'P0' | 'P1' | 'P2' | 'Drop' | 'Defer';
export type Milestone = 'M1' | 'M2' | 'M3' | 'M4' | 'M5' | 'M6' | 'M7' | 'M8';
/** A window shell class: any constructor that builds a `WndNeuz` (shells take their ports as arguments). */
export type WndCtor = abstract new (...args: never[]) => WndNeuz;

/** `[name, id, prio, milestone, wp, group, owner, note]` (the generated row shape). */
export type CatalogRow = readonly [
  name: string,
  id: number,
  prio: Priority,
  m: Milestone | null,
  wp: string | null,
  group: string,
  owner: string,
  note: string,
];

export interface CatalogEntry {
  readonly name: string;
  /** The `ResData.h` APP id (crosses the wire inside taskbar applet shortcuts). */
  readonly id: number;
  readonly prio: Priority;
  /** The milestone in which the window must be complete (`null` for Drop and Defer). */
  readonly m: Milestone | null;
  /** The batch work package of Plan/42 §24 that ports it (`null` for Drop and Defer). */
  readonly wp: string | null;
  /** `true` once the shell exists and is registered in {@link BUILT_CONTROLLERS}. */
  readonly built: boolean;
  /** Group letter of Plan/42 §19.4 (`-` for Drop and Defer). */
  readonly group: string;
  /** Who owns the behaviour behind the window ports. */
  readonly owner: string;
  /** Why a Drop/Defer layout is never built (empty otherwise). */
  readonly note: string;
  /** Lazy loader of the shell class (present exactly when `built`). */
  readonly controller?: () => Promise<WndCtor>;
}

/** The status shown by the `/dev/ui` index. */
export type CatalogStatus = 'built' | 'pending' | 'drop' | 'defer';

/**
 * The windows that exist: `APP_*` name -> lazy loader of the shell class. A window batch WP adds one line per
 * window here (`APP_INVENTORY: async () => (await import('./windows/inventory/WndInventory')).WndInventory`).
 * Drop and Defer layouts must never appear (the `ui_catalog_built` test).
 */
export const BUILT_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  ...LOGIN_CONTROLLERS,
  ...NAV_CONTROLLERS,
  ...HUD_CONTROLLERS,
  ...CHAR_CONTROLLERS,
  ...DIALOG_CONTROLLERS,
  ...DUEL_CONTROLLERS,
  ...ITEMUSE_CONTROLLERS,
  ...APPEARANCE_CONTROLLERS,
  ...BANK_CONTROLLERS,
  ...MAIL_CONTROLLERS,
  ...JOB_CONTROLLERS,
  ...NPC_CONTROLLERS,
  ...PARTY_CONTROLLERS,
  ...MESSENGER_CONTROLLERS,
  ...GUILD_CONTROLLERS,
  ...SIEGE_CONTROLLERS,
  ...COUPLE_CONTROLLERS,
  ...PET_CONTROLLERS,
  ...POCKET_CONTROLLERS,
  ...SKILL_CONTROLLERS,
  ...TRADE_CONTROLLERS,
  ...VENDOR_CONTROLLERS,
  ...READABLE_CONTROLLERS,
  ...ULTIMATE_CONTROLLERS,
  ...UPGRADE_CONTROLLERS,
  ...EXCHANGE_CONTROLLERS,
  ...QUEST_CONTROLLERS,
  // ---- lord (ui-lord, M7)
  ...LORD_CONTROLLERS,
  ...GC1TO1_CONTROLLERS,
  // ---- housing / guild house (ui-house-quiz)
  ...HOUSING_CONTROLLERS,
  ...QUIZ_CONTROLLERS,
  // ---- secret room and tax (ui-secret-tax, M7)
  ...SECRETROOM_CONTROLLERS,
  // ---- rainbow race and quiz (ui-rainbow)
  ...RAINBOW_CONTROLLERS,
  // ---- mini-games (ui-minigames, M7)
  ...MINIGAME_CONTROLLERS,
};

function buildCatalog(): Record<string, CatalogEntry> {
  const out: Record<string, CatalogEntry> = {};
  for (const [name, id, prio, m, wp, group, owner, note] of CATALOG_ROWS) {
    const controller = BUILT_CONTROLLERS[name];
    out[name] = {
      name,
      id,
      prio,
      m,
      wp,
      built: controller !== undefined,
      group,
      owner,
      note,
      ...(controller ? { controller } : {}),
    };
  }
  return out;
}

/** One entry per layout, keyed by `APP_*` name (365 entries). */
export const WINDOW_CATALOG: Readonly<Record<string, CatalogEntry>> = buildCatalog();

/** The entry of an APP id. */
export function catalogById(id: number): CatalogEntry | undefined {
  return CATALOG_BY_ID.get(id);
}
const CATALOG_BY_ID: ReadonlyMap<number, CatalogEntry> = new Map(
  Object.values(WINDOW_CATALOG).map((e) => [e.id, e]),
);

export function catalogStatus(e: CatalogEntry): CatalogStatus {
  if (e.prio === 'Drop') return 'drop';
  if (e.prio === 'Defer') return 'defer';
  return e.built ? 'built' : 'pending';
}

/** The numbers of Plan/42 §19.2: P0 28, P1 62, P2 170, Drop 98, Defer 7 (changed only with a gate ADR). */
export function catalogCounts(): Record<Priority, number> & { total: number } {
  const counts = { P0: 0, P1: 0, P2: 0, Drop: 0, Defer: 0, total: 0 };
  for (const e of Object.values(WINDOW_CATALOG)) {
    counts[e.prio] += 1;
    counts.total += 1;
  }
  return counts;
}
