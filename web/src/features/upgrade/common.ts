// What the upgrade features share (Plan/45 §18.1; WP-CFEAT-24, WP-CFEAT-25): the reservation of an item for a window, the shell's view of an inventory item,
// the system lines and message boxes, the random-option lines of an item, and the "window is open" menu rule. Everything reads the inventory feature and the
// UI of the context; nothing here sends.

import type { ClientContext } from '../../app/context';
import { APP_INVENTORY, APP_TRADE } from '../../gen/defs/resdata';
import { TID_GAME_WND_OVERLAPED } from '../../gen/defs/text';
import type { RandomOptionKind, RandomOptionLine } from '../../rules/types';
import type { UiApi } from '../../ui';
import type { DialogItem } from '../../ui/windows/dialogs/ports';
import { fullUi, type FeatureContext } from '../common/module';
import { formatTid, SYSTEM_CHAT } from '../common/text';
import type { InventoryFeature } from '../inventory';
import { levelDown } from '../tooltip/sections/requirements';
import type { InvItem } from '../inventory/items';
import { dstAwakeLine } from '../tooltip/dst';
import { randomOptionKind } from '../tooltip/sections/awakening';
import { tooltipOf } from '../tooltip/module';
import type { ItemTooltipNumbers } from '../../rules/types';

export interface UpgradeCommon {
  readonly fc: FeatureContext;
  readonly ctx: ClientContext;
  readonly inv: InventoryFeature;
  /** The full UI, `null` on a title screen. */
  ui(): UiApi | null;
  /** The text of a `TID_*` (the data index first, the feature texts as a fallback). */
  text(tid: number): string;
  /** A client-side system line in the colour of the text (`PutString( prj.GetText( tid ), NULL, prj.GetTextColor( tid ) )`). */
  say(tid: number, args?: readonly (string | number)[]): void;
  /** A client-side system line with its own colour (`PutString( text, NULL, color )`). */
  sayRaw(text: string, color: number): void;
  /** `OpenMessageBox( text )`. */
  msgbox(text: string): void;
  /** `SetExtra( GetExtra() + 1 )` (`on`) / `SetExtra( 0 )` (`off`). */
  reserve(objId: number, on: boolean): void;
  /** Replaces fields of an inventory item (an answer of the server that has no `UpdateItem` parameter: the commercial stone). */
  patchItem(objId: number, patch: Partial<InvItem>): void;
  /** The shell's view of an inventory object, `null` when it is gone. */
  dialogItem(objId: number): DialogItem | null;
  /** The random-option kind of an item (`GetRandomOptionKind`), `null` for an item that has none. */
  optionKind(item: InvItem): RandomOptionKind | null;
  /** The decoded lines of an item's random-option word, or of `word` instead when given (the offered set of a protected awakening). */
  optionLines(item: InvItem, word?: bigint): readonly RandomOptionLine[];
  /** One line as the window prints it (`"STR +3"`, `"Attack -5%"`). */
  lineText(line: RandomOptionLine): string;
  /** `GetLevelDown()`: `-5`, `-10` or `0`. */
  levelDown(item: InvItem): number;
  /** A trade is open (`m_vtInfo.GetOther()`) or the player's own shop is open. */
  tradingOrVending(): boolean;
  /** The `help` panel text of a window (`Client/<name>.inc`), `''` when the bundle has none. */
  panel(name: string): string;
}

/** The numbers of the tooltip module over an inventory item's view (the same call the item tooltips make). */
function numbersOf(
  ctx: ClientContext,
  inv: InventoryFeature,
  item: InvItem,
  word: bigint | undefined,
): ItemTooltipNumbers | null {
  const view = inv.itemView(item);
  const tooltip = tooltipOf(ctx);
  if (!view || !tooltip) return null;
  const v = word === undefined ? view : { ...view, item: { ...view.item, randomOpt: word } };
  const answer = tooltip.numbers(v, { flag: APP_INVENTORY, now: ctx.now() });
  return (answer?.detail ?? null) as ItemTooltipNumbers | null;
}

export function createUpgradeCommon(fc: FeatureContext, inv: InventoryFeature): UpgradeCommon {
  const ctx = fc.app;
  const text = (tid: number): string => ctx.data?.text(tid) ?? fc.texts.text(tid);
  const colorOf = (tid: number): number => (ctx.data ?? fc.texts).textColor(tid);
  const chat = (): FeatureContext['chat'] => fc.chat;
  const common: UpgradeCommon = {
    fc,
    ctx,
    inv,
    ui: () => fullUi(ctx),
    text,
    say(tid, args = []) {
      chat()?.putString(formatTid(fc.texts, tid, args), null, colorOf(tid), SYSTEM_CHAT);
    },
    sayRaw(line, color) {
      chat()?.putString(line, null, color, SYSTEM_CHAT);
    },
    msgbox(line) {
      fullUi(ctx)?.msgbox.open(line);
    },
    reserve(objId, on) {
      const it = inv.item(objId);
      if (!it) return;
      inv.setExtra(objId, on ? it.extra + 1 : 0);
    },
    patchItem(objId, patch) {
      const s = inv.store.getState();
      const it = s.container.getAtId(objId);
      if (!it) return;
      const c = s.container.clone();
      c.replaceAtId(objId, { ...it, ...patch });
      inv.store.setState({
        ...s,
        container: c,
        version: s.version + 1,
        // the composed tooltips are cached per `(serial, equipVersion)`: the element or the stone changed the lines of this one
        equipVersion: s.equipVersion + 1,
      });
    },
    dialogItem(objId) {
      const it = inv.item(objId);
      return it ? inv.itemView(it) : null;
    },
    optionKind(item) {
      const prop = ctx.data?.item(item.itemId);
      if (!prop) return null;
      return randomOptionKind(
        prop.parts,
        prop.item_kind3,
        (item.pet?.level ?? 0) >= 2,
        ctx.features.features.only_armor_awakes,
      );
    },
    optionLines(item, word) {
      return numbersOf(ctx, inv, item, word)?.randomOptions ?? [];
    },
    lineText(line) {
      return dstAwakeLine(line.dst, line.adj, text).trim();
    },
    levelDown(item) {
      return levelDown(item.randomOpt);
    },
    tradingOrVending() {
      const ui = fullUi(ctx);
      return ui !== null && (ui.wm.isVendorWindowOpen() || ui.wm.isOpen(APP_TRADE));
    },
    panel(name) {
      return ctx.bundles.get('help')?.panels[name] ?? '';
    },
  };
  return common;
}

/**
 * The `GetWndBase( app )` refusal of the NPC menus: a window of the kind is already open -> `TID_GAME_WND_OVERLAPED` and the menu does nothing. Returns
 * `true` when the menu may go on.
 */
export function freeOrSay(common: UpgradeCommon, app: number): boolean {
  const ui = common.ui();
  if (!ui) return false;
  if (ui.wm.isOpen(app)) {
    common.say(TID_GAME_WND_OVERLAPED);
    return false;
  }
  return true;
}
