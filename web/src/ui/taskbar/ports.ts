// The ports of the taskbar (Plan/42 §11.5-§11.6, 45 §19.4; WP-UI-14/19): what the taskbar needs from the feature layer.
// Everything is a plain TypeScript interface (no protobuf types); the implementations sit in `features/taskbar/`.

import type { Shortcut } from '../dnd/shortcut';

/** What a slot looks like when it is painted: an icon (VFS path of the exact form), a stack count, a cooldown. */
export interface SlotIcon {
  readonly path: string;
  /** The number drawn at the bottom right (items with `dwPackMax > 1`: the total of that item in the inventory). */
  readonly count?: number;
  /** The cooldown group of an item / the reuse delay of a skill: absolute `host.now()` ms. */
  readonly cooldown?: { readonly startMs: number; readonly endMs: number } | null;
}

/** The messages of §11.6 (`// legacy-send` markers sit in `features/taskbar`). */
export interface TaskbarActions {
  /** `SendAddAppletTaskBar` */
  addApplet(slot: number, sc: Shortcut): void;
  /** `SendRemoveAppletTaskBar` */
  removeApplet(slot: number): void;
  /** `SendAddItemTaskBar` */
  addItem(page: number, slot: number, sc: Shortcut): void;
  /** `SendRemoveItemTaskBar` */
  removeItem(page: number, slot: number): void;
  /** `SendSkillTaskBar`: all 5 queue slots. */
  sendQueue(queue: readonly (Shortcut | null)[]): void;
}

/** What the drop rules need to know about a skill (`g_pPlayer->GetSkill` + the skill prop). */
export interface SkillDropInfo {
  /** `dwLevel` of the player's skill (party skills: always usable). */
  readonly level: number;
  /** `CheckSkill`: the skill may be used (weapon, job, ...). */
  readonly usable: boolean;
  /** `dwComboStyle` of the skill prop (`CT_STEP` 1, `CT_CIRCLE` 2, `CT_FINISH` 3, `CT_GENERAL` 4). */
  readonly combo: number;
  /** `AddSkillProp.dwCooldown` of the player's level in ms (`> 0`: the skill cannot go into the action slot); absent = none. */
  readonly cooldown?: number;
  /** `szName` of the skill (the refusal line). */
  readonly name?: string;
}

/**
 * The rules of the action slot that 45 owns (WP-CFEAT-32): `CheckAddSkill` and the `SkillTaskBar` send. The row mechanics of 42
 * (`ActionQueue`, WP-UI-42) call them.
 */
export interface QueueActions {
  /**
   * `CWndTaskBar::CheckAddSkill(styleSrc, dest)`: may a skill of combo style `src` go into queue slot `dest`? `styles[i]` is the combo
   * style of the skill in queue slot `i` (0 for an empty slot) and `count` the number of queued skills.
   */
  checkAddSkill(src: number, dest: number, styles: readonly number[], count: number): boolean;
  /** `SendSkillTaskBar`: all five queue slots (`null` = empty) go to the server. */
  send(queue: readonly (Shortcut | null)[]): void;
}

/** The item side: stackable items follow their stacks (`m_dwItemId`, `m_Inventory.Find`). */
export interface TaskbarItemPort {
  /** The inventory has been received (before that no ITEM slot is touched). */
  ready?(): boolean;
  /** The inventory item with this object id exists. */
  has(objId: number): boolean;
  /** `m_dwItemId` of an inventory item when it is stackable (`dwPackMax > 1`), else `null`. */
  stackItemId(objId: number): number | null;
  /** The object id of the first inventory stack of this item id (`m_Inventory.Find`), `null` = none. */
  find(itemId: number): number | null;
  /** `dwPackMax > 1` for an item id. */
  stackable(itemId: number): boolean;
}

/** The action-queue rules (45 WP-CFEAT-32 / WP-UI-42): drops on the queue row go through here. */
export interface TaskbarQueuePort {
  /** `SetSkillQueue(index, type, id)`: `false` = refused (the forbid icon shows). */
  add(index: number, type: number, id: number, icon: string | null): boolean;
  /** `RemoveSkillQueue(index, send)`. */
  remove(index: number, send?: boolean): void;
}

/** The ports of the shortcut kinds (§11.5, 45 §19.4); a missing port makes its kind do nothing. */
export interface ShortcutPorts {
  items?: { useFromShortcut(objId: number): void };
  skills?: { useShortcut(type: number, id: number): void };
  motions?: { execute(id: number): void };
  chat?: { sendMacro(text: string): void; sendEmoticon(index: number): void };
  // ---- lord (ui-lord, M7): the source of the LORDSKILL slots; `id` is the stored `LI_*` id 0..7 (OQ7)
  lord?: {
    useSkill(id: number): void;
    /** The icon file of the skill (`icon/<file>`), `null` for none. */
    icon?(id: number): string | null;
    /** The cooldown radar of the skill (absolute `host.now()` ms, like `SlotIcon.cooldown`), `null` while it is ready. */
    cooldown?(id: number): { startMs: number; endMs: number } | null;
    /** The tooltip of the slot in the markup of the original (`#b#cff2fbe6d<name>#nb#nc` ...), `null` for an unknown skill. */
    tooltip?(id: number): string | null;
  };
  messenger?: { openMessage(id: number): void };
  actionSlot?: { use(): void };
  /** `APP_TRADE` in an applet slot requests a trade with the focus mover instead of opening a window. */
  trade?: { request(): void };
}

/** What the taskbar needs from the feature layer (45) and the UI services. */
export interface TaskbarPorts {
  /** Executes a shortcut that is not an applet (§11.5). Applets go through `wm.objectExecutorApplet`. */
  execute(sc: Shortcut): void;
  /** The icon of an ITEM / SKILL / MOTION / EMOTICON / CHAT / ... slot; `null` draws nothing (and empties nothing). */
  slotIcon(sc: Shortcut): SlotIcon | null;
  /** The tooltip text of a non-applet slot (when the item/skill composers do not apply). */
  slotTooltip?(sc: Shortcut): string | null;
  /** Item and skill tooltips of a slot (the tooltip host with the composers of 45); `true` = shown. */
  slotTooltipRich?(
    sc: Shortcut,
    ownerRect: { l: number; t: number; r: number; b: number },
  ): boolean;
  readonly actions: TaskbarActions;
  /** `features.new_font_color`: the colour of stack counts. */
  newFontColor?(): boolean;
  /** The item of an ITEM slot is worn equipment (43's `g_bSlotSwitchAboutEquipItem` latch, `slotIsEquipItem`). */
  slotIsEquip?(sc: Shortcut): boolean;
  /** Skills in drops (level, usability, combo style); without it every skill drop is refused. */
  skills?: { info(type: number, id: number): SkillDropInfo | null } | undefined;
  items?: TaskbarItemPort | undefined;
  queue?: TaskbarQueuePort | undefined;
  /** A system-chat line of a text id (`PutString(GETTEXT(tid), ...)`). */
  message?(tid: number): void;
  /** `APP_TRADE` slot: request a trade with the focus mover. */
  tradeRequest?: (() => void) | undefined;
  /** The rules of the action slot (45 WP-CFEAT-32); without them (and without `queue`) the queue row refuses every edit. */
  queueActions?: QueueActions | undefined;
  /** A system-chat line of a text id with `%s` arguments (`TID_GAME_SKILLLNOTUSE` with the skill name). */
  messageFmt?(tid: number, ...args: string[]): void;
  /** `IsSMMode(SM_ACTPOINT)`: the queue costs no action points (no AP shade, no lamp). */
  actionPointsFree?(): boolean;
  /** Something on the bar changes by itself (a cool-down runs): the window repaints about ten times a second while this is `true`. */
  animating?(): boolean;
  /** The text of the action-queue tooltip (`ap` points, `cost` of the slot under the pointer, 0 for the first slot). */
  queueTooltip?(ap: number, slot: number, cost: number): string | null;
  /** The icon file of a skill / motion / applet shortcut dropped on the bar (the drag payload may lack it). */
  iconOf?(sc: Shortcut): string | null;
}
