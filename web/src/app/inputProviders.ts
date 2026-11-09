// The late-bound answers features give the input layer (Plan/43 §17 "interfaces requested from other client docs"): the input host
// is built before the features, so what it needs from them is read through this per-context table at the moment of use. A feature
// sets its member when it registers (`inputProvidersOf(ctx).revival = ...`); an absent member means "no such feature yet" and the
// input layer takes its default.

import type { ScreenRect, Vec3LH } from '../render/types';
import type { ClientContext } from './context';

export interface InputProviders {
  /** `IsBullet()`: an arrow is in the bullet slot (the inventory, 45 WP-CFEAT-10). Default: every bow has arrows. */
  bulletEquipped?: () => boolean;
  /** The revival window (45 WP-CFEAT-12): the Esc chain opens it when it is closed and the player is dead. */
  revival?: { isOpen(): boolean; open(): void };
  /** `ShowMoverMenu` (45 §11.1, WP-CFEAT-18): a click on a player or an NPC within 20 m; `anchor` is its bound rect (CSS pixels). */
  moverMenu?: (id: number, anchor: ScreenRect | null) => void;
  /**
   * The player branch of `CMover::IsAttackAble` (`rules.clientAttackable`, 43 §7.6; the duel feature of 45 WP-CFEAT-36 answers it):
   * `0` not attackable, `1` attackable, `2` attackable and remembered as the last target. Absent: players are not attackable.
   */
  playerAttackable?: (id: number) => number;
  /**
   * The world-map overlay (45 §7.7, WP-CFEAT-28): the Esc chain closes it, a click on the 3-D view while it is shown goes to it (CSS pixels
   * of the canvas). Absent: no overlay.
   */
  worldMap?: { shown(): boolean; hide(): void; click?(x: number, y: number): void };
  /**
   * `m_vtInfo` of the own mover (45 WP-CFEAT-21, -22): `ownOpen` = the own personal shop is open (`IsVendorOpen`: the world stops moving and
   * clicking), `otherOpen` = `GetOther() != NULL` (a visited shop or a trade partner: the shop click of another player does nothing).
   */
  vendor?: { ownOpen(): boolean; otherOpen(): boolean };
  /** The create-monster placement mode of the item-use router (45 WP-CFEAT-13): the waiting item, its end and the send at the ground point. */
  createMonster?: { active(): boolean; leave(): void; place(pos: Vec3LH): boolean };
  /** Whether an object stands in a safe region (`IsRegionAttr(RA_SAFETY)` of another player, the personal-shop click). */
  regionSafe?: (objId: number) => boolean;
  /** The party-summon buff icon was double clicked (45 WP-CFEAT-34 opens `APP_SUMMON_PARTY_USE` when a summon is pending). */
  partySummon?: () => void;
  /** The item buff under a pointer position (`GetBuffIconRect`: the `II_SYS_*` item id), for a click that reaches the world. */
  buffIconAt?: (x: number, y: number) => number | null;
  /**
   * The 1:1 guild combat (45 WP-CFEAT-45, M7): `participant()` = the own character fights in the arena (the camera does not collide, 43 §8.4), `possible( id )` =
   * the mover is a participant of the arena whose menu stays closed to Alt + click (43 §4.4). Absent: inert, both answer `false` (the combat is off until M7).
   */
  gc1to1?: { participant(): boolean; possible(id: number): boolean };
  /**
   * The quest helper's NPC-position destination (`m_bSetQuestNPCDest` of V15, set by `QuestHelperNpcPos`; 45 WP-CFEAT-42, ui-quest): `active()` = the destination came from the NPC locator, `clear()`
   * drops only that flag and its distance label (`WndManager.cpp:3458-3459`, `World.cpp:296-297`; the destination itself stays). The input host (Esc step 6 of `input/escChain.ts`, the `setFocus`
   * clear) reads it. Absent: inert.
   */
  questDestination?: { active(): boolean; clear(): void };
  // ---- housing / guild house (ui-house-quiz)
  /**
   * The placement of a housing or guild-house furniture (`CDeployManager`, `CGuildDeployManager`; 45 WP-CFEAT-46, `features/events/deploy`): `active()` while a ghost follows the pointer, `cursor( p )` with the terrain point under the
   * pointer (`null` keeps the last one), `confirm()` the click, `cancel()` Esc (`true` when a placement ended), `wheel( n )` the rotation by the wheel, `pageKey( code )` PgUp / PgDn. Absent: inert.
   */
  deploy?: {
    active(): boolean;
    cursor(p: Vec3LH | null): void;
    confirm(): void;
    cancel(): boolean;
    wheel(notches: number): void;
    pageKey(code: string): void;
  };
  /** `ShowCCtrlMenu` (45 WP-CFEAT-46): the furniture menu of a clicked `CK1_GUILD_HOUSE` ctrl; `anchor` is its bound rect (CSS pixels). */
  ctrlMenu?: (id: number, anchor: ScreenRect | null) => void;
  /**
   * `GuildHouse->m_dwSelectedObjID` (45 WP-CFEAT-46): `select( id )` when the focus lands on a guild-house furniture (the storage window follows), `clear()` on Esc (`NULL_ID`).
   */
  guildFurniture?: { select(id: number): void; clear(): void };
}

const tables = new WeakMap<ClientContext, InputProviders>();

/** The provider table of a context (created on first use). */
export function inputProvidersOf(ctx: ClientContext): InputProviders {
  let t = tables.get(ctx);
  if (!t) {
    t = {};
    tables.set(ctx, t);
  }
  return t;
}
